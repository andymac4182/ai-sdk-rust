use std::collections::{BTreeMap, BTreeSet, VecDeque};
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
    LanguageModelCallOptions, LanguageModelContent, LanguageModelCustomContent,
    LanguageModelDocumentSource, LanguageModelErrorStreamPart, LanguageModelFilePart,
    LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelMessage,
    LanguageModelProviderTool, LanguageModelRawStreamPart, LanguageModelReasoning,
    LanguageModelReasoningDelta, LanguageModelReasoningEffort, LanguageModelReasoningEnd,
    LanguageModelReasoningStart, LanguageModelRequest, LanguageModelResponse,
    LanguageModelResponseFormat, LanguageModelSource, LanguageModelStreamFinish,
    LanguageModelStreamPart, LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
    LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSupportedUrls,
    LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd, LanguageModelTextStart,
    LanguageModelTool, LanguageModelToolApprovalRequest, LanguageModelToolCall,
    LanguageModelToolCallPart, LanguageModelToolChoice, LanguageModelToolContentPart,
    LanguageModelToolInputDelta, LanguageModelToolInputEnd, LanguageModelToolInputStart,
    LanguageModelToolResult, LanguageModelToolResultContentPart, LanguageModelToolResultOutput,
    LanguageModelUrlSource, LanguageModelUsage, LanguageModelUserContentPart, OutputTokenUsage,
};
use crate::openai_compatible::{OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel};
use crate::provider::{
    ApiCallError, ModelType, NoSuchModelError, Provider, ProviderMetadata, ProviderOptions,
    SpecificationVersion,
};
use crate::provider_utils::{
    FetchErrorInfo, HandledFetchError, ParseJsonResult, PostJsonToApiOptions, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, ReasoningLevel, RuntimeEnvironment, ToolNameMapping,
    combine_headers, convert_to_base64, create_event_source_response_handler,
    create_json_error_response_handler, create_json_response_handler, create_tool_name_mapping,
    generate_id, get_top_level_media_type, map_reasoning_to_provider_effort, post_json_to_api,
    resolve_full_media_type, with_user_agent_suffix,
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
                &options,
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
                &options,
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
        options: &LanguageModelCallOptions,
        warnings: Vec<Warning>,
    ) -> LanguageModelGenerateResult {
        let (content, has_tool_calls) = open_responses_content(
            &response,
            &options.prompt,
            &options.tools,
            &self.config.provider_options_name,
        );
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
    let provider_options = options.provider_options.as_ref();
    let provider_tool_names = open_responses_provider_tool_names();
    let tool_name_mapping =
        create_tool_name_mapping(options.tools.iter().flatten(), &provider_tool_names);
    let has_local_shell_tool =
        open_responses_has_provider_tool(&options.tools, "openai.local_shell");
    let has_shell_tool = open_responses_has_provider_tool(&options.tools, "openai.shell");
    let has_apply_patch_tool =
        open_responses_has_provider_tool(&options.tools, "openai.apply_patch");
    let custom_provider_tool_names = open_responses_custom_provider_tool_names(&options.tools);
    let store = open_responses_store_enabled(provider_options_name, provider_options);
    let has_conversation =
        open_responses_conversation_enabled(provider_options_name, provider_options);
    if has_conversation
        && open_responses_previous_response_id_enabled(provider_options_name, provider_options)
    {
        warnings.push(Warning::Unsupported {
            feature: "conversation".to_string(),
            details: Some(
                "conversation and previousResponseId cannot be used together".to_string(),
            ),
        });
    }
    let prompt_input_options = OpenResponsesPromptInputOptions {
        store,
        has_conversation,
        provider_options_name,
        tool_name_mapping: &tool_name_mapping,
        has_local_shell_tool,
        has_shell_tool,
        has_apply_patch_tool,
        custom_provider_tool_names: &custom_provider_tool_names,
    };
    let (input, instructions) =
        open_responses_input(&options.prompt, &prompt_input_options, &mut warnings)?;
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
    apply_open_responses_reasoning_options(options.reasoning.as_ref(), &mut body, &mut warnings);

    Ok((JsonValue::Object(body), warnings))
}

fn apply_open_responses_reasoning_options(
    reasoning: Option<&LanguageModelReasoningEffort>,
    body: &mut JsonObject,
    warnings: &mut Vec<Warning>,
) {
    let provider_effort = remove_open_responses_reasoning_effort(body);
    let effort = provider_effort.or_else(|| open_responses_reasoning_effort(reasoning, warnings));
    let summary = remove_open_responses_reasoning_summary(body);

    if effort.is_none() && summary.is_none() {
        return;
    }

    let mut reasoning_options = match body.remove("reasoning") {
        Some(JsonValue::Object(options)) => options,
        Some(value) => {
            body.insert("reasoning".to_string(), value);
            JsonObject::new()
        }
        None => JsonObject::new(),
    };

    if let Some(effort) = effort {
        reasoning_options.insert("effort".to_string(), JsonValue::String(effort));
    }

    if let Some(summary) = summary {
        reasoning_options.insert("summary".to_string(), JsonValue::String(summary));
    }

    body.insert(
        "reasoning".to_string(),
        JsonValue::Object(reasoning_options),
    );
}

fn open_responses_reasoning_effort(
    reasoning: Option<&LanguageModelReasoningEffort>,
    warnings: &mut Vec<Warning>,
) -> Option<String> {
    match reasoning? {
        LanguageModelReasoningEffort::ProviderDefault => None,
        LanguageModelReasoningEffort::None => Some("none".to_string()),
        effort => {
            let reasoning_level = ReasoningLevel::try_from(effort.clone()).ok()?;
            map_reasoning_to_provider_effort(
                reasoning_level,
                &BTreeMap::from([
                    (ReasoningLevel::Minimal, "low".to_string()),
                    (ReasoningLevel::Low, "low".to_string()),
                    (ReasoningLevel::Medium, "medium".to_string()),
                    (ReasoningLevel::High, "high".to_string()),
                    (ReasoningLevel::Xhigh, "xhigh".to_string()),
                ]),
                warnings,
            )
        }
    }
}

fn remove_open_responses_reasoning_summary(body: &mut JsonObject) -> Option<String> {
    let mut summary = None;

    for key in ["reasoningSummary", "reasoning_summary"] {
        if let Some(value) = body.remove(key)
            && summary.is_none()
            && let Some(value) = value.as_str()
            && matches!(value, "concise" | "detailed" | "auto")
        {
            summary = Some(value.to_string());
        }
    }

    summary
}

fn remove_open_responses_reasoning_effort(body: &mut JsonObject) -> Option<String> {
    for key in ["reasoningEffort", "reasoning_effort"] {
        if let Some(value) = body.remove(key)
            && let Some(value) = value.as_str()
        {
            return Some(value.to_string());
        }
    }

    None
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
    let passthrough_options =
        open_responses_provider_option_passthrough_enabled(raw_provider_options_name);

    if let Some(options) = provider_options.get(raw_provider_options_name) {
        if camel_provider_options_name != raw_provider_options_name {
            warnings.push(Warning::Deprecated {
                setting: format!("providerOptions key '{raw_provider_options_name}'"),
                message: format!("Use '{camel_provider_options_name}' instead."),
            });
        }

        merge_open_responses_provider_option_object(options, passthrough_options, body);
    }

    if camel_provider_options_name != raw_provider_options_name
        && let Some(options) = provider_options.get(&camel_provider_options_name)
    {
        merge_open_responses_provider_option_object(options, passthrough_options, body);
    }

    merge_vercel_ai_gateway_open_responses_provider_options(
        raw_provider_options_name,
        provider_options,
        body,
    );
}

fn open_responses_provider_option_passthrough_enabled(provider_options_name: &str) -> bool {
    matches!(
        provider_options_name,
        "openai" | "azure" | "vercel-ai-gateway"
    )
}

fn merge_open_responses_provider_option_object(
    options: &JsonObject,
    passthrough_options: bool,
    body: &mut JsonObject,
) {
    if passthrough_options {
        merge_open_responses_passthrough_provider_option_object(options, body);
        return;
    }

    for key in ["reasoningSummary", "reasoning_summary"] {
        if let Some(value) = options.get(key) {
            body.insert(key.to_string(), value.clone());
            return;
        }
    }
}

fn merge_open_responses_passthrough_provider_option_object(
    options: &JsonObject,
    body: &mut JsonObject,
) {
    for (key, value) in options {
        match key.as_str() {
            "allowedTools"
            | "allowed_tools"
            | "forceReasoning"
            | "force_reasoning"
            | "passThroughUnsupportedFiles"
            | "pass_through_unsupported_files"
            | "systemMessageMode"
            | "system_message_mode" => {}
            "contextManagement" | "context_management" => {
                body.insert(
                    "context_management".to_string(),
                    open_responses_context_management_provider_option(value),
                );
            }
            "conversation" | "include" | "instructions" | "metadata" | "store" | "truncation"
            | "user" => {
                body.insert(key.clone(), value.clone());
            }
            "logprobs" => {
                if let Some(top_logprobs) = open_responses_logprobs_provider_option(value) {
                    body.insert("top_logprobs".to_string(), top_logprobs);
                    open_responses_add_include(body, "message.output_text.logprobs");
                }
            }
            "maxToolCalls" | "max_tool_calls" => {
                body.insert("max_tool_calls".to_string(), value.clone());
            }
            "parallelToolCalls" | "parallel_tool_calls" => {
                body.insert("parallel_tool_calls".to_string(), value.clone());
            }
            "previousResponseId" | "previous_response_id" => {
                body.insert("previous_response_id".to_string(), value.clone());
            }
            "promptCacheKey" | "prompt_cache_key" => {
                body.insert("prompt_cache_key".to_string(), value.clone());
            }
            "promptCacheRetention" | "prompt_cache_retention" => {
                body.insert("prompt_cache_retention".to_string(), value.clone());
            }
            "reasoningEffort" | "reasoning_effort" => {
                body.insert("reasoningEffort".to_string(), value.clone());
            }
            "reasoningSummary" | "reasoning_summary" => {
                body.insert("reasoningSummary".to_string(), value.clone());
            }
            "safetyIdentifier" | "safety_identifier" => {
                body.insert("safety_identifier".to_string(), value.clone());
            }
            "serviceTier" | "service_tier" => {
                body.insert("service_tier".to_string(), value.clone());
            }
            "strictJsonSchema" | "strict_json_schema" => {
                open_responses_apply_strict_json_schema_provider_option(body, value);
            }
            "textVerbosity" | "text_verbosity" => {
                open_responses_insert_text_provider_option(body, "verbosity", value.clone());
            }
            "topLogprobs" | "top_logprobs" => {
                body.insert("top_logprobs".to_string(), value.clone());
            }
            _ => {
                body.insert(key.clone(), value.clone());
            }
        }
    }
}

fn open_responses_logprobs_provider_option(value: &JsonValue) -> Option<JsonValue> {
    match value {
        JsonValue::Bool(true) => Some(json!(20)),
        JsonValue::Number(_) => Some(value.clone()),
        _ => None,
    }
}

fn open_responses_add_include(body: &mut JsonObject, include: &str) {
    match body.get_mut("include") {
        Some(JsonValue::Array(values)) => {
            if !values.iter().any(|value| value.as_str() == Some(include)) {
                values.push(JsonValue::String(include.to_string()));
            }
        }
        Some(_) => {}
        None => {
            body.insert(
                "include".to_string(),
                JsonValue::Array(vec![JsonValue::String(include.to_string())]),
            );
        }
    }
}

fn open_responses_context_management_provider_option(value: &JsonValue) -> JsonValue {
    let JsonValue::Array(items) = value else {
        return value.clone();
    };

    JsonValue::Array(
        items
            .iter()
            .map(|item| {
                let JsonValue::Object(object) = item else {
                    return item.clone();
                };
                let mut object = object.clone();
                if let Some(compact_threshold) = object.remove("compactThreshold") {
                    object.insert("compact_threshold".to_string(), compact_threshold);
                }
                JsonValue::Object(object)
            })
            .collect(),
    )
}

fn open_responses_insert_text_provider_option(body: &mut JsonObject, key: &str, value: JsonValue) {
    let text = body
        .entry("text".to_string())
        .or_insert_with(|| JsonValue::Object(JsonObject::new()));
    if let JsonValue::Object(text) = text {
        text.insert(key.to_string(), value);
    }
}

fn open_responses_apply_strict_json_schema_provider_option(
    body: &mut JsonObject,
    value: &JsonValue,
) {
    let Some(strict) = value.as_bool() else {
        return;
    };

    let Some(JsonValue::Object(text)) = body.get_mut("text") else {
        return;
    };
    let Some(JsonValue::Object(format)) = text.get_mut("format") else {
        return;
    };
    if format.get("type").and_then(JsonValue::as_str) == Some("json_schema") {
        format.insert("strict".to_string(), JsonValue::Bool(strict));
    }
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

struct OpenResponsesPromptInputOptions<'a> {
    store: bool,
    has_conversation: bool,
    provider_options_name: &'a str,
    tool_name_mapping: &'a ToolNameMapping,
    has_local_shell_tool: bool,
    has_shell_tool: bool,
    has_apply_patch_tool: bool,
    custom_provider_tool_names: &'a BTreeSet<String>,
}

fn open_responses_input(
    prompt: &[LanguageModelMessage],
    options: &OpenResponsesPromptInputOptions<'_>,
    warnings: &mut Vec<Warning>,
) -> Result<(Vec<JsonValue>, Option<String>), String> {
    let store = options.store;
    let has_conversation = options.has_conversation;
    let provider_options_name = options.provider_options_name;
    let tool_name_mapping = options.tool_name_mapping;
    let has_local_shell_tool = options.has_local_shell_tool;
    let has_shell_tool = options.has_shell_tool;
    let has_apply_patch_tool = options.has_apply_patch_tool;
    let custom_provider_tool_names = options.custom_provider_tool_names;
    let mut input = Vec::new();
    let mut system_messages = Vec::new();
    let mut processed_approval_ids = BTreeSet::new();
    let mut referenced_reasoning_item_ids = BTreeSet::new();
    let mut reasoning_item_positions = BTreeMap::<String, usize>::new();

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
                            content.push(open_responses_file_part(file, provider_options_name)?);
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
                let mut assistant_items = Vec::new();

                for part in &message.content {
                    match part {
                        LanguageModelAssistantContentPart::Text(text) => {
                            let item_id = open_responses_prompt_item_id(
                                text.provider_options.as_ref(),
                                provider_options_name,
                            );
                            let phase = open_responses_prompt_phase(
                                text.provider_options.as_ref(),
                                provider_options_name,
                            );
                            if has_conversation && item_id.is_some() {
                                open_responses_flush_assistant_content(
                                    &mut assistant_items,
                                    &mut content,
                                );
                                continue;
                            }

                            if store && let Some(item_id) = item_id {
                                open_responses_flush_assistant_content(
                                    &mut assistant_items,
                                    &mut content,
                                );
                                assistant_items.push(json!({
                                    "type": "item_reference",
                                    "id": item_id
                                }));
                                continue;
                            }

                            if item_id.is_some() || phase.is_some() {
                                open_responses_flush_assistant_content(
                                    &mut assistant_items,
                                    &mut content,
                                );
                                assistant_items.push(open_responses_assistant_text_message(
                                    &text.text, item_id, phase,
                                ));
                                continue;
                            }

                            content.push(json!({
                                "type": "output_text",
                                "text": text.text
                            }));
                        }
                        LanguageModelAssistantContentPart::Reasoning(reasoning) => {
                            let item_id = open_responses_prompt_item_id(
                                reasoning.provider_options.as_ref(),
                                provider_options_name,
                            );
                            let encrypted_content = open_responses_reasoning_encrypted_content(
                                reasoning.provider_options.as_ref(),
                                provider_options_name,
                            );
                            if has_conversation && item_id.is_some() {
                                open_responses_flush_assistant_content(
                                    &mut assistant_items,
                                    &mut content,
                                );
                                continue;
                            }

                            if store && let Some(item_id) = item_id {
                                open_responses_flush_assistant_content(
                                    &mut assistant_items,
                                    &mut content,
                                );
                                if referenced_reasoning_item_ids.insert(item_id.to_string()) {
                                    assistant_items.push(json!({
                                        "type": "item_reference",
                                        "id": item_id
                                    }));
                                }
                            } else if let Some(item_id) = item_id {
                                open_responses_flush_assistant_items(
                                    &mut input,
                                    &mut assistant_items,
                                    &mut content,
                                );
                                open_responses_push_reasoning_item(
                                    &mut input,
                                    &mut reasoning_item_positions,
                                    Some(item_id),
                                    encrypted_content,
                                    reasoning,
                                    warnings,
                                );
                            } else if let Some(encrypted_content) = encrypted_content {
                                open_responses_flush_assistant_items(
                                    &mut input,
                                    &mut assistant_items,
                                    &mut content,
                                );
                                input.push(open_responses_reasoning_item(
                                    None,
                                    Some(encrypted_content),
                                    &reasoning.text,
                                ));
                            } else {
                                warnings.push(Warning::Other {
                                    message: format!(
                                        "Non-OpenAI reasoning parts are not supported. Skipping reasoning part: {}.",
                                        open_responses_reasoning_warning_part(reasoning)
                                    ),
                                });
                            }
                        }
                        LanguageModelAssistantContentPart::Custom(custom) => {
                            if custom.kind != "openai.compaction" {
                                continue;
                            }

                            let item_id = open_responses_prompt_item_id(
                                custom.provider_options.as_ref(),
                                provider_options_name,
                            );
                            if has_conversation && item_id.is_some() {
                                open_responses_flush_assistant_content(
                                    &mut assistant_items,
                                    &mut content,
                                );
                                continue;
                            }

                            if store && let Some(item_id) = item_id {
                                open_responses_flush_assistant_content(
                                    &mut assistant_items,
                                    &mut content,
                                );
                                assistant_items.push(json!({
                                    "type": "item_reference",
                                    "id": item_id
                                }));
                                continue;
                            }

                            if let Some(item_id) = item_id {
                                open_responses_flush_assistant_items(
                                    &mut input,
                                    &mut assistant_items,
                                    &mut content,
                                );
                                input.push(open_responses_compaction_item(
                                    item_id,
                                    open_responses_compaction_encrypted_content(
                                        custom.provider_options.as_ref(),
                                        provider_options_name,
                                    ),
                                ));
                            }
                        }
                        LanguageModelAssistantContentPart::ToolCall(tool_call) => {
                            open_responses_flush_assistant_content(
                                &mut assistant_items,
                                &mut content,
                            );
                            let item_id = open_responses_prompt_item_id(
                                tool_call.provider_options.as_ref(),
                                provider_options_name,
                            );
                            if has_conversation && item_id.is_some() {
                                continue;
                            }

                            let resolved_tool_name =
                                tool_name_mapping.to_provider_tool_name(&tool_call.tool_name);
                            if resolved_tool_name == "tool_search" {
                                if store && let Some(item_id) = item_id {
                                    assistant_items.push(json!({
                                        "type": "item_reference",
                                        "id": item_id
                                    }));
                                } else {
                                    assistant_items.push(open_responses_tool_search_call_item(
                                        tool_call, item_id,
                                    ));
                                }
                                continue;
                            }

                            if tool_call.provider_executed == Some(true) {
                                if store && let Some(item_id) = item_id {
                                    assistant_items.push(json!({
                                        "type": "item_reference",
                                        "id": item_id
                                    }));
                                }
                                continue;
                            }

                            if store && let Some(item_id) = item_id {
                                assistant_items.push(json!({
                                    "type": "item_reference",
                                    "id": item_id
                                }));
                                continue;
                            }

                            if has_local_shell_tool && resolved_tool_name == "local_shell" {
                                assistant_items
                                    .push(open_responses_local_shell_call_item(tool_call, item_id));
                                continue;
                            }

                            if has_shell_tool && resolved_tool_name == "shell" {
                                assistant_items
                                    .push(open_responses_shell_call_item(tool_call, item_id));
                                continue;
                            }

                            if has_apply_patch_tool && resolved_tool_name == "apply_patch" {
                                assistant_items
                                    .push(open_responses_apply_patch_call_item(tool_call, item_id));
                                continue;
                            }

                            if custom_provider_tool_names.contains(&resolved_tool_name) {
                                assistant_items.push(open_responses_custom_tool_call_item(
                                    tool_call,
                                    &resolved_tool_name,
                                    item_id,
                                ));
                                continue;
                            }

                            assistant_items.push(json!({
                                "type": "function_call",
                                "call_id": tool_call.tool_call_id,
                                "name": tool_call.tool_name,
                                "arguments": open_responses_function_call_arguments(
                                    &tool_call.input
                                )
                            }));
                        }
                        LanguageModelAssistantContentPart::ToolResult(part) => {
                            open_responses_flush_assistant_content(
                                &mut assistant_items,
                                &mut content,
                            );

                            if open_responses_is_execution_denied_output(&part.output) {
                                continue;
                            }

                            if has_conversation {
                                continue;
                            }

                            let resolved_tool_name =
                                tool_name_mapping.to_provider_tool_name(&part.tool_name);
                            if resolved_tool_name == "tool_search" {
                                let item_id = open_responses_prompt_item_id(
                                    part.provider_options.as_ref(),
                                    provider_options_name,
                                )
                                .unwrap_or(part.tool_call_id.as_str());

                                if store {
                                    assistant_items.push(json!({
                                        "type": "item_reference",
                                        "id": item_id
                                    }));
                                } else if let Some(output) =
                                    open_responses_tool_result_json(&part.output)
                                {
                                    assistant_items.push(open_responses_tool_search_output_item(
                                        Some(item_id),
                                        JsonValue::Null,
                                        "server",
                                        output,
                                    ));
                                }
                                continue;
                            }

                            if has_shell_tool && resolved_tool_name == "shell" {
                                if let Some(output) = open_responses_tool_result_json(&part.output)
                                {
                                    assistant_items.push(open_responses_shell_call_output_item(
                                        &part.tool_call_id,
                                        output,
                                    ));
                                }
                                continue;
                            }

                            if store {
                                let item_id = open_responses_prompt_item_id(
                                    part.provider_options.as_ref(),
                                    provider_options_name,
                                )
                                .unwrap_or(part.tool_call_id.as_str());
                                assistant_items.push(json!({
                                    "type": "item_reference",
                                    "id": item_id
                                }));
                            } else {
                                warnings.push(Warning::Other {
                                    message: format!(
                                        "Results for OpenAI tool {} are not sent to the API when store is false",
                                        part.tool_name
                                    ),
                                });
                            }
                        }
                        LanguageModelAssistantContentPart::ToolApprovalRequest(_) => {}
                        _ => {
                            return Err(
                                "Open Responses assistant prompt part is not implemented yet."
                                    .to_string(),
                            );
                        }
                    }
                }

                open_responses_flush_assistant_content(&mut assistant_items, &mut content);
                input.extend(assistant_items);
            }
            LanguageModelMessage::Tool(message) => {
                for part in &message.content {
                    match part {
                        LanguageModelToolContentPart::ToolResult(part) => {
                            if open_responses_execution_denied_approval_id(&part.output).is_some() {
                                continue;
                            }

                            let resolved_tool_name =
                                tool_name_mapping.to_provider_tool_name(&part.tool_name);
                            if resolved_tool_name == "tool_search"
                                && let Some(output) = open_responses_tool_result_json(&part.output)
                            {
                                input.push(open_responses_tool_search_output_item(
                                    None,
                                    JsonValue::String(part.tool_call_id.clone()),
                                    "client",
                                    output,
                                ));
                                continue;
                            }

                            if has_local_shell_tool
                                && resolved_tool_name == "local_shell"
                                && let Some(output) = open_responses_tool_result_json(&part.output)
                            {
                                input.push(open_responses_local_shell_call_output_item(
                                    &part.tool_call_id,
                                    output,
                                ));
                                continue;
                            }

                            if has_shell_tool
                                && resolved_tool_name == "shell"
                                && let Some(output) = open_responses_tool_result_json(&part.output)
                            {
                                input.push(open_responses_shell_call_output_item(
                                    &part.tool_call_id,
                                    output,
                                ));
                                continue;
                            }

                            if has_apply_patch_tool
                                && resolved_tool_name == "apply_patch"
                                && let Some(output) = open_responses_tool_result_json(&part.output)
                            {
                                input.push(open_responses_apply_patch_call_output_item(
                                    &part.tool_call_id,
                                    output,
                                ));
                                continue;
                            }

                            if custom_provider_tool_names.contains(&resolved_tool_name) {
                                input.push(open_responses_custom_tool_call_output_item(
                                    &part.tool_call_id,
                                    &part.output,
                                    provider_options_name,
                                    warnings,
                                ));
                                continue;
                            }

                            input.push(json!({
                                "type": "function_call_output",
                                "call_id": part.tool_call_id,
                                "output": open_responses_tool_result_output(
                                    &part.output,
                                    provider_options_name,
                                    warnings
                                )
                            }));
                        }
                        LanguageModelToolContentPart::ToolApprovalResponse(part) => {
                            if !processed_approval_ids.insert(part.approval_id.clone()) {
                                continue;
                            }

                            if store {
                                input.push(json!({
                                    "type": "item_reference",
                                    "id": part.approval_id
                                }));
                            }

                            input.push(json!({
                                "type": "mcp_approval_response",
                                "approval_request_id": part.approval_id,
                                "approve": part.approved
                            }));
                        }
                    }
                }
            }
        }
    }

    if !store
        && input
            .iter()
            .any(open_responses_reasoning_missing_encrypted_content)
    {
        warnings.push(Warning::Other {
            message: "Reasoning parts without encrypted content are not supported when store is false. Skipping reasoning parts.".to_string(),
        });
        input.retain(|item| !open_responses_reasoning_missing_encrypted_content(item));
    }

    let instructions = (!system_messages.is_empty()).then(|| system_messages.join("\n"));

    Ok((input, instructions))
}

fn open_responses_flush_assistant_content(
    input: &mut Vec<JsonValue>,
    content: &mut Vec<JsonValue>,
) {
    if content.is_empty() {
        return;
    }

    input.push(json!({
        "type": "message",
        "role": "assistant",
        "content": std::mem::take(content)
    }));
}

fn open_responses_flush_assistant_items(
    input: &mut Vec<JsonValue>,
    assistant_items: &mut Vec<JsonValue>,
    content: &mut Vec<JsonValue>,
) {
    open_responses_flush_assistant_content(assistant_items, content);
    input.append(assistant_items);
}

fn open_responses_assistant_text_message(
    text: &str,
    item_id: Option<&str>,
    phase: Option<&str>,
) -> JsonValue {
    let mut message = JsonObject::new();
    message.insert("type".to_string(), JsonValue::String("message".to_string()));
    message.insert(
        "role".to_string(),
        JsonValue::String("assistant".to_string()),
    );
    message.insert(
        "content".to_string(),
        JsonValue::Array(vec![json!({
            "type": "output_text",
            "text": text
        })]),
    );
    if let Some(item_id) = item_id {
        message.insert("id".to_string(), JsonValue::String(item_id.to_string()));
    }
    if let Some(phase) = phase {
        message.insert("phase".to_string(), JsonValue::String(phase.to_string()));
    }
    JsonValue::Object(message)
}

fn open_responses_push_reasoning_item(
    input: &mut Vec<JsonValue>,
    reasoning_item_positions: &mut BTreeMap<String, usize>,
    item_id: Option<&str>,
    encrypted_content: Option<&str>,
    reasoning: &crate::language_model::LanguageModelReasoningPart,
    warnings: &mut Vec<Warning>,
) {
    let Some(item_id) = item_id else {
        input.push(open_responses_reasoning_item(
            None,
            encrypted_content,
            &reasoning.text,
        ));
        return;
    };

    if let Some(position) = reasoning_item_positions.get(item_id).copied() {
        if reasoning.text.is_empty() {
            warnings.push(Warning::Other {
                message: format!(
                    "Cannot append empty reasoning part to existing reasoning sequence. Skipping reasoning part: {}.",
                    open_responses_reasoning_warning_part(reasoning)
                ),
            });
        } else if let Some(summary) = input
            .get_mut(position)
            .and_then(JsonValue::as_object_mut)
            .and_then(|item| item.get_mut("summary"))
            .and_then(JsonValue::as_array_mut)
        {
            summary.push(json!({
                "type": "summary_text",
                "text": reasoning.text
            }));
        }

        if let Some(encrypted_content) = encrypted_content
            && let Some(item) = input.get_mut(position).and_then(JsonValue::as_object_mut)
        {
            item.insert(
                "encrypted_content".to_string(),
                JsonValue::String(encrypted_content.to_string()),
            );
        }
        return;
    }

    reasoning_item_positions.insert(item_id.to_string(), input.len());
    input.push(open_responses_reasoning_item(
        Some(item_id),
        encrypted_content,
        &reasoning.text,
    ));
}

fn open_responses_reasoning_item(
    item_id: Option<&str>,
    encrypted_content: Option<&str>,
    text: &str,
) -> JsonValue {
    let mut item = JsonObject::new();
    item.insert(
        "type".to_string(),
        JsonValue::String("reasoning".to_string()),
    );
    if let Some(item_id) = item_id {
        item.insert("id".to_string(), JsonValue::String(item_id.to_string()));
    }
    if let Some(encrypted_content) = encrypted_content {
        item.insert(
            "encrypted_content".to_string(),
            JsonValue::String(encrypted_content.to_string()),
        );
    }

    let summary = if text.is_empty() {
        Vec::new()
    } else {
        vec![json!({
            "type": "summary_text",
            "text": text
        })]
    };
    item.insert("summary".to_string(), JsonValue::Array(summary));
    JsonValue::Object(item)
}

fn open_responses_reasoning_missing_encrypted_content(item: &JsonValue) -> bool {
    item.get("type").and_then(JsonValue::as_str) == Some("reasoning")
        && item.get("encrypted_content").is_none_or(JsonValue::is_null)
}

fn open_responses_reasoning_warning_part(
    reasoning: &crate::language_model::LanguageModelReasoningPart,
) -> String {
    serde_json::to_string(reasoning).unwrap_or_else(|_| "{\"type\":\"reasoning\"}".to_string())
}

fn open_responses_compaction_item(item_id: &str, encrypted_content: Option<&str>) -> JsonValue {
    let mut item = JsonObject::new();
    item.insert(
        "type".to_string(),
        JsonValue::String("compaction".to_string()),
    );
    item.insert("id".to_string(), JsonValue::String(item_id.to_string()));
    if let Some(encrypted_content) = encrypted_content {
        item.insert(
            "encrypted_content".to_string(),
            JsonValue::String(encrypted_content.to_string()),
        );
    }
    JsonValue::Object(item)
}

fn open_responses_function_call_arguments(input: &JsonValue) -> String {
    if input.is_null() {
        return "{}".to_string();
    }

    input
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| input.to_string())
}

fn open_responses_tool_search_call_item(
    tool_call: &LanguageModelToolCallPart,
    item_id: Option<&str>,
) -> JsonValue {
    let parsed_input = open_responses_tool_search_prompt_input(&tool_call.input);
    let call_id = parsed_input
        .get("call_id")
        .cloned()
        .unwrap_or(JsonValue::Null);
    let execution = if call_id.is_null() {
        "server"
    } else {
        "client"
    };

    json!({
        "type": "tool_search_call",
        "id": item_id.unwrap_or(tool_call.tool_call_id.as_str()),
        "execution": execution,
        "call_id": call_id,
        "status": "completed",
        "arguments": parsed_input
            .get("arguments")
            .cloned()
            .unwrap_or(JsonValue::Null)
    })
}

fn open_responses_tool_search_prompt_input(input: &JsonValue) -> JsonObject {
    let parsed_input = input
        .as_str()
        .and_then(|input| serde_json::from_str::<JsonValue>(input).ok())
        .unwrap_or_else(|| input.clone());

    parsed_input.as_object().cloned().unwrap_or_default()
}

fn open_responses_tool_result_json(output: &LanguageModelToolResultOutput) -> Option<&JsonValue> {
    match output {
        LanguageModelToolResultOutput::Json { value, .. }
        | LanguageModelToolResultOutput::ErrorJson { value, .. } => Some(value),
        _ => None,
    }
}

fn open_responses_tool_search_output_item(
    id: Option<&str>,
    call_id: JsonValue,
    execution: &str,
    output: &JsonValue,
) -> JsonValue {
    let mut item = JsonObject::new();
    item.insert(
        "type".to_string(),
        JsonValue::String("tool_search_output".to_string()),
    );
    if let Some(id) = id {
        item.insert("id".to_string(), JsonValue::String(id.to_string()));
    }
    item.insert(
        "execution".to_string(),
        JsonValue::String(execution.to_string()),
    );
    item.insert("call_id".to_string(), call_id);
    item.insert(
        "status".to_string(),
        JsonValue::String("completed".to_string()),
    );
    item.insert(
        "tools".to_string(),
        output
            .get("tools")
            .cloned()
            .unwrap_or_else(|| JsonValue::Array(Vec::new())),
    );
    JsonValue::Object(item)
}

fn open_responses_local_shell_call_item(
    tool_call: &LanguageModelToolCallPart,
    item_id: Option<&str>,
) -> JsonValue {
    let parsed_input = open_responses_prompt_json_object_input(&tool_call.input);
    let action = parsed_input
        .get("action")
        .and_then(JsonValue::as_object)
        .cloned()
        .unwrap_or_default();
    let mut request_action = JsonObject::new();
    request_action.insert(
        "type".to_string(),
        action
            .get("type")
            .cloned()
            .unwrap_or_else(|| JsonValue::String("exec".to_string())),
    );
    request_action.insert(
        "command".to_string(),
        action
            .get("command")
            .cloned()
            .unwrap_or_else(|| JsonValue::Array(Vec::new())),
    );
    if let Some(timeout_ms) = action.get("timeoutMs").filter(|value| !value.is_null()) {
        request_action.insert("timeout_ms".to_string(), timeout_ms.clone());
    }
    if let Some(user) = action.get("user").filter(|value| !value.is_null()) {
        request_action.insert("user".to_string(), user.clone());
    }
    if let Some(working_directory) = action
        .get("workingDirectory")
        .filter(|value| !value.is_null())
    {
        request_action.insert("working_directory".to_string(), working_directory.clone());
    }
    if let Some(environment) = action.get("env").filter(|value| !value.is_null()) {
        request_action.insert("env".to_string(), environment.clone());
    }

    json!({
        "type": "local_shell_call",
        "call_id": tool_call.tool_call_id,
        "id": item_id.unwrap_or(tool_call.tool_call_id.as_str()),
        "action": request_action
    })
}

fn open_responses_local_shell_call_output_item(call_id: &str, output: &JsonValue) -> JsonValue {
    json!({
        "type": "local_shell_call_output",
        "call_id": call_id,
        "output": output.get("output").cloned().unwrap_or(JsonValue::Null)
    })
}

fn open_responses_shell_call_item(
    tool_call: &LanguageModelToolCallPart,
    item_id: Option<&str>,
) -> JsonValue {
    let parsed_input = open_responses_prompt_json_object_input(&tool_call.input);
    let action = parsed_input
        .get("action")
        .and_then(JsonValue::as_object)
        .cloned()
        .unwrap_or_default();
    let mut request_action = JsonObject::new();
    request_action.insert(
        "commands".to_string(),
        action
            .get("commands")
            .cloned()
            .unwrap_or_else(|| JsonValue::Array(Vec::new())),
    );
    if let Some(timeout_ms) = action.get("timeoutMs").filter(|value| !value.is_null()) {
        request_action.insert("timeout_ms".to_string(), timeout_ms.clone());
    }
    if let Some(max_output_length) = action
        .get("maxOutputLength")
        .filter(|value| !value.is_null())
    {
        request_action.insert("max_output_length".to_string(), max_output_length.clone());
    }

    json!({
        "type": "shell_call",
        "call_id": tool_call.tool_call_id,
        "id": item_id.unwrap_or(tool_call.tool_call_id.as_str()),
        "status": "completed",
        "action": request_action
    })
}

fn open_responses_shell_call_output_item(call_id: &str, output: &JsonValue) -> JsonValue {
    json!({
        "type": "shell_call_output",
        "call_id": call_id,
        "output": output
            .get("output")
            .and_then(JsonValue::as_array)
            .map(|items| {
                JsonValue::Array(
                    items
                        .iter()
                        .map(open_responses_shell_prompt_output_item)
                        .collect(),
                )
            })
            .unwrap_or_else(|| JsonValue::Array(Vec::new()))
    })
}

fn open_responses_apply_patch_call_item(
    tool_call: &LanguageModelToolCallPart,
    item_id: Option<&str>,
) -> JsonValue {
    let parsed_input = open_responses_prompt_json_object_input(&tool_call.input);

    json!({
        "type": "apply_patch_call",
        "call_id": parsed_input
            .get("callId")
            .cloned()
            .unwrap_or_else(|| JsonValue::String(tool_call.tool_call_id.clone())),
        "id": item_id.unwrap_or(tool_call.tool_call_id.as_str()),
        "status": "completed",
        "operation": parsed_input
            .get("operation")
            .cloned()
            .unwrap_or(JsonValue::Null)
    })
}

fn open_responses_apply_patch_call_output_item(call_id: &str, output: &JsonValue) -> JsonValue {
    json!({
        "type": "apply_patch_call_output",
        "call_id": call_id,
        "status": output
            .get("status")
            .cloned()
            .unwrap_or_else(|| JsonValue::String("completed".to_string())),
        "output": output.get("output").cloned().unwrap_or(JsonValue::Null)
    })
}

fn open_responses_custom_tool_call_item(
    tool_call: &LanguageModelToolCallPart,
    resolved_tool_name: &str,
    item_id: Option<&str>,
) -> JsonValue {
    let input = match &tool_call.input {
        JsonValue::String(input) => input.clone(),
        input => open_responses_stringified_json(input.clone()),
    };
    let mut item = JsonObject::new();
    item.insert(
        "type".to_string(),
        JsonValue::String("custom_tool_call".to_string()),
    );
    item.insert(
        "call_id".to_string(),
        JsonValue::String(tool_call.tool_call_id.clone()),
    );
    item.insert(
        "name".to_string(),
        JsonValue::String(resolved_tool_name.to_string()),
    );
    item.insert("input".to_string(), JsonValue::String(input));
    if let Some(item_id) = item_id {
        item.insert("id".to_string(), JsonValue::String(item_id.to_string()));
    }
    JsonValue::Object(item)
}

fn open_responses_custom_tool_call_output_item(
    call_id: &str,
    output: &LanguageModelToolResultOutput,
    provider_options_name: &str,
    warnings: &mut Vec<Warning>,
) -> JsonValue {
    json!({
        "type": "custom_tool_call_output",
        "call_id": call_id,
        "output": open_responses_tool_result_output(output, provider_options_name, warnings)
    })
}

fn open_responses_shell_prompt_output_item(item: &JsonValue) -> JsonValue {
    let outcome = item
        .get("outcome")
        .and_then(JsonValue::as_object)
        .map(
            |outcome| match outcome.get("type").and_then(JsonValue::as_str) {
                Some("exit") => json!({
                    "type": "exit",
                    "exit_code": outcome
                        .get("exitCode")
                        .or_else(|| outcome.get("exit_code"))
                        .cloned()
                        .unwrap_or(JsonValue::Null)
                }),
                _ => json!({ "type": "timeout" }),
            },
        )
        .unwrap_or_else(|| json!({ "type": "timeout" }));

    json!({
        "stdout": item.get("stdout").cloned().unwrap_or(JsonValue::Null),
        "stderr": item.get("stderr").cloned().unwrap_or(JsonValue::Null),
        "outcome": outcome
    })
}

fn open_responses_prompt_json_object_input(input: &JsonValue) -> JsonObject {
    let parsed_input = input
        .as_str()
        .and_then(|input| serde_json::from_str::<JsonValue>(input).ok())
        .unwrap_or_else(|| input.clone());

    parsed_input.as_object().cloned().unwrap_or_default()
}

fn open_responses_store_enabled(
    provider_options_name: &str,
    provider_options: Option<&ProviderOptions>,
) -> bool {
    let Some(provider_options) = provider_options else {
        return true;
    };

    let raw_provider_options_name = provider_options_name
        .split('.')
        .next()
        .unwrap_or(provider_options_name)
        .trim();
    if !open_responses_provider_option_passthrough_enabled(raw_provider_options_name) {
        return true;
    }

    let camel_provider_options_name =
        open_responses_camel_case_provider_options_key(raw_provider_options_name);
    let mut store = None;

    if let Some(options) = provider_options.get(raw_provider_options_name) {
        store = options.get("store").and_then(JsonValue::as_bool);
    }

    if camel_provider_options_name != raw_provider_options_name
        && let Some(options) = provider_options.get(&camel_provider_options_name)
        && let Some(value) = options.get("store").and_then(JsonValue::as_bool)
    {
        store = Some(value);
    }

    store.unwrap_or(true)
}

fn open_responses_conversation_enabled(
    provider_options_name: &str,
    provider_options: Option<&ProviderOptions>,
) -> bool {
    open_responses_provider_option_value(provider_options_name, provider_options, &["conversation"])
        .is_some()
}

fn open_responses_previous_response_id_enabled(
    provider_options_name: &str,
    provider_options: Option<&ProviderOptions>,
) -> bool {
    open_responses_provider_option_value(
        provider_options_name,
        provider_options,
        &["previousResponseId", "previous_response_id"],
    )
    .is_some()
}

fn open_responses_provider_option_value<'a>(
    provider_options_name: &str,
    provider_options: Option<&'a ProviderOptions>,
    keys: &[&str],
) -> Option<&'a JsonValue> {
    let provider_options = provider_options?;
    let raw_provider_options_name = provider_options_name
        .split('.')
        .next()
        .unwrap_or(provider_options_name)
        .trim();
    if !open_responses_provider_option_passthrough_enabled(raw_provider_options_name) {
        return None;
    }

    if let Some(value) = provider_options
        .get(raw_provider_options_name)
        .and_then(|options| open_responses_option_value(options, keys))
    {
        return Some(value);
    }

    let camel_provider_options_name =
        open_responses_camel_case_provider_options_key(raw_provider_options_name);
    if camel_provider_options_name != raw_provider_options_name {
        return provider_options
            .get(&camel_provider_options_name)
            .and_then(|options| open_responses_option_value(options, keys));
    }

    None
}

fn open_responses_option_value<'a>(
    options: &'a JsonObject,
    keys: &[&str],
) -> Option<&'a JsonValue> {
    keys.iter()
        .filter_map(|key| options.get(*key))
        .find(|value| !value.is_null())
}

fn open_responses_execution_denied_approval_id(
    output: &LanguageModelToolResultOutput,
) -> Option<&str> {
    match output {
        LanguageModelToolResultOutput::ExecutionDenied {
            provider_options: Some(provider_options),
            ..
        } => provider_options
            .get("openai")
            .and_then(|options| options.get("approvalId"))
            .and_then(JsonValue::as_str),
        _ => None,
    }
}

fn open_responses_is_execution_denied_output(output: &LanguageModelToolResultOutput) -> bool {
    match output {
        LanguageModelToolResultOutput::ExecutionDenied { .. } => true,
        LanguageModelToolResultOutput::Json { value, .. } => value
            .get("type")
            .and_then(JsonValue::as_str)
            .is_some_and(|kind| kind == "execution-denied"),
        _ => false,
    }
}

fn open_responses_file_part(
    file: &LanguageModelFilePart,
    provider_options_name: &str,
) -> Result<JsonValue, String> {
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
                Ok(open_responses_image_file_part(
                    url.as_str().to_string(),
                    open_responses_image_detail(file, provider_options_name),
                ))
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
                Ok(open_responses_image_file_part(
                    data_uri,
                    open_responses_image_detail(file, provider_options_name),
                ))
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

fn open_responses_image_file_part(image_url: String, detail: Option<JsonValue>) -> JsonValue {
    let mut part = JsonObject::new();
    part.insert(
        "type".to_string(),
        JsonValue::String("input_image".to_string()),
    );
    part.insert("image_url".to_string(), JsonValue::String(image_url));
    if let Some(detail) = detail {
        part.insert("detail".to_string(), detail);
    }
    JsonValue::Object(part)
}

fn open_responses_image_detail(
    file: &LanguageModelFilePart,
    provider_options_name: &str,
) -> Option<JsonValue> {
    file.provider_options
        .as_ref()
        .and_then(|provider_options| provider_options.get(provider_options_name))
        .and_then(|options| options.get("imageDetail"))
        .filter(|detail| !detail.is_null())
        .cloned()
}

fn open_responses_tool_result_output(
    output: &LanguageModelToolResultOutput,
    provider_options_name: &str,
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
                    LanguageModelToolResultContentPart::File(file) => {
                        match open_responses_file_part(file, provider_options_name) {
                            Ok(file_part) => content.push(file_part),
                            Err(message) => warnings.push(Warning::Unsupported {
                                feature: "toolResultFileContent".to_string(),
                                details: Some(message),
                            }),
                        }
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
    prompt: &[LanguageModelMessage],
    tools: &Option<Vec<LanguageModelTool>>,
    provider_options_name: &str,
) -> (Vec<LanguageModelContent>, bool) {
    let mut content = Vec::new();
    let mut has_tool_calls = false;
    let mut source_index = 0usize;
    let provider_tool_names = open_responses_provider_tool_names();
    let tool_name_mapping = create_tool_name_mapping(tools.iter().flatten(), &provider_tool_names);
    let web_search_tool_name = open_responses_web_search_response_tool_name(tools);
    let shell_provider_executed = open_responses_shell_provider_executed(tools);
    let mut hosted_tool_search_call_ids = VecDeque::<String>::new();
    let approval_request_tool_call_ids =
        open_responses_approval_request_tool_call_ids(prompt, provider_options_name);
    let mut approval_request_stream_tool_call_ids = BTreeMap::<String, String>::new();

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
                        let mut text_part = LanguageModelText::new(text);
                        if let Some(metadata) =
                            open_responses_text_metadata(provider_options_name, part, content_part)
                        {
                            text_part = text_part.with_provider_metadata(metadata);
                        }

                        content.push(LanguageModelContent::Text(text_part));
                        open_responses_push_annotation_sources(
                            &mut content,
                            provider_options_name,
                            content_part,
                            &mut source_index,
                        );
                    }
                }
            }
            Some("reasoning") => {
                let mut reasoning_parts = part
                    .get("content")
                    .or_else(|| part.get("summary"))
                    .and_then(JsonValue::as_array)
                    .into_iter()
                    .flatten()
                    .peekable();

                if reasoning_parts.peek().is_none() {
                    content.push(LanguageModelContent::Reasoning(
                        LanguageModelReasoning::new("").with_provider_metadata(
                            open_responses_reasoning_metadata(provider_options_name, part),
                        ),
                    ));
                } else {
                    for content_part in reasoning_parts {
                        if let Some(text) = content_part.get("text").and_then(JsonValue::as_str) {
                            content.push(LanguageModelContent::Reasoning(
                                LanguageModelReasoning::new(text).with_provider_metadata(
                                    open_responses_reasoning_metadata(provider_options_name, part),
                                ),
                            ));
                        }
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
            Some("custom_tool_call") => {
                has_tool_calls = true;
                let tool_call_id = part
                    .get("call_id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();
                let tool_name = part
                    .get("name")
                    .and_then(JsonValue::as_str)
                    .map(|name| tool_name_mapping.to_custom_tool_name(name))
                    .unwrap_or_default();
                let input = open_responses_stringified_json(
                    part.get("input").cloned().unwrap_or(JsonValue::Null),
                );

                content.push(LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                    tool_call_id,
                    tool_name,
                    input,
                )));
            }
            Some("tool_search_call") => {
                let tool_call_id = open_responses_tool_search_call_id(part);
                let hosted = matches!(
                    part.get("execution").and_then(JsonValue::as_str),
                    Some("server")
                );

                if hosted {
                    hosted_tool_search_call_ids.push_back(tool_call_id.clone());
                }

                let mut tool_call = LanguageModelToolCall::new(
                    tool_call_id,
                    tool_name_mapping.to_custom_tool_name("tool_search"),
                    open_responses_tool_search_input(part),
                );

                if hosted {
                    tool_call = tool_call.with_provider_executed(true);
                }

                content.push(LanguageModelContent::ToolCall(tool_call));
            }
            Some("tool_search_output") => {
                let tool_call_id = part
                    .get("call_id")
                    .and_then(JsonValue::as_str)
                    .map(ToString::to_string)
                    .or_else(|| hosted_tool_search_call_ids.pop_front())
                    .or_else(|| {
                        part.get("id")
                            .and_then(JsonValue::as_str)
                            .map(ToString::to_string)
                    })
                    .unwrap_or_default();
                open_responses_push_tool_result(
                    &mut content,
                    &tool_call_id,
                    &tool_name_mapping.to_custom_tool_name("tool_search"),
                    json!({
                        "tools": part.get("tools").cloned().unwrap_or_else(|| JsonValue::Array(Vec::new()))
                    }),
                );
            }
            Some("local_shell_call") => {
                let tool_call_id = part
                    .get("call_id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();

                content.push(LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                    tool_call_id,
                    tool_name_mapping.to_custom_tool_name("local_shell"),
                    json!({
                        "action": part.get("action").cloned().unwrap_or(JsonValue::Null)
                    })
                    .to_string(),
                )));
            }
            Some("shell_call") => {
                let tool_call_id = part
                    .get("call_id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();
                let mut tool_call = LanguageModelToolCall::new(
                    tool_call_id,
                    tool_name_mapping.to_custom_tool_name("shell"),
                    open_responses_shell_call_input(part),
                );

                if shell_provider_executed {
                    tool_call = tool_call.with_provider_executed(true);
                }

                content.push(LanguageModelContent::ToolCall(tool_call));
            }
            Some("shell_call_output") => {
                let tool_call_id = part
                    .get("call_id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();
                open_responses_push_tool_result(
                    &mut content,
                    tool_call_id,
                    &tool_name_mapping.to_custom_tool_name("shell"),
                    open_responses_shell_call_output(part),
                );
            }
            Some("apply_patch_call") => {
                let tool_call_id = part
                    .get("call_id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();

                content.push(LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                    tool_call_id,
                    tool_name_mapping.to_custom_tool_name("apply_patch"),
                    json!({
                        "callId": part.get("call_id").cloned().unwrap_or(JsonValue::Null),
                        "operation": part.get("operation").cloned().unwrap_or(JsonValue::Null)
                    })
                    .to_string(),
                )));
            }
            Some("mcp_call") => {
                let tool_call_id = open_responses_mcp_tool_call_id(
                    part,
                    &approval_request_tool_call_ids,
                    &approval_request_stream_tool_call_ids,
                );
                let tool_name = part
                    .get("name")
                    .and_then(JsonValue::as_str)
                    .map(|name| format!("mcp.{name}"))
                    .unwrap_or_else(|| "mcp".to_string());
                let input = part
                    .get("arguments")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("{}");

                content.push(LanguageModelContent::ToolCall(
                    LanguageModelToolCall::new(&tool_call_id, tool_name.clone(), input)
                        .with_provider_executed(true)
                        .with_dynamic(true),
                ));
                content.push(LanguageModelContent::ToolResult(
                    open_responses_mcp_tool_result(part, &tool_call_id, &tool_name),
                ));
            }
            Some("mcp_approval_request") => {
                let approval_id = part
                    .get("approval_request_id")
                    .and_then(JsonValue::as_str)
                    .or_else(|| part.get("id").and_then(JsonValue::as_str))
                    .unwrap_or_default();
                let tool_call_id = generate_id();
                approval_request_stream_tool_call_ids
                    .insert(approval_id.to_string(), tool_call_id.clone());
                let tool_name = part
                    .get("name")
                    .and_then(JsonValue::as_str)
                    .map(|name| format!("mcp.{name}"))
                    .unwrap_or_else(|| "mcp".to_string());
                let input = part
                    .get("arguments")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("{}");

                let mut tool_call = LanguageModelToolCall::new(&tool_call_id, tool_name, input)
                    .with_provider_executed(true)
                    .with_dynamic(true);
                if let Some(metadata) =
                    open_responses_mcp_approval_metadata(provider_options_name, part, approval_id)
                {
                    tool_call = tool_call.with_provider_metadata(metadata);
                }
                content.push(LanguageModelContent::ToolCall(tool_call));
                content.push(LanguageModelContent::ToolApprovalRequest(
                    LanguageModelToolApprovalRequest::new(approval_id, &tool_call_id),
                ));
            }
            Some("computer_call") => {
                let tool_call_id = part
                    .get("id")
                    .and_then(JsonValue::as_str)
                    .unwrap_or_default();
                let tool_name = tool_name_mapping.to_custom_tool_name("computer_use");

                content.push(LanguageModelContent::ToolCall(
                    LanguageModelToolCall::new(tool_call_id, tool_name.clone(), "")
                        .with_provider_executed(true),
                ));
                open_responses_push_tool_result(
                    &mut content,
                    tool_call_id,
                    &tool_name,
                    json!({
                        "type": "computer_use_tool_result",
                        "status": part
                            .get("status")
                            .cloned()
                            .unwrap_or_else(|| JsonValue::String("completed".to_string()))
                    }),
                );
            }
            Some("compaction") => {
                content.push(LanguageModelContent::Custom(
                    LanguageModelCustomContent::new("openai.compaction").with_provider_metadata(
                        open_responses_compaction_metadata(provider_options_name, part),
                    ),
                ));
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

fn open_responses_metadata(provider_name: &str, provider: JsonObject) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    metadata.insert(provider_name.to_string(), provider);
    metadata
}

fn open_responses_approval_request_tool_call_ids(
    prompt: &[LanguageModelMessage],
    provider_name: &str,
) -> BTreeMap<String, String> {
    let mut mapping = BTreeMap::new();

    for message in prompt {
        let LanguageModelMessage::Assistant(message) = message else {
            continue;
        };

        for part in &message.content {
            let LanguageModelAssistantContentPart::ToolCall(tool_call) = part else {
                continue;
            };
            let Some(approval_id) = open_responses_prompt_approval_request_id(
                tool_call.provider_options.as_ref(),
                provider_name,
            ) else {
                continue;
            };
            mapping.insert(approval_id.to_string(), tool_call.tool_call_id.clone());
        }
    }

    mapping
}

fn open_responses_prompt_approval_request_id<'a>(
    provider_options: Option<&'a ProviderOptions>,
    provider_name: &str,
) -> Option<&'a str> {
    open_responses_prompt_provider_options(provider_options, provider_name)
        .and_then(|metadata| metadata.get("approvalRequestId"))
        .and_then(JsonValue::as_str)
}

fn open_responses_prompt_item_id<'a>(
    provider_options: Option<&'a ProviderOptions>,
    provider_name: &str,
) -> Option<&'a str> {
    open_responses_prompt_provider_options(provider_options, provider_name)
        .and_then(|metadata| metadata.get("itemId"))
        .and_then(JsonValue::as_str)
}

fn open_responses_prompt_phase<'a>(
    provider_options: Option<&'a ProviderOptions>,
    provider_name: &str,
) -> Option<&'a str> {
    open_responses_prompt_provider_options(provider_options, provider_name)
        .and_then(|metadata| metadata.get("phase"))
        .and_then(JsonValue::as_str)
}

fn open_responses_reasoning_encrypted_content<'a>(
    provider_options: Option<&'a ProviderOptions>,
    provider_name: &str,
) -> Option<&'a str> {
    open_responses_prompt_provider_options(provider_options, provider_name)
        .and_then(|metadata| metadata.get("reasoningEncryptedContent"))
        .and_then(JsonValue::as_str)
}

fn open_responses_compaction_encrypted_content<'a>(
    provider_options: Option<&'a ProviderOptions>,
    provider_name: &str,
) -> Option<&'a str> {
    open_responses_prompt_provider_options(provider_options, provider_name)
        .and_then(|metadata| metadata.get("encryptedContent"))
        .and_then(JsonValue::as_str)
}

fn open_responses_prompt_provider_options<'a>(
    provider_options: Option<&'a ProviderOptions>,
    provider_name: &str,
) -> Option<&'a JsonObject> {
    let provider_options = provider_options?;

    provider_options
        .get(provider_name)
        .or_else(|| {
            let raw_provider_name = provider_name
                .split('.')
                .next()
                .unwrap_or(provider_name)
                .trim();
            provider_options.get(raw_provider_name)
        })
        .or_else(|| provider_options.get("openai"))
}

fn open_responses_mcp_tool_call_id(
    item: &JsonValue,
    prompt_mapping: &BTreeMap<String, String>,
    response_mapping: &BTreeMap<String, String>,
) -> String {
    if let Some(approval_id) = item.get("approval_request_id").and_then(JsonValue::as_str)
        && let Some(tool_call_id) = response_mapping
            .get(approval_id)
            .or_else(|| prompt_mapping.get(approval_id))
    {
        return tool_call_id.clone();
    }

    item.get("id")
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_string()
}

fn open_responses_mcp_approval_metadata(
    provider_name: &str,
    item: &JsonValue,
    approval_id: &str,
) -> Option<ProviderMetadata> {
    let mut metadata = JsonObject::new();

    if let Some(item_id) = item.get("id").filter(|value| !value.is_null()) {
        metadata.insert("itemId".to_string(), item_id.clone());
    }

    if let Some(namespace) = item.get("namespace").filter(|value| !value.is_null()) {
        metadata.insert("namespace".to_string(), namespace.clone());
    }

    if !approval_id.is_empty() {
        metadata.insert(
            "approvalRequestId".to_string(),
            JsonValue::String(approval_id.to_string()),
        );
    }

    (!metadata.is_empty()).then(|| open_responses_metadata(provider_name, metadata))
}

fn open_responses_item_metadata(provider_name: &str, item: &JsonValue) -> Option<ProviderMetadata> {
    let mut metadata = JsonObject::new();

    if let Some(item_id) = item.get("id").filter(|value| !value.is_null()) {
        metadata.insert("itemId".to_string(), item_id.clone());
    }

    if let Some(namespace) = item.get("namespace").filter(|value| !value.is_null()) {
        metadata.insert("namespace".to_string(), namespace.clone());
    }

    (!metadata.is_empty()).then(|| open_responses_metadata(provider_name, metadata))
}

fn open_responses_namespace_metadata(
    provider_name: &str,
    item: &JsonValue,
) -> Option<ProviderMetadata> {
    let mut metadata = JsonObject::new();

    if let Some(namespace) = item.get("namespace").filter(|value| !value.is_null()) {
        metadata.insert("namespace".to_string(), namespace.clone());
    }

    (!metadata.is_empty()).then(|| open_responses_metadata(provider_name, metadata))
}

fn open_responses_text_metadata(
    provider_name: &str,
    item: &JsonValue,
    content_part: &JsonValue,
) -> Option<ProviderMetadata> {
    let mut metadata = JsonObject::new();

    if let Some(item_id) = item.get("id").filter(|value| !value.is_null()) {
        metadata.insert("itemId".to_string(), item_id.clone());
    }

    if let Some(phase) = item.get("phase").filter(|value| !value.is_null()) {
        metadata.insert("phase".to_string(), phase.clone());
    }

    if let Some(annotations) = content_part
        .get("annotations")
        .and_then(JsonValue::as_array)
        .filter(|annotations| !annotations.is_empty())
    {
        metadata.insert(
            "annotations".to_string(),
            JsonValue::Array(annotations.clone()),
        );
    }

    (!metadata.is_empty()).then(|| open_responses_metadata(provider_name, metadata))
}

fn open_responses_reasoning_metadata(provider_name: &str, item: &JsonValue) -> ProviderMetadata {
    let mut metadata = JsonObject::new();
    metadata.insert(
        "itemId".to_string(),
        item.get("id").cloned().unwrap_or(JsonValue::Null),
    );
    metadata.insert(
        "reasoningEncryptedContent".to_string(),
        item.get("encrypted_content")
            .cloned()
            .unwrap_or(JsonValue::Null),
    );
    open_responses_metadata(provider_name, metadata)
}

fn open_responses_compaction_metadata(provider_name: &str, item: &JsonValue) -> ProviderMetadata {
    let mut metadata = JsonObject::new();
    metadata.insert(
        "type".to_string(),
        JsonValue::String("compaction".to_string()),
    );
    metadata.insert(
        "itemId".to_string(),
        item.get("id").cloned().unwrap_or(JsonValue::Null),
    );
    metadata.insert(
        "encryptedContent".to_string(),
        item.get("encrypted_content")
            .cloned()
            .unwrap_or(JsonValue::Null),
    );
    open_responses_metadata(provider_name, metadata)
}

fn open_responses_push_annotation_sources(
    content: &mut Vec<LanguageModelContent>,
    provider_name: &str,
    content_part: &JsonValue,
    source_index: &mut usize,
) {
    for annotation in content_part
        .get("annotations")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(source) = open_responses_annotation_source(
            provider_name,
            annotation,
            open_responses_next_source_id(source_index),
        ) {
            content.push(LanguageModelContent::Source(source));
        }
    }
}

fn open_responses_annotation_source(
    provider_name: &str,
    annotation: &JsonValue,
    source_id: String,
) -> Option<LanguageModelSource> {
    match annotation.get("type").and_then(JsonValue::as_str) {
        Some("url_citation") => {
            let url = annotation.get("url").and_then(JsonValue::as_str)?;
            let mut source = LanguageModelUrlSource::new(source_id, url);
            if let Some(title) = annotation.get("title").and_then(JsonValue::as_str) {
                source = source.with_title(title);
            }
            Some(LanguageModelSource::Url(source))
        }
        Some("file_citation") => {
            let filename = annotation
                .get("filename")
                .and_then(JsonValue::as_str)
                .unwrap_or_default();
            let source = LanguageModelDocumentSource::new(source_id, "text/plain", filename)
                .with_filename(filename)
                .with_provider_metadata(open_responses_annotation_metadata(
                    provider_name,
                    annotation,
                    &[("type", "type"), ("file_id", "fileId"), ("index", "index")],
                ));
            Some(LanguageModelSource::Document(source))
        }
        Some("container_file_citation") => {
            let filename = annotation
                .get("filename")
                .and_then(JsonValue::as_str)
                .unwrap_or_default();
            let source = LanguageModelDocumentSource::new(source_id, "text/plain", filename)
                .with_filename(filename)
                .with_provider_metadata(open_responses_annotation_metadata(
                    provider_name,
                    annotation,
                    &[
                        ("type", "type"),
                        ("file_id", "fileId"),
                        ("container_id", "containerId"),
                    ],
                ));
            Some(LanguageModelSource::Document(source))
        }
        Some("file_path") => {
            let file_id = annotation
                .get("file_id")
                .and_then(JsonValue::as_str)
                .unwrap_or_default();
            let source =
                LanguageModelDocumentSource::new(source_id, "application/octet-stream", file_id)
                    .with_filename(file_id)
                    .with_provider_metadata(open_responses_annotation_metadata(
                        provider_name,
                        annotation,
                        &[("type", "type"), ("file_id", "fileId"), ("index", "index")],
                    ));
            Some(LanguageModelSource::Document(source))
        }
        _ => None,
    }
}

fn open_responses_next_source_id(source_index: &mut usize) -> String {
    let source_id = format!("source-{}", *source_index);
    *source_index += 1;
    source_id
}

fn open_responses_annotation_metadata(
    provider_name: &str,
    annotation: &JsonValue,
    fields: &[(&str, &str)],
) -> ProviderMetadata {
    let mut metadata = JsonObject::new();

    for (source_key, target_key) in fields {
        if let Some(value) = annotation.get(*source_key).filter(|value| !value.is_null()) {
            metadata.insert((*target_key).to_string(), value.clone());
        }
    }

    open_responses_metadata(provider_name, metadata)
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

fn open_responses_has_provider_tool(tools: &Option<Vec<LanguageModelTool>>, tool_id: &str) -> bool {
    tools.iter().flatten().any(|tool| {
        let LanguageModelTool::Provider(tool) = tool else {
            return false;
        };

        tool.id == tool_id
    })
}

fn open_responses_custom_provider_tool_names(
    tools: &Option<Vec<LanguageModelTool>>,
) -> BTreeSet<String> {
    tools
        .iter()
        .flatten()
        .filter_map(|tool| {
            let LanguageModelTool::Provider(tool) = tool else {
                return None;
            };

            (tool.id == "openai.custom").then(|| tool.name.clone())
        })
        .collect()
}

fn open_responses_shell_provider_executed(tools: &Option<Vec<LanguageModelTool>>) -> bool {
    tools.iter().flatten().any(|tool| {
        let LanguageModelTool::Provider(tool) = tool else {
            return false;
        };

        tool.id == "openai.shell"
            && matches!(
                tool.args
                    .get("environment")
                    .and_then(JsonValue::as_object)
                    .and_then(|environment| environment.get("type"))
                    .and_then(JsonValue::as_str),
                Some("containerAuto" | "containerReference")
            )
    })
}

fn open_responses_stringified_json(value: JsonValue) -> String {
    serde_json::to_string(&value).unwrap_or_else(|_| "null".to_string())
}

fn open_responses_tool_search_call_id(part: &JsonValue) -> String {
    part.get("call_id")
        .and_then(JsonValue::as_str)
        .or_else(|| part.get("id").and_then(JsonValue::as_str))
        .unwrap_or_default()
        .to_string()
}

fn open_responses_tool_search_input(part: &JsonValue) -> String {
    json!({
        "arguments": part.get("arguments").cloned().unwrap_or(JsonValue::Null),
        "call_id": part.get("call_id").cloned().unwrap_or(JsonValue::Null)
    })
    .to_string()
}

fn open_responses_shell_call_input(part: &JsonValue) -> String {
    json!({
        "action": {
            "commands": part
                .get("action")
                .and_then(JsonValue::as_object)
                .and_then(|action| action.get("commands"))
                .cloned()
                .unwrap_or_else(|| JsonValue::Array(Vec::new()))
        }
    })
    .to_string()
}

fn open_responses_shell_call_output(part: &JsonValue) -> JsonValue {
    json!({
        "output": part
            .get("output")
            .and_then(JsonValue::as_array)
            .map(|items| {
                JsonValue::Array(
                    items
                        .iter()
                        .map(open_responses_shell_output_item)
                        .collect(),
                )
            })
            .unwrap_or_else(|| JsonValue::Array(Vec::new()))
    })
}

fn open_responses_shell_output_item(item: &JsonValue) -> JsonValue {
    let outcome = item
        .get("outcome")
        .and_then(JsonValue::as_object)
        .map(
            |outcome| match outcome.get("type").and_then(JsonValue::as_str) {
                Some("exit") => json!({
                    "type": "exit",
                    "exitCode": outcome.get("exit_code").cloned().unwrap_or(JsonValue::Null)
                }),
                _ => json!({ "type": "timeout" }),
            },
        )
        .unwrap_or_else(|| json!({ "type": "timeout" }));

    json!({
        "stdout": item.get("stdout").cloned().unwrap_or(JsonValue::Null),
        "stderr": item.get("stderr").cloned().unwrap_or(JsonValue::Null),
        "outcome": outcome
    })
}

fn open_responses_mcp_tool_result(
    part: &JsonValue,
    tool_call_id: &str,
    tool_name: &str,
) -> LanguageModelToolResult {
    let mut result = JsonObject::new();
    result.insert("type".to_string(), JsonValue::String("call".to_string()));
    result.insert(
        "serverLabel".to_string(),
        part.get("server_label").cloned().unwrap_or(JsonValue::Null),
    );
    result.insert(
        "name".to_string(),
        part.get("name").cloned().unwrap_or(JsonValue::Null),
    );
    result.insert(
        "arguments".to_string(),
        part.get("arguments").cloned().unwrap_or(JsonValue::Null),
    );

    if let Some(output) = part.get("output").filter(|output| !output.is_null()) {
        result.insert("output".to_string(), output.clone());
    }

    if let Some(error) = part.get("error").filter(|error| !error.is_null()) {
        result.insert("error".to_string(), error.clone());
    }

    let result = NonNullJsonValue::new(JsonValue::Object(result))
        .expect("MCP tool result object is non-null JSON");
    LanguageModelToolResult::new(tool_call_id, tool_name, result).with_dynamic(true)
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

fn open_responses_push_stream_tool_result(
    stream: &mut Vec<LanguageModelStreamPart>,
    tool_call_id: &str,
    tool_name: &str,
    result: JsonValue,
    provider_metadata: Option<ProviderMetadata>,
) {
    if let Ok(result) = NonNullJsonValue::new(result) {
        let mut tool_result = LanguageModelToolResult::new(tool_call_id, tool_name, result);
        if let Some(provider_metadata) = provider_metadata {
            tool_result = tool_result.with_provider_metadata(provider_metadata);
        }
        stream.push(LanguageModelStreamPart::ToolResult(tool_result));
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
    options: &LanguageModelCallOptions,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    let mut stream = vec![LanguageModelStreamPart::StreamStart(
        LanguageModelStreamStart::new(warnings),
    )];
    let mut finish_reason = LanguageModelFinishReason {
        unified: FinishReason::Other,
        raw: None,
    };
    let mut usage = LanguageModelUsage::default();
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
    let mut ongoing_tool_calls = BTreeMap::<String, OngoingOpenResponsesToolCall>::new();
    let provider_tool_names = open_responses_provider_tool_names();
    let tool_name_mapping =
        create_tool_name_mapping(options.tools.iter().flatten(), &provider_tool_names);
    let web_search_tool_name = open_responses_web_search_response_tool_name(&options.tools);
    let shell_provider_executed = open_responses_shell_provider_executed(&options.tools);
    let mut hosted_tool_search_call_ids = VecDeque::<String>::new();
    let approval_request_tool_call_ids =
        open_responses_approval_request_tool_call_ids(&options.prompt, provider_name);
    let mut approval_request_stream_tool_call_ids = BTreeMap::<String, String>::new();
    let mut source_index = 0usize;
    let mut ongoing_annotations = Vec::<JsonValue>::new();
    let mut active_message_phase = None::<String>;
    let mut active_reasoning_items = BTreeMap::<String, BTreeSet<String>>::new();
    let mut active_message_items = BTreeSet::<String>::new();
    let mut completed_message_text = BTreeMap::<String, String>::new();
    let store_response = request_body
        .get("store")
        .and_then(JsonValue::as_bool)
        .unwrap_or(true);

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
                        &mut emitted_response_metadata,
                        response,
                    );
                }

                match event_type {
                    Some("response.output_text.delta") => {
                        if let Some(delta) = value.get("delta").and_then(JsonValue::as_str)
                            && !delta.is_empty()
                        {
                            let id = open_responses_stream_text_id(&value);
                            open_responses_push_text_delta(
                                &mut stream,
                                &mut text_buffers,
                                &mut active_text,
                                &ended_text,
                                &id,
                                delta,
                                open_responses_stream_text_metadata(
                                    provider_name,
                                    Some(&id),
                                    active_message_phase.as_deref(),
                                    &[],
                                ),
                            );
                        }
                    }
                    Some("response.output_text.done") => {
                        let id = open_responses_stream_text_id(&value);
                        let text = value.get("text").and_then(JsonValue::as_str);
                        if active_message_items.contains(&id) {
                            if let Some(text) = text {
                                completed_message_text.insert(id, text.to_string());
                            }
                        } else {
                            open_responses_finish_text_block(
                                &mut stream,
                                &mut text_buffers,
                                &mut active_text,
                                &mut ended_text,
                                &id,
                                text,
                                open_responses_stream_text_metadata(
                                    provider_name,
                                    Some(&id),
                                    active_message_phase.as_deref(),
                                    &ongoing_annotations,
                                ),
                            );
                        }
                    }
                    Some("response.reasoning_summary_text.delta")
                    | Some("response.reasoning_text.delta") => {
                        if let Some(delta) = value.get("delta").and_then(JsonValue::as_str)
                            && !delta.is_empty()
                        {
                            let item_id = open_responses_stream_item_id(&value);
                            let id = open_responses_stream_reasoning_id(&value);
                            open_responses_push_reasoning_delta(
                                &mut stream,
                                &mut reasoning_buffers,
                                &mut active_reasoning,
                                &ended_reasoning,
                                &id,
                                delta,
                                open_responses_stream_reasoning_metadata(
                                    provider_name,
                                    item_id.as_deref(),
                                    None,
                                ),
                            );
                        }
                    }
                    Some("response.reasoning_summary_text.done")
                    | Some("response.reasoning_text.done") => {
                        let item_id = open_responses_stream_item_id(&value);
                        let id = open_responses_stream_reasoning_id(&value);
                        let text = value.get("text").and_then(JsonValue::as_str);
                        open_responses_finish_reasoning_block(
                            &mut stream,
                            &mut reasoning_buffers,
                            &mut active_reasoning,
                            &mut ended_reasoning,
                            &id,
                            text,
                            open_responses_stream_reasoning_metadata(
                                provider_name,
                                item_id.as_deref(),
                                None,
                            ),
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
                            let id = open_responses_stream_text_id(&value);
                            let annotations = part
                                .and_then(|part| part.get("annotations"))
                                .and_then(JsonValue::as_array)
                                .cloned()
                                .unwrap_or_default();
                            if active_message_items.contains(&id) {
                                if let Some(text) = text {
                                    completed_message_text.insert(id, text.to_string());
                                }
                            } else {
                                open_responses_finish_text_block(
                                    &mut stream,
                                    &mut text_buffers,
                                    &mut active_text,
                                    &mut ended_text,
                                    &id,
                                    text,
                                    open_responses_stream_text_metadata(
                                        provider_name,
                                        Some(&id),
                                        active_message_phase.as_deref(),
                                        &annotations,
                                    ),
                                );
                            }
                        } else if open_responses_is_reasoning_text_part(part_type) {
                            let item_id = open_responses_stream_item_id(&value);
                            let id = open_responses_stream_reasoning_id(&value);
                            open_responses_finish_reasoning_block(
                                &mut stream,
                                &mut reasoning_buffers,
                                &mut active_reasoning,
                                &mut ended_reasoning,
                                &id,
                                text,
                                open_responses_stream_reasoning_metadata(
                                    provider_name,
                                    item_id.as_deref(),
                                    None,
                                ),
                            );
                        }
                    }
                    Some("response.reasoning_summary_part.added") => {
                        if let Some(item_id) = value.get("item_id").and_then(JsonValue::as_str) {
                            let summary_index = open_responses_stream_summary_index(&value);
                            active_reasoning_items
                                .entry(item_id.to_string())
                                .or_default()
                                .insert(summary_index.clone());
                            let id = format!("{item_id}:{summary_index}");
                            open_responses_start_reasoning_block(
                                &mut stream,
                                &mut active_reasoning,
                                &ended_reasoning,
                                &id,
                                open_responses_stream_reasoning_metadata(
                                    provider_name,
                                    Some(item_id),
                                    None,
                                ),
                            );
                        }
                    }
                    Some("response.reasoning_summary_part.done") => {
                        if store_response
                            && let Some(item_id) = value.get("item_id").and_then(JsonValue::as_str)
                        {
                            let summary_index = open_responses_stream_summary_index(&value);
                            let id = format!("{item_id}:{summary_index}");
                            open_responses_finish_reasoning_block(
                                &mut stream,
                                &mut reasoning_buffers,
                                &mut active_reasoning,
                                &mut ended_reasoning,
                                &id,
                                None,
                                open_responses_stream_reasoning_metadata(
                                    provider_name,
                                    Some(item_id),
                                    None,
                                ),
                            );
                        }
                    }
                    Some("response.output_text.annotation.added") => {
                        if let Some(annotation) = value.get("annotation") {
                            ongoing_annotations.push(annotation.clone());
                            if let Some(source) = open_responses_annotation_source(
                                provider_name,
                                annotation,
                                open_responses_next_source_id(&mut source_index),
                            ) {
                                stream.push(LanguageModelStreamPart::Source(source));
                            }
                        }
                    }
                    Some("response.output_item.added") => {
                        if let Some(item) = value.get("item") {
                            match item.get("type").and_then(JsonValue::as_str) {
                                Some("function_call") => {
                                    let tool_call_id = item
                                        .get("call_id")
                                        .and_then(JsonValue::as_str)
                                        .or_else(|| item.get("id").and_then(JsonValue::as_str))
                                        .unwrap_or_default();
                                    let tool_name = item
                                        .get("name")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    if let Some(output_index) =
                                        open_responses_stream_output_index(&value)
                                    {
                                        ongoing_tool_calls.insert(
                                            output_index,
                                            OngoingOpenResponsesToolCall::new(
                                                tool_call_id,
                                                tool_name,
                                            ),
                                        );
                                    }
                                    stream.push(LanguageModelStreamPart::ToolInputStart(
                                        LanguageModelToolInputStart::new(tool_call_id, tool_name),
                                    ));
                                }
                                Some("custom_tool_call") => {
                                    let tool_call_id = item
                                        .get("call_id")
                                        .and_then(JsonValue::as_str)
                                        .or_else(|| item.get("id").and_then(JsonValue::as_str))
                                        .unwrap_or_default();
                                    let tool_name = item
                                        .get("name")
                                        .and_then(JsonValue::as_str)
                                        .map(|name| tool_name_mapping.to_custom_tool_name(name))
                                        .unwrap_or_default();
                                    if let Some(output_index) =
                                        open_responses_stream_output_index(&value)
                                    {
                                        ongoing_tool_calls.insert(
                                            output_index,
                                            OngoingOpenResponsesToolCall::new(
                                                tool_call_id,
                                                tool_name.clone(),
                                            ),
                                        );
                                    }
                                    stream.push(LanguageModelStreamPart::ToolInputStart(
                                        LanguageModelToolInputStart::new(tool_call_id, tool_name),
                                    ));
                                }
                                Some("web_search_call") => {
                                    let tool_call_id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let tool_name = tool_name_mapping
                                        .to_custom_tool_name(&web_search_tool_name);
                                    stream.push(LanguageModelStreamPart::ToolInputStart(
                                        LanguageModelToolInputStart::new(tool_call_id, &tool_name)
                                            .with_provider_executed(true),
                                    ));
                                    stream.push(LanguageModelStreamPart::ToolInputEnd(
                                        LanguageModelToolInputEnd::new(tool_call_id),
                                    ));
                                    stream.push(LanguageModelStreamPart::ToolCall(
                                        LanguageModelToolCall::new(tool_call_id, tool_name, "{}")
                                            .with_provider_executed(true),
                                    ));
                                }
                                Some("file_search_call") => {
                                    let tool_call_id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let tool_name =
                                        tool_name_mapping.to_custom_tool_name("file_search");
                                    stream.push(LanguageModelStreamPart::ToolCall(
                                        LanguageModelToolCall::new(tool_call_id, tool_name, "{}")
                                            .with_provider_executed(true),
                                    ));
                                }
                                Some("code_interpreter_call") => {
                                    let tool_call_id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let tool_name =
                                        tool_name_mapping.to_custom_tool_name("code_interpreter");
                                    let container_id = item
                                        .get("container_id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    if let Some(output_index) =
                                        open_responses_stream_output_index(&value)
                                    {
                                        ongoing_tool_calls.insert(
                                            output_index,
                                            OngoingOpenResponsesToolCall::code_interpreter(
                                                tool_call_id,
                                                tool_name.clone(),
                                                container_id,
                                            ),
                                        );
                                    }
                                    stream.push(LanguageModelStreamPart::ToolInputStart(
                                        LanguageModelToolInputStart::new(tool_call_id, &tool_name)
                                            .with_provider_executed(true),
                                    ));
                                    stream.push(LanguageModelStreamPart::ToolInputDelta(
                                        LanguageModelToolInputDelta::new(
                                            tool_call_id,
                                            format!(
                                                "{{\"containerId\":\"{}\",\"code\":\"",
                                                open_responses_escape_json_delta(container_id)
                                            ),
                                        ),
                                    ));
                                }
                                Some("image_generation_call") => {
                                    let tool_call_id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let tool_name =
                                        tool_name_mapping.to_custom_tool_name("image_generation");
                                    stream.push(LanguageModelStreamPart::ToolCall(
                                        LanguageModelToolCall::new(tool_call_id, tool_name, "{}")
                                            .with_provider_executed(true),
                                    ));
                                }
                                Some("message") => {
                                    ongoing_annotations.clear();
                                    active_message_phase = item
                                        .get("phase")
                                        .and_then(JsonValue::as_str)
                                        .map(ToString::to_string);
                                    if let Some(id) = item.get("id").and_then(JsonValue::as_str) {
                                        active_message_items.insert(id.to_string());
                                        open_responses_start_text_block(
                                            &mut stream,
                                            &mut active_text,
                                            &ended_text,
                                            id,
                                            open_responses_stream_text_metadata(
                                                provider_name,
                                                Some(id),
                                                active_message_phase.as_deref(),
                                                &[],
                                            ),
                                        );
                                    }
                                }
                                Some("reasoning") => {
                                    if let Some(item_id) =
                                        item.get("id").and_then(JsonValue::as_str)
                                    {
                                        active_reasoning_items
                                            .entry(item_id.to_string())
                                            .or_default()
                                            .insert("0".to_string());
                                        let id = format!("{item_id}:0");
                                        open_responses_start_reasoning_block(
                                            &mut stream,
                                            &mut active_reasoning,
                                            &ended_reasoning,
                                            &id,
                                            Some(open_responses_reasoning_metadata(
                                                provider_name,
                                                item,
                                            )),
                                        );
                                    }
                                }
                                Some("apply_patch_call") => {
                                    let tool_call_id = item
                                        .get("call_id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let tool_name =
                                        tool_name_mapping.to_custom_tool_name("apply_patch");
                                    let operation = item.get("operation");
                                    let delete_file = operation
                                        .and_then(|operation| operation.get("type"))
                                        .and_then(JsonValue::as_str)
                                        == Some("delete_file");

                                    if let Some(output_index) =
                                        open_responses_stream_output_index(&value)
                                    {
                                        ongoing_tool_calls.insert(
                                            output_index,
                                            OngoingOpenResponsesToolCall::apply_patch(
                                                tool_call_id,
                                                tool_name.clone(),
                                                delete_file,
                                            ),
                                        );
                                    }

                                    stream.push(LanguageModelStreamPart::ToolInputStart(
                                        LanguageModelToolInputStart::new(tool_call_id, &tool_name),
                                    ));

                                    if delete_file {
                                        stream.push(LanguageModelStreamPart::ToolInputDelta(
                                            LanguageModelToolInputDelta::new(
                                                tool_call_id,
                                                json!({
                                                    "callId": item.get("call_id").cloned().unwrap_or(JsonValue::Null),
                                                    "operation": operation.cloned().unwrap_or(JsonValue::Null)
                                                })
                                                .to_string(),
                                            ),
                                        ));
                                        stream.push(LanguageModelStreamPart::ToolInputEnd(
                                            LanguageModelToolInputEnd::new(tool_call_id),
                                        ));
                                    } else {
                                        stream.push(LanguageModelStreamPart::ToolInputDelta(
                                            LanguageModelToolInputDelta::new(
                                                tool_call_id,
                                                open_responses_apply_patch_input_prefix(
                                                    tool_call_id,
                                                    operation,
                                                ),
                                            ),
                                        ));
                                    }
                                }
                                _ => {}
                            }
                            open_responses_record_pending_tool_call(&mut pending_tool_calls, item);
                        }
                    }
                    Some("response.function_call_arguments.delta") => {
                        open_responses_append_pending_tool_call_arguments(
                            &mut pending_tool_calls,
                            &value,
                        );
                        if let Some(delta) = value.get("delta").and_then(JsonValue::as_str) {
                            open_responses_push_ongoing_tool_input_delta(
                                &mut stream,
                                &ongoing_tool_calls,
                                &value,
                                delta,
                            );
                        }
                    }
                    Some("response.function_call_arguments.done") => {
                        open_responses_finish_pending_tool_call_arguments(
                            &mut pending_tool_calls,
                            &value,
                        );
                    }
                    Some("response.custom_tool_call_input.delta") => {
                        if let Some(delta) = value.get("delta").and_then(JsonValue::as_str) {
                            open_responses_push_ongoing_tool_input_delta(
                                &mut stream,
                                &ongoing_tool_calls,
                                &value,
                                delta,
                            );
                        }
                    }
                    Some("response.apply_patch_call_operation_diff.delta") => {
                        if let Some(output_index) = open_responses_stream_output_index(&value)
                            && let Some(tool_call) = ongoing_tool_calls.get_mut(&output_index)
                            && let Some(apply_patch) = tool_call.apply_patch.as_mut()
                            && let Some(delta) = value.get("delta").and_then(JsonValue::as_str)
                        {
                            stream.push(LanguageModelStreamPart::ToolInputDelta(
                                LanguageModelToolInputDelta::new(
                                    &tool_call.tool_call_id,
                                    open_responses_escape_json_delta(delta),
                                ),
                            ));
                            apply_patch.has_diff = true;
                        }
                    }
                    Some("response.apply_patch_call_operation_diff.done") => {
                        if let Some(output_index) = open_responses_stream_output_index(&value)
                            && let Some(tool_call) = ongoing_tool_calls.get_mut(&output_index)
                        {
                            open_responses_finish_apply_patch_tool_input(
                                &mut stream,
                                tool_call,
                                value.get("diff"),
                            );
                        }
                    }
                    Some("response.image_generation_call.partial_image") => {
                        let tool_call_id = value
                            .get("item_id")
                            .and_then(JsonValue::as_str)
                            .unwrap_or_default();
                        let result = json!({
                            "result": value
                                .get("partial_image_b64")
                                .cloned()
                                .unwrap_or(JsonValue::Null)
                        });
                        if let Ok(result) = NonNullJsonValue::new(result) {
                            stream.push(LanguageModelStreamPart::ToolResult(
                                LanguageModelToolResult::new(
                                    tool_call_id,
                                    tool_name_mapping.to_custom_tool_name("image_generation"),
                                    result,
                                )
                                .with_preliminary(true),
                            ));
                        }
                    }
                    Some("response.code_interpreter_call_code.delta") => {
                        if let Some(output_index) = open_responses_stream_output_index(&value)
                            && let Some(tool_call) = ongoing_tool_calls.get_mut(&output_index)
                            && let Some(code) = tool_call.code_interpreter.as_mut()
                            && let Some(delta) = value.get("delta").and_then(JsonValue::as_str)
                        {
                            stream.push(LanguageModelStreamPart::ToolInputDelta(
                                LanguageModelToolInputDelta::new(
                                    &tool_call.tool_call_id,
                                    open_responses_escape_json_delta(delta),
                                ),
                            ));
                            code.has_code_delta = true;
                        }
                    }
                    Some("response.code_interpreter_call_code.done") => {
                        if let Some(output_index) = open_responses_stream_output_index(&value)
                            && let Some(tool_call) = ongoing_tool_calls.get_mut(&output_index)
                        {
                            open_responses_finish_code_interpreter_tool_input(
                                &mut stream,
                                tool_call,
                                value.get("code"),
                                None,
                            );
                        }
                    }
                    Some("response.output_item.done") => {
                        if let Some(item) = value.get("item") {
                            match item.get("type").and_then(JsonValue::as_str) {
                                Some("function_call") => {
                                    if let Some(output_index) =
                                        open_responses_stream_output_index(&value)
                                        && let Some(tool_call) =
                                            ongoing_tool_calls.remove(&output_index)
                                    {
                                        let mut input_end =
                                            LanguageModelToolInputEnd::new(tool_call.tool_call_id);
                                        if let Some(metadata) =
                                            open_responses_namespace_metadata(provider_name, item)
                                        {
                                            input_end = input_end.with_provider_metadata(metadata);
                                        }
                                        stream
                                            .push(LanguageModelStreamPart::ToolInputEnd(input_end));
                                    }
                                    if open_responses_push_tool_call_from_item(
                                        &mut stream,
                                        &mut emitted_tool_calls,
                                        &mut pending_tool_calls,
                                        provider_name,
                                        item,
                                    ) {
                                        has_tool_calls = true;
                                    }
                                }
                                Some("web_search_call") => {
                                    let tool_call_id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let tool_name = tool_name_mapping
                                        .to_custom_tool_name(&web_search_tool_name);
                                    open_responses_push_stream_tool_result(
                                        &mut stream,
                                        tool_call_id,
                                        &tool_name,
                                        open_responses_web_search_output(item.get("action")),
                                        None,
                                    );
                                }
                                Some("file_search_call") => {
                                    let tool_call_id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let tool_name =
                                        tool_name_mapping.to_custom_tool_name("file_search");
                                    open_responses_push_stream_tool_result(
                                        &mut stream,
                                        tool_call_id,
                                        &tool_name,
                                        open_responses_file_search_output(item),
                                        None,
                                    );
                                }
                                Some("code_interpreter_call") => {
                                    let tool_call_id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let tool_name =
                                        tool_name_mapping.to_custom_tool_name("code_interpreter");
                                    let emitted_tool_call =
                                        open_responses_stream_output_index(&value)
                                            .and_then(|output_index| {
                                                ongoing_tool_calls.remove(&output_index)
                                            })
                                            .is_some_and(|mut tool_call| {
                                                open_responses_finish_code_interpreter_tool_input(
                                                    &mut stream,
                                                    &mut tool_call,
                                                    item.get("code"),
                                                    item.get("container_id"),
                                                )
                                            });
                                    if !emitted_tool_call {
                                        stream.push(LanguageModelStreamPart::ToolInputEnd(
                                            LanguageModelToolInputEnd::new(tool_call_id),
                                        ));
                                        stream.push(LanguageModelStreamPart::ToolCall(
                                            LanguageModelToolCall::new(
                                                tool_call_id,
                                                tool_name.clone(),
                                                json!({
                                                    "code": item.get("code").cloned().unwrap_or(JsonValue::Null),
                                                    "containerId": item.get("container_id").cloned().unwrap_or(JsonValue::Null)
                                                })
                                                .to_string(),
                                            )
                                            .with_provider_executed(true),
                                        ));
                                    }
                                    open_responses_push_stream_tool_result(
                                        &mut stream,
                                        tool_call_id,
                                        &tool_name,
                                        json!({
                                            "outputs": item.get("outputs").cloned().unwrap_or(JsonValue::Null)
                                        }),
                                        None,
                                    );
                                }
                                Some("image_generation_call") => {
                                    let tool_call_id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let tool_name =
                                        tool_name_mapping.to_custom_tool_name("image_generation");
                                    open_responses_push_stream_tool_result(
                                        &mut stream,
                                        tool_call_id,
                                        &tool_name,
                                        json!({
                                            "result": item.get("result").cloned().unwrap_or(JsonValue::Null)
                                        }),
                                        None,
                                    );
                                }
                                Some("custom_tool_call") => {
                                    has_tool_calls = true;
                                    if let Some(output_index) =
                                        open_responses_stream_output_index(&value)
                                        && let Some(tool_call) =
                                            ongoing_tool_calls.remove(&output_index)
                                    {
                                        stream.push(LanguageModelStreamPart::ToolInputEnd(
                                            LanguageModelToolInputEnd::new(tool_call.tool_call_id),
                                        ));
                                    }
                                    let tool_call_id = item
                                        .get("call_id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let tool_name = item
                                        .get("name")
                                        .and_then(JsonValue::as_str)
                                        .map(|name| tool_name_mapping.to_custom_tool_name(name))
                                        .unwrap_or_default();
                                    let input = open_responses_stringified_json(
                                        item.get("input").cloned().unwrap_or(JsonValue::Null),
                                    );
                                    let mut tool_call =
                                        LanguageModelToolCall::new(tool_call_id, tool_name, input);
                                    if let Some(metadata) =
                                        open_responses_item_metadata(provider_name, item)
                                    {
                                        tool_call = tool_call.with_provider_metadata(metadata);
                                    }
                                    stream.push(LanguageModelStreamPart::ToolCall(tool_call));
                                }
                                Some("tool_search_call") => {
                                    let tool_call_id = open_responses_tool_search_call_id(item);
                                    let hosted = matches!(
                                        item.get("execution").and_then(JsonValue::as_str),
                                        Some("server")
                                    );

                                    if hosted {
                                        hosted_tool_search_call_ids.push_back(tool_call_id.clone());
                                    }

                                    let mut tool_call = LanguageModelToolCall::new(
                                        tool_call_id,
                                        tool_name_mapping.to_custom_tool_name("tool_search"),
                                        open_responses_tool_search_input(item),
                                    );

                                    if hosted {
                                        tool_call = tool_call.with_provider_executed(true);
                                    }

                                    if let Some(metadata) =
                                        open_responses_item_metadata(provider_name, item)
                                    {
                                        tool_call = tool_call.with_provider_metadata(metadata);
                                    }

                                    stream.push(LanguageModelStreamPart::ToolCall(tool_call));
                                }
                                Some("tool_search_output") => {
                                    let tool_call_id = item
                                        .get("call_id")
                                        .and_then(JsonValue::as_str)
                                        .map(ToString::to_string)
                                        .or_else(|| hosted_tool_search_call_ids.pop_front())
                                        .or_else(|| {
                                            item.get("id")
                                                .and_then(JsonValue::as_str)
                                                .map(ToString::to_string)
                                        })
                                        .unwrap_or_default();
                                    open_responses_push_stream_tool_result(
                                        &mut stream,
                                        &tool_call_id,
                                        &tool_name_mapping.to_custom_tool_name("tool_search"),
                                        json!({
                                            "tools": item.get("tools").cloned().unwrap_or_else(|| JsonValue::Array(Vec::new()))
                                        }),
                                        open_responses_item_metadata(provider_name, item),
                                    );
                                }
                                Some("local_shell_call") => {
                                    let tool_call_id = item
                                        .get("call_id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();

                                    let mut tool_call = LanguageModelToolCall::new(
                                        tool_call_id,
                                        tool_name_mapping.to_custom_tool_name("local_shell"),
                                        json!({
                                            "action": item.get("action").cloned().unwrap_or(JsonValue::Null)
                                        })
                                        .to_string(),
                                    );
                                    if let Some(metadata) =
                                        open_responses_item_metadata(provider_name, item)
                                    {
                                        tool_call = tool_call.with_provider_metadata(metadata);
                                    }
                                    stream.push(LanguageModelStreamPart::ToolCall(tool_call));
                                }
                                Some("shell_call") => {
                                    let tool_call_id = item
                                        .get("call_id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let mut tool_call = LanguageModelToolCall::new(
                                        tool_call_id,
                                        tool_name_mapping.to_custom_tool_name("shell"),
                                        open_responses_shell_call_input(item),
                                    );

                                    if shell_provider_executed {
                                        tool_call = tool_call.with_provider_executed(true);
                                    }

                                    if let Some(metadata) =
                                        open_responses_item_metadata(provider_name, item)
                                    {
                                        tool_call = tool_call.with_provider_metadata(metadata);
                                    }

                                    stream.push(LanguageModelStreamPart::ToolCall(tool_call));
                                }
                                Some("shell_call_output") => {
                                    let tool_call_id = item
                                        .get("call_id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    open_responses_push_stream_tool_result(
                                        &mut stream,
                                        tool_call_id,
                                        &tool_name_mapping.to_custom_tool_name("shell"),
                                        open_responses_shell_call_output(item),
                                        None,
                                    );
                                }
                                Some("apply_patch_call") => {
                                    let tool_call_id = item
                                        .get("call_id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    if let Some(output_index) =
                                        open_responses_stream_output_index(&value)
                                        && let Some(mut tool_call) =
                                            ongoing_tool_calls.remove(&output_index)
                                    {
                                        open_responses_finish_apply_patch_tool_input(
                                            &mut stream,
                                            &mut tool_call,
                                            item.get("operation")
                                                .and_then(|operation| operation.get("diff")),
                                        );
                                    }

                                    if item.get("status").and_then(JsonValue::as_str)
                                        == Some("completed")
                                    {
                                        let mut tool_call = LanguageModelToolCall::new(
                                            tool_call_id,
                                            tool_name_mapping.to_custom_tool_name("apply_patch"),
                                            json!({
                                                "callId": item.get("call_id").cloned().unwrap_or(JsonValue::Null),
                                                "operation": item.get("operation").cloned().unwrap_or(JsonValue::Null)
                                            })
                                            .to_string(),
                                        );
                                        if let Some(metadata) =
                                            open_responses_item_metadata(provider_name, item)
                                        {
                                            tool_call = tool_call.with_provider_metadata(metadata);
                                        }
                                        stream.push(LanguageModelStreamPart::ToolCall(tool_call));
                                    }
                                }
                                Some("mcp_call") => {
                                    let tool_call_id = open_responses_mcp_tool_call_id(
                                        item,
                                        &approval_request_tool_call_ids,
                                        &approval_request_stream_tool_call_ids,
                                    );
                                    let tool_name = item
                                        .get("name")
                                        .and_then(JsonValue::as_str)
                                        .map(|name| format!("mcp.{name}"))
                                        .unwrap_or_else(|| "mcp".to_string());
                                    let input = item
                                        .get("arguments")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or("{}");

                                    stream.push(LanguageModelStreamPart::ToolCall(
                                        LanguageModelToolCall::new(
                                            &tool_call_id,
                                            tool_name.clone(),
                                            input,
                                        )
                                        .with_provider_executed(true)
                                        .with_dynamic(true),
                                    ));
                                    stream.push(LanguageModelStreamPart::ToolResult({
                                        let mut tool_result = open_responses_mcp_tool_result(
                                            item,
                                            &tool_call_id,
                                            &tool_name,
                                        );
                                        if let Some(metadata) =
                                            open_responses_item_metadata(provider_name, item)
                                        {
                                            tool_result =
                                                tool_result.with_provider_metadata(metadata);
                                        }
                                        tool_result
                                    }));
                                }
                                Some("mcp_approval_request") => {
                                    let approval_id = item
                                        .get("approval_request_id")
                                        .and_then(JsonValue::as_str)
                                        .or_else(|| item.get("id").and_then(JsonValue::as_str))
                                        .unwrap_or_default();
                                    let tool_call_id = generate_id();
                                    approval_request_stream_tool_call_ids
                                        .insert(approval_id.to_string(), tool_call_id.clone());
                                    let tool_name = item
                                        .get("name")
                                        .and_then(JsonValue::as_str)
                                        .map(|name| format!("mcp.{name}"))
                                        .unwrap_or_else(|| "mcp".to_string());
                                    let input = item
                                        .get("arguments")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or("{}");

                                    let mut tool_call =
                                        LanguageModelToolCall::new(&tool_call_id, tool_name, input)
                                            .with_provider_executed(true)
                                            .with_dynamic(true);
                                    if let Some(metadata) = open_responses_mcp_approval_metadata(
                                        provider_name,
                                        item,
                                        approval_id,
                                    ) {
                                        tool_call = tool_call.with_provider_metadata(metadata);
                                    }
                                    stream.push(LanguageModelStreamPart::ToolCall(tool_call));
                                    stream.push(LanguageModelStreamPart::ToolApprovalRequest(
                                        LanguageModelToolApprovalRequest::new(
                                            approval_id,
                                            &tool_call_id,
                                        ),
                                    ));
                                }
                                Some("computer_call") => {
                                    let tool_call_id = item
                                        .get("id")
                                        .and_then(JsonValue::as_str)
                                        .unwrap_or_default();
                                    let tool_name =
                                        tool_name_mapping.to_custom_tool_name("computer_use");

                                    stream.push(LanguageModelStreamPart::ToolCall(
                                        LanguageModelToolCall::new(
                                            tool_call_id,
                                            tool_name.clone(),
                                            "",
                                        )
                                        .with_provider_executed(true),
                                    ));
                                    open_responses_push_stream_tool_result(
                                        &mut stream,
                                        tool_call_id,
                                        &tool_name,
                                        json!({
                                            "type": "computer_use_tool_result",
                                            "status": item
                                                .get("status")
                                                .cloned()
                                                .unwrap_or_else(|| JsonValue::String("completed".to_string()))
                                        }),
                                        None,
                                    );
                                }
                                Some("message") => {
                                    if let Some(id) = item.get("id").and_then(JsonValue::as_str) {
                                        let phase = item
                                            .get("phase")
                                            .and_then(JsonValue::as_str)
                                            .or(active_message_phase.as_deref());
                                        let final_text = completed_message_text.remove(id);
                                        open_responses_finish_text_block(
                                            &mut stream,
                                            &mut text_buffers,
                                            &mut active_text,
                                            &mut ended_text,
                                            id,
                                            final_text.as_deref(),
                                            open_responses_stream_text_metadata(
                                                provider_name,
                                                Some(id),
                                                phase,
                                                &ongoing_annotations,
                                            ),
                                        );
                                        active_message_items.remove(id);
                                    }
                                    active_message_phase = None;
                                }
                                Some("reasoning") => {
                                    if let Some(item_id) =
                                        item.get("id").and_then(JsonValue::as_str)
                                    {
                                        let summary_indices = active_reasoning_items
                                            .remove(item_id)
                                            .unwrap_or_else(|| BTreeSet::from(["0".to_string()]));
                                        for summary_index in summary_indices {
                                            let id = format!("{item_id}:{summary_index}");
                                            open_responses_start_reasoning_block(
                                                &mut stream,
                                                &mut active_reasoning,
                                                &ended_reasoning,
                                                &id,
                                                Some(open_responses_reasoning_metadata(
                                                    provider_name,
                                                    item,
                                                )),
                                            );
                                            open_responses_finish_reasoning_block(
                                                &mut stream,
                                                &mut reasoning_buffers,
                                                &mut active_reasoning,
                                                &mut ended_reasoning,
                                                &id,
                                                None,
                                                Some(open_responses_reasoning_metadata(
                                                    provider_name,
                                                    item,
                                                )),
                                            );
                                        }
                                    }
                                }
                                Some("compaction") => {
                                    stream.push(LanguageModelStreamPart::Custom(
                                        LanguageModelCustomContent::new("openai.compaction")
                                            .with_provider_metadata(
                                                open_responses_compaction_metadata(
                                                    provider_name,
                                                    item,
                                                ),
                                            ),
                                    ));
                                }
                                _ => {
                                    if open_responses_push_tool_call_from_item(
                                        &mut stream,
                                        &mut emitted_tool_calls,
                                        &mut pending_tool_calls,
                                        provider_name,
                                        item,
                                    ) {
                                        has_tool_calls = true;
                                    }
                                }
                            }
                        }
                    }
                    Some("response.completed") => {
                        if let Some(response) = open_responses_event_response(&value) {
                            usage = open_responses_usage(response.get("usage"));
                            has_tool_calls |= open_responses_push_tool_calls_from_response(
                                &mut stream,
                                &mut emitted_tool_calls,
                                &mut pending_tool_calls,
                                provider_name,
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
                        if let Some(response) = open_responses_event_response(&value) {
                            usage = open_responses_usage(response.get("usage"));
                            finish_reason = LanguageModelFinishReason {
                                unified: FinishReason::Error,
                                raw: open_responses_failed_raw_finish_reason(response),
                            };
                        }
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

    stream.push(LanguageModelStreamPart::Finish(
        LanguageModelStreamFinish::new(usage, finish_reason),
    ));

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

fn open_responses_failed_raw_finish_reason(response: &JsonValue) -> Option<String> {
    response
        .get("error")
        .and_then(|error| error.get("code"))
        .and_then(JsonValue::as_str)
        .or_else(|| response.get("status").and_then(JsonValue::as_str))
        .map(ToString::to_string)
}

fn open_responses_push_response_metadata(
    stream: &mut Vec<LanguageModelStreamPart>,
    emitted_response_metadata: &mut bool,
    response: &JsonValue,
) {
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

fn open_responses_stream_item_id(value: &JsonValue) -> Option<String> {
    value
        .get("item_id")
        .or_else(|| value.get("item").and_then(|item| item.get("id")))
        .and_then(JsonValue::as_str)
        .map(ToString::to_string)
}

fn open_responses_stream_text_id(value: &JsonValue) -> String {
    open_responses_stream_item_id(value)
        .unwrap_or_else(|| open_responses_stream_block_id("txt", value))
}

fn open_responses_stream_summary_index(value: &JsonValue) -> String {
    open_responses_json_index(value.get("summary_index"))
        .or_else(|| open_responses_json_index(value.get("content_index")))
        .unwrap_or_else(|| "0".to_string())
}

fn open_responses_stream_reasoning_id(value: &JsonValue) -> String {
    open_responses_stream_item_id(value)
        .map(|item_id| format!("{item_id}:{}", open_responses_stream_summary_index(value)))
        .unwrap_or_else(|| open_responses_stream_block_id("reasoning", value))
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

fn open_responses_stream_text_metadata(
    provider_name: &str,
    item_id: Option<&str>,
    phase: Option<&str>,
    annotations: &[JsonValue],
) -> Option<ProviderMetadata> {
    let mut metadata = JsonObject::new();

    if let Some(item_id) = item_id {
        metadata.insert("itemId".to_string(), JsonValue::String(item_id.to_string()));
    }

    if let Some(phase) = phase {
        metadata.insert("phase".to_string(), JsonValue::String(phase.to_string()));
    }

    if !annotations.is_empty() {
        metadata.insert(
            "annotations".to_string(),
            JsonValue::Array(annotations.to_vec()),
        );
    }

    (!metadata.is_empty()).then(|| open_responses_metadata(provider_name, metadata))
}

fn open_responses_stream_reasoning_metadata(
    provider_name: &str,
    item_id: Option<&str>,
    encrypted_content: Option<JsonValue>,
) -> Option<ProviderMetadata> {
    let item_id = item_id?;
    let mut metadata = JsonObject::new();
    metadata.insert("itemId".to_string(), JsonValue::String(item_id.to_string()));

    if let Some(encrypted_content) = encrypted_content {
        metadata.insert("reasoningEncryptedContent".to_string(), encrypted_content);
    }

    Some(open_responses_metadata(provider_name, metadata))
}

fn open_responses_start_text_block(
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

fn open_responses_push_text_delta(
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

    open_responses_start_text_block(stream, active_text, ended_text, id, provider_metadata);

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
    provider_metadata: Option<ProviderMetadata>,
) {
    if ended_text.contains(id) {
        return;
    }

    let buffered = text_buffers.remove(id).unwrap_or_default();
    let emitted_final_text = buffered.is_empty() && final_text.is_some_and(|text| !text.is_empty());
    if emitted_final_text && let Some(text) = final_text {
        open_responses_push_text_delta(
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

fn open_responses_start_reasoning_block(
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

fn open_responses_push_reasoning_delta(
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

    open_responses_start_reasoning_block(
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
    let mut delta_part = LanguageModelReasoningDelta::new(id, delta);
    if let Some(provider_metadata) = provider_metadata {
        delta_part = delta_part.with_provider_metadata(provider_metadata);
    }
    stream.push(LanguageModelStreamPart::ReasoningDelta(delta_part));
}

fn open_responses_finish_reasoning_block(
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
        open_responses_push_reasoning_delta(
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
    provider_name: &str,
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
            provider_name,
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
    provider_name: &str,
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

    let mut tool_call = LanguageModelToolCall::new(tool_call_id, tool_name, input);
    if let Some(metadata) = open_responses_item_metadata(provider_name, item) {
        tool_call = tool_call.with_provider_metadata(metadata);
    }
    stream.push(LanguageModelStreamPart::ToolCall(tool_call));
    true
}

#[derive(Clone, Debug, Default)]
struct PendingOpenResponsesToolCall {
    tool_name: Option<String>,
    tool_call_id: Option<String>,
    arguments: Option<String>,
}

#[derive(Clone, Debug)]
struct OngoingOpenResponsesToolCall {
    tool_call_id: String,
    tool_name: String,
    code_interpreter: Option<OngoingOpenResponsesCodeInterpreter>,
    apply_patch: Option<OngoingOpenResponsesApplyPatch>,
}

impl OngoingOpenResponsesToolCall {
    fn new(tool_call_id: impl Into<String>, tool_name: impl Into<String>) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            code_interpreter: None,
            apply_patch: None,
        }
    }

    fn code_interpreter(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        container_id: impl Into<String>,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            code_interpreter: Some(OngoingOpenResponsesCodeInterpreter {
                container_id: Some(container_id.into()),
                has_code_delta: false,
                end_emitted: false,
                tool_call_emitted: false,
            }),
            apply_patch: None,
        }
    }

    fn apply_patch(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        delete_file: bool,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            code_interpreter: None,
            apply_patch: Some(OngoingOpenResponsesApplyPatch {
                has_diff: delete_file,
                end_emitted: delete_file,
            }),
        }
    }
}

#[derive(Clone, Debug)]
struct OngoingOpenResponsesCodeInterpreter {
    container_id: Option<String>,
    has_code_delta: bool,
    end_emitted: bool,
    tool_call_emitted: bool,
}

#[derive(Clone, Debug)]
struct OngoingOpenResponsesApplyPatch {
    has_diff: bool,
    end_emitted: bool,
}

fn open_responses_stream_output_index(value: &JsonValue) -> Option<String> {
    value
        .get("output_index")
        .and_then(|output_index| match output_index {
            JsonValue::Number(number) => Some(number.to_string()),
            JsonValue::String(output_index) => Some(output_index.clone()),
            _ => None,
        })
}

fn open_responses_push_ongoing_tool_input_delta(
    stream: &mut Vec<LanguageModelStreamPart>,
    ongoing_tool_calls: &BTreeMap<String, OngoingOpenResponsesToolCall>,
    value: &JsonValue,
    delta: &str,
) {
    if let Some(output_index) = open_responses_stream_output_index(value)
        && let Some(tool_call) = ongoing_tool_calls.get(&output_index)
    {
        stream.push(LanguageModelStreamPart::ToolInputDelta(
            LanguageModelToolInputDelta::new(&tool_call.tool_call_id, delta),
        ));
    }
}

fn open_responses_escape_json_delta(delta: &str) -> String {
    let encoded = serde_json::to_string(delta).expect("string JSON serialization cannot fail");
    encoded[1..encoded.len() - 1].to_string()
}

fn open_responses_apply_patch_input_prefix(
    tool_call_id: &str,
    operation: Option<&JsonValue>,
) -> String {
    let operation_type = operation
        .and_then(|operation| operation.get("type"))
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    let path = operation
        .and_then(|operation| operation.get("path"))
        .and_then(JsonValue::as_str)
        .unwrap_or_default();

    format!(
        "{{\"callId\":\"{}\",\"operation\":{{\"type\":\"{}\",\"path\":\"{}\",\"diff\":\"",
        open_responses_escape_json_delta(tool_call_id),
        open_responses_escape_json_delta(operation_type),
        open_responses_escape_json_delta(path)
    )
}

fn open_responses_finish_code_interpreter_tool_input(
    stream: &mut Vec<LanguageModelStreamPart>,
    tool_call: &mut OngoingOpenResponsesToolCall,
    code_value: Option<&JsonValue>,
    container_value: Option<&JsonValue>,
) -> bool {
    let Some(code_interpreter) = tool_call.code_interpreter.as_mut() else {
        return false;
    };

    let tool_call_id = tool_call.tool_call_id.clone();
    let tool_name = tool_call.tool_name.clone();

    if !code_interpreter.end_emitted {
        if !code_interpreter.has_code_delta {
            if let Some(code) = code_value.and_then(JsonValue::as_str) {
                stream.push(LanguageModelStreamPart::ToolInputDelta(
                    LanguageModelToolInputDelta::new(
                        &tool_call_id,
                        open_responses_escape_json_delta(code),
                    ),
                ));
                code_interpreter.has_code_delta = true;
            }
        }

        stream.push(LanguageModelStreamPart::ToolInputDelta(
            LanguageModelToolInputDelta::new(&tool_call_id, "\"}"),
        ));
        stream.push(LanguageModelStreamPart::ToolInputEnd(
            LanguageModelToolInputEnd::new(&tool_call_id),
        ));
        code_interpreter.end_emitted = true;
    }

    if !code_interpreter.tool_call_emitted {
        let container_id = code_interpreter
            .container_id
            .as_ref()
            .filter(|container_id| !container_id.is_empty())
            .map(|container_id| JsonValue::String(container_id.clone()))
            .or_else(|| container_value.cloned())
            .unwrap_or(JsonValue::Null);

        stream.push(LanguageModelStreamPart::ToolCall(
            LanguageModelToolCall::new(
                &tool_call_id,
                tool_name,
                json!({
                    "code": code_value.cloned().unwrap_or(JsonValue::Null),
                    "containerId": container_id
                })
                .to_string(),
            )
            .with_provider_executed(true),
        ));
        code_interpreter.tool_call_emitted = true;
    }

    true
}

fn open_responses_finish_apply_patch_tool_input(
    stream: &mut Vec<LanguageModelStreamPart>,
    tool_call: &mut OngoingOpenResponsesToolCall,
    diff_value: Option<&JsonValue>,
) -> bool {
    let Some(apply_patch) = tool_call.apply_patch.as_mut() else {
        return false;
    };

    if !apply_patch.end_emitted {
        if !apply_patch.has_diff {
            if let Some(diff) = diff_value.and_then(JsonValue::as_str) {
                stream.push(LanguageModelStreamPart::ToolInputDelta(
                    LanguageModelToolInputDelta::new(
                        &tool_call.tool_call_id,
                        open_responses_escape_json_delta(diff),
                    ),
                ));
                apply_patch.has_diff = true;
            }
        }

        stream.push(LanguageModelStreamPart::ToolInputDelta(
            LanguageModelToolInputDelta::new(&tool_call.tool_call_id, "\"}}"),
        ));
        stream.push(LanguageModelStreamPart::ToolInputEnd(
            LanguageModelToolInputEnd::new(&tool_call.tool_call_id),
        ));
        apply_patch.end_emitted = true;
    }

    true
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
        FinishReason, LanguageModel, LanguageModelAssistantContentPart,
        LanguageModelAssistantMessage, LanguageModelCallOptions, LanguageModelContent,
        LanguageModelCustomPart, LanguageModelFilePart, LanguageModelMessage,
        LanguageModelProviderTool, LanguageModelReasoningEffort, LanguageModelReasoningPart,
        LanguageModelResponseFormat, LanguageModelSource, LanguageModelStreamPart,
        LanguageModelTextPart, LanguageModelTool, LanguageModelToolApprovalRequestPart,
        LanguageModelToolApprovalResponsePart, LanguageModelToolCallPart, LanguageModelToolChoice,
        LanguageModelToolContentPart, LanguageModelToolMessage, LanguageModelToolResultContentPart,
        LanguageModelToolResultOutput, LanguageModelToolResultPart, LanguageModelUserContentPart,
        LanguageModelUserMessage,
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

    fn openai_metadata_value<'a>(
        provider_metadata: &'a Option<ProviderMetadata>,
        key: &str,
    ) -> Option<&'a JsonValue> {
        provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("openai"))
            .and_then(|metadata| metadata.get(key))
    }

    fn open_responses_test_shell_tool() -> LanguageModelTool {
        let mut args = JsonObject::new();
        args.insert(
            "environment".to_string(),
            json!({
                "type": "containerAuto"
            }),
        );
        LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "openai.shell",
            "shell",
            args,
        ))
    }

    fn open_responses_test_local_shell_tool() -> LanguageModelTool {
        LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "openai.local_shell",
            "local_shell",
            JsonObject::new(),
        ))
    }

    fn open_responses_test_apply_patch_tool() -> LanguageModelTool {
        LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "openai.apply_patch",
            "apply_patch",
            JsonObject::new(),
        ))
    }

    fn open_responses_test_custom_tool() -> LanguageModelTool {
        LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "openai.custom",
            "write_sql",
            JsonObject::new(),
        ))
    }

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
        assert!(result.provider_metadata.is_none());

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
    fn open_responses_provider_converts_tool_approval_responses_to_mcp_input() {
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
                        "id": "resp_approval",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Approval recorded"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 5,
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
        let approval_provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "approvalId": "approval_1"
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolApprovalResponse(
                    LanguageModelToolApprovalResponsePart::new("approval_1", false)
                        .with_reason("policy block"),
                ),
                LanguageModelToolContentPart::ToolApprovalResponse(
                    LanguageModelToolApprovalResponsePart::new("approval_1", false),
                ),
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "mcp_call_1",
                    "mcp.deploy",
                    LanguageModelToolResultOutput::execution_denied()
                        .with_reason("policy block")
                        .with_provider_options(approval_provider_options),
                )),
            ])),
        ])));

        assert!(result.warnings.is_empty());
        assert_eq!(result.content.len(), 1);

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
                        "type": "item_reference",
                        "id": "approval_1"
                    },
                    {
                        "type": "mcp_approval_response",
                        "approval_request_id": "approval_1",
                        "approve": false
                    }
                ]
            }))
        );
    }

    #[test]
    fn open_responses_provider_aliases_mcp_calls_from_prompt_approval_metadata() {
        let transport: OpenResponsesTransport =
            Arc::new(move |_request| -> OpenResponsesTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_mcp_approval_alias",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "id": "mcp_call_after_approval",
                                "type": "mcp_call",
                                "status": "completed",
                                "approval_request_id": "approval_1",
                                "arguments": "{\"target\":\"prod\"}",
                                "name": "deploy",
                                "server_label": "deployments",
                                "output": "{\"deployed\":true}"
                            }
                        ],
                        "usage": {
                            "input_tokens": 8,
                            "output_tokens": 5
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
        let approval_metadata: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "approvalRequestId": "approval_1"
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::ToolCall(
                    LanguageModelToolCallPart::new(
                        "pending_tool_call_1",
                        "mcp.deploy",
                        json!({
                            "target": "prod"
                        }),
                    )
                    .with_provider_executed(true)
                    .with_provider_options(approval_metadata),
                ),
                LanguageModelAssistantContentPart::ToolApprovalRequest(
                    LanguageModelToolApprovalRequestPart::new(
                        "approval_1",
                        "pending_tool_call_1",
                    ),
                ),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolApprovalResponse(
                    LanguageModelToolApprovalResponsePart::new("approval_1", true),
                ),
            ])),
        ])));

        let tool_calls = result
            .content
            .iter()
            .filter_map(|part| match part {
                LanguageModelContent::ToolCall(tool_call) => Some(tool_call),
                _ => None,
            })
            .collect::<Vec<_>>();
        let tool_results = result
            .content
            .iter()
            .filter_map(|part| match part {
                LanguageModelContent::ToolResult(tool_result) => Some(tool_result),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_results.len(), 1);
        assert_eq!(tool_calls[0].tool_call_id, "pending_tool_call_1");
        assert_eq!(tool_calls[0].tool_name, "mcp.deploy");
        assert_eq!(tool_results[0].tool_call_id, "pending_tool_call_1");
        assert_eq!(tool_results[0].tool_name, "mcp.deploy");
    }

    #[test]
    fn open_responses_provider_uses_item_references_for_stored_assistant_history() {
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
                        "id": "resp_refs",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "References accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 4,
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
        let item_options = |item_id: &str| -> ProviderOptions {
            serde_json::from_value(json!({
                "openai": {
                    "itemId": item_id
                }
            }))
            .expect("provider options deserialize")
        };

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Text(
                    LanguageModelTextPart::new("stored text")
                        .with_provider_options(item_options("message_item")),
                ),
                LanguageModelAssistantContentPart::Reasoning(
                    LanguageModelReasoningPart::new("stored reasoning")
                        .with_provider_options(item_options("reasoning_item")),
                ),
                LanguageModelAssistantContentPart::Custom(
                    LanguageModelCustomPart::new("openai.compaction")
                        .with_provider_options(item_options("compaction_item")),
                ),
                LanguageModelAssistantContentPart::ToolCall(
                    LanguageModelToolCallPart::new(
                        "provider_call_1",
                        "mcp.lookup",
                        json!({
                            "query": "rust"
                        }),
                    )
                    .with_provider_executed(true)
                    .with_provider_options(item_options("mcp_call_item")),
                ),
                LanguageModelAssistantContentPart::ToolResult(
                    LanguageModelToolResultPart::new(
                        "provider_call_1",
                        "mcp.lookup",
                        LanguageModelToolResultOutput::json(json!({
                            "answer": "ok"
                        })),
                    )
                    .with_provider_options(item_options("mcp_result_item")),
                ),
            ])),
        ])));

        assert!(result.warnings.is_empty());
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
                        "type": "item_reference",
                        "id": "message_item"
                    },
                    {
                        "type": "item_reference",
                        "id": "reasoning_item"
                    },
                    {
                        "type": "item_reference",
                        "id": "compaction_item"
                    },
                    {
                        "type": "item_reference",
                        "id": "mcp_call_item"
                    },
                    {
                        "type": "item_reference",
                        "id": "mcp_result_item"
                    }
                ]
            }))
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_reasoning_history_with_store_false() {
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
                        "id": "resp_reasoning_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Reasoning accepted"
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
        let reasoning_options =
            |item_id: Option<&str>, encrypted_content: Option<&str>| -> ProviderOptions {
                let mut openai = JsonObject::new();
                if let Some(item_id) = item_id {
                    openai.insert("itemId".to_string(), JsonValue::String(item_id.to_string()));
                }
                if let Some(encrypted_content) = encrypted_content {
                    openai.insert(
                        "reasoningEncryptedContent".to_string(),
                        JsonValue::String(encrypted_content.to_string()),
                    );
                }

                let mut options = ProviderOptions::new();
                options.insert("openai".to_string(), openai);
                options
            };
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Visible before reasoning",
                        )),
                        LanguageModelAssistantContentPart::Reasoning(
                            LanguageModelReasoningPart::new("First reasoning step")
                                .with_provider_options(reasoning_options(
                                    Some("reasoning_001"),
                                    None,
                                )),
                        ),
                        LanguageModelAssistantContentPart::Reasoning(
                            LanguageModelReasoningPart::new("Second reasoning step")
                                .with_provider_options(reasoning_options(
                                    Some("reasoning_001"),
                                    Some("encrypted_content_001"),
                                )),
                        ),
                        LanguageModelAssistantContentPart::Reasoning(
                            LanguageModelReasoningPart::new("Reasoning without item id")
                                .with_provider_options(reasoning_options(
                                    None,
                                    Some("encrypted_without_id"),
                                )),
                        ),
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Visible after reasoning",
                        )),
                    ]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "Visible before reasoning"
                            }
                        ]
                    },
                    {
                        "type": "reasoning",
                        "id": "reasoning_001",
                        "encrypted_content": "encrypted_content_001",
                        "summary": [
                            {
                                "type": "summary_text",
                                "text": "First reasoning step"
                            },
                            {
                                "type": "summary_text",
                                "text": "Second reasoning step"
                            }
                        ]
                    },
                    {
                        "type": "reasoning",
                        "encrypted_content": "encrypted_without_id",
                        "summary": [
                            {
                                "type": "summary_text",
                                "text": "Reasoning without item id"
                            }
                        ]
                    },
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "Visible after reasoning"
                            }
                        ]
                    }
                ],
                "store": false
            }))
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_compaction_history_with_store_false() {
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
                        "id": "resp_compaction_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Compaction accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 5,
                            "output_tokens": 2
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
        let compaction_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "compaction_001",
                "encryptedContent": "encrypted_compaction"
            }
        }))
        .expect("provider options deserialize");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Visible before compaction",
                        )),
                        LanguageModelAssistantContentPart::Custom(
                            LanguageModelCustomPart::new("openai.compaction")
                                .with_provider_options(compaction_options),
                        ),
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Visible after compaction",
                        )),
                    ]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "Visible before compaction"
                            }
                        ]
                    },
                    {
                        "type": "compaction",
                        "id": "compaction_001",
                        "encrypted_content": "encrypted_compaction"
                    },
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "Visible after compaction"
                            }
                        ]
                    }
                ],
                "store": false
            }))
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_text_item_id_and_phase_with_store_false() {
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
                        "id": "resp_text_phase",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Text history accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 4,
                            "output_tokens": 2
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
        let text_options = |item_id: &str, phase: Option<&str>| -> ProviderOptions {
            let mut openai = JsonObject::new();
            openai.insert("itemId".to_string(), JsonValue::String(item_id.to_string()));
            if let Some(phase) = phase {
                openai.insert("phase".to_string(), JsonValue::String(phase.to_string()));
            }

            let mut options = ProviderOptions::new();
            options.insert("openai".to_string(), openai);
            options
        };
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Text(
                            LanguageModelTextPart::new("I will search for that")
                                .with_provider_options(text_options("msg_001", Some("commentary"))),
                        ),
                        LanguageModelAssistantContentPart::Text(
                            LanguageModelTextPart::new("The capital of France is Paris.")
                                .with_provider_options(text_options(
                                    "msg_002",
                                    Some("final_answer"),
                                )),
                        ),
                        LanguageModelAssistantContentPart::Text(
                            LanguageModelTextPart::new("No phase")
                                .with_provider_options(text_options("msg_003", None)),
                        ),
                    ]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "I will search for that"
                            }
                        ],
                        "id": "msg_001",
                        "phase": "commentary"
                    },
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "The capital of France is Paris."
                            }
                        ],
                        "id": "msg_002",
                        "phase": "final_answer"
                    },
                    {
                        "type": "message",
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "No phase"
                            }
                        ],
                        "id": "msg_003"
                    }
                ],
                "store": false
            }))
        );
    }

    #[test]
    fn open_responses_provider_warns_for_unstored_reasoning_without_encrypted_content() {
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
                        "id": "resp_reasoning_warning",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Reasoning warning accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 1,
                            "output_tokens": 2
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "reasoning_without_encryption"
            }
        }))
        .expect("provider options deserialize");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Reasoning(
                            LanguageModelReasoningPart::new("Reasoning without encrypted content")
                                .with_provider_options(item_options),
                        ),
                        LanguageModelAssistantContentPart::Reasoning(
                            LanguageModelReasoningPart::new("Reasoning without provider options"),
                        ),
                    ]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.warnings.len(), 2);
        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                crate::warning::Warning::Other { message }
                    if message == "Reasoning parts without encrypted content are not supported when store is false. Skipping reasoning parts."
            )
        }));
        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                crate::warning::Warning::Other { message }
                    if message.starts_with("Non-OpenAI reasoning parts are not supported.")
            )
        }));
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
                "input": [],
                "store": false
            }))
        );
    }

    #[test]
    fn open_responses_provider_skips_conversation_history_items() {
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
                        "id": "resp_conversation",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Conversation accepted"
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
        let item_options = |item_id: &str| -> ProviderOptions {
            serde_json::from_value(json!({
                "openai": {
                    "itemId": item_id
                }
            }))
            .expect("provider options deserialize")
        };
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "conversation": "conv_123",
                "previousResponseId": "resp_previous"
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![
                    LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                        LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
                    ])),
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Text(
                            LanguageModelTextPart::new("Stored text")
                                .with_provider_options(item_options("message_existing")),
                        ),
                        LanguageModelAssistantContentPart::Reasoning(
                            LanguageModelReasoningPart::new("Stored reasoning")
                                .with_provider_options(item_options("reasoning_existing")),
                        ),
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_weather",
                                "get_weather",
                                json!({
                                    "location": "San Francisco"
                                }),
                            )
                            .with_provider_options(item_options("call_existing")),
                        ),
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Fresh assistant text",
                        )),
                    ])),
                    LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_weather",
                            "get_weather",
                            LanguageModelToolResultOutput::json(json!({
                                "temp": 72
                            })),
                        )),
                    ])),
                ])
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.warnings.len(), 1);
        assert!(matches!(
            result.warnings.first(),
            Some(crate::warning::Warning::Unsupported { feature, details })
                if feature == "conversation"
                    && details.as_deref()
                        == Some("conversation and previousResponseId cannot be used together")
        ));

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

        assert_eq!(request_body["conversation"], "conv_123");
        assert_eq!(request_body["previous_response_id"], "resp_previous");
        assert_eq!(
            request_body["input"],
            json!([
                {
                    "type": "message",
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": "Hello"
                        }
                    ]
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Fresh assistant text"
                        }
                    ]
                },
                {
                    "type": "function_call_output",
                    "call_id": "call_weather",
                    "output": "{\"temp\":72}"
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_hosted_tool_search_history_with_store_false() {
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
                        "id": "resp_tool_search_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Tool search accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 9,
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
        let item_options = |item_id: &str| -> ProviderOptions {
            serde_json::from_value(json!({
                "openai": {
                    "itemId": item_id
                }
            }))
            .expect("provider options deserialize")
        };
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "tsc_hosted_123",
                                "tool_search",
                                JsonValue::String(
                                    json!({
                                        "arguments": {
                                            "paths": ["get_weather"]
                                        },
                                        "call_id": null
                                    })
                                    .to_string(),
                                ),
                            )
                            .with_provider_executed(true)
                            .with_provider_options(item_options("tsc_hosted_123")),
                        ),
                        LanguageModelAssistantContentPart::ToolResult(
                            LanguageModelToolResultPart::new(
                                "tsc_hosted_123",
                                "tool_search",
                                LanguageModelToolResultOutput::json(json!({
                                    "tools": [
                                        {
                                            "type": "function",
                                            "name": "get_weather",
                                            "defer_loading": true
                                        }
                                    ]
                                })),
                            )
                            .with_provider_options(item_options("tso_hosted_456")),
                        ),
                    ]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "tool_search_call",
                    "id": "tsc_hosted_123",
                    "execution": "server",
                    "call_id": null,
                    "status": "completed",
                    "arguments": {
                        "paths": ["get_weather"]
                    }
                },
                {
                    "type": "tool_search_output",
                    "id": "tso_hosted_456",
                    "execution": "server",
                    "call_id": null,
                    "status": "completed",
                    "tools": [
                        {
                            "type": "function",
                            "name": "get_weather",
                            "defer_loading": true
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_client_tool_search_output_with_store_false() {
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
                        "id": "resp_client_tool_search_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Client tool search accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 11,
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "tsc_client_1"
            }
        }))
        .expect("provider options deserialize");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_abc123",
                                "tool_search",
                                JsonValue::String(
                                    json!({
                                        "arguments": {
                                            "goal": "Find weather tools"
                                        },
                                        "call_id": "call_abc123"
                                    })
                                    .to_string(),
                                ),
                            )
                            .with_provider_options(item_options),
                        ),
                    ])),
                    LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_abc123",
                            "tool_search",
                            LanguageModelToolResultOutput::json(json!({
                                "tools": [
                                    {
                                        "type": "function",
                                        "name": "get_weather",
                                        "description": "Get weather",
                                        "defer_loading": true,
                                        "parameters": {
                                            "type": "object",
                                            "properties": {
                                                "location": {
                                                    "type": "string"
                                                }
                                            },
                                            "required": ["location"]
                                        }
                                    }
                                ]
                            })),
                        )),
                    ])),
                ])
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "tool_search_call",
                    "id": "tsc_client_1",
                    "execution": "client",
                    "call_id": "call_abc123",
                    "status": "completed",
                    "arguments": {
                        "goal": "Find weather tools"
                    }
                },
                {
                    "type": "tool_search_output",
                    "execution": "client",
                    "call_id": "call_abc123",
                    "status": "completed",
                    "tools": [
                        {
                            "type": "function",
                            "name": "get_weather",
                            "description": "Get weather",
                            "defer_loading": true,
                            "parameters": {
                                "type": "object",
                                "properties": {
                                    "location": {
                                        "type": "string"
                                    }
                                },
                                "required": ["location"]
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_warns_for_unstored_hosted_tool_results() {
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
                        "id": "resp_web_search_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hosted history accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 8,
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
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Let me search.",
                        )),
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "ws_123",
                                "web_search",
                                json!({
                                    "query": "Rust AI SDK"
                                }),
                            )
                            .with_provider_executed(true),
                        ),
                        LanguageModelAssistantContentPart::ToolResult(
                            LanguageModelToolResultPart::new(
                                "ws_123",
                                "web_search",
                                LanguageModelToolResultOutput::json(json!({
                                    "sources": [
                                        {
                                            "type": "url",
                                            "url": "https://example.test"
                                        }
                                    ]
                                })),
                            ),
                        ),
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Search complete.",
                        )),
                    ]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(
            result.warnings,
            vec![crate::warning::Warning::Other {
                message:
                    "Results for OpenAI tool web_search are not sent to the API when store is false"
                        .to_string()
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
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");

        assert_eq!(
            request_body["input"],
            json!([
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Let me search."
                        }
                    ]
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Search complete."
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_skips_assistant_execution_denied_tool_results() {
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
                        "id": "resp_denied_tool_results",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Denied results skipped"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 8,
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
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "I need approval before running the first tool.",
                        )),
                        LanguageModelAssistantContentPart::ToolResult(
                            LanguageModelToolResultPart::new(
                                "ws_denied_direct",
                                "web_search",
                                LanguageModelToolResultOutput::execution_denied()
                                    .with_reason("User denied the tool execution"),
                            ),
                        ),
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "The first tool was not run.",
                        )),
                        LanguageModelAssistantContentPart::ToolResult(
                            LanguageModelToolResultPart::new(
                                "ws_denied_json",
                                "web_search",
                                LanguageModelToolResultOutput::json(json!({
                                    "type": "execution-denied",
                                    "reason": "User denied the tool execution"
                                })),
                            ),
                        ),
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "The second tool was not run.",
                        )),
                    ]),
                )])
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "I need approval before running the first tool."
                        }
                    ]
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "The first tool was not run."
                        }
                    ]
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "The second tool was not run."
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_local_shell_history_with_store_false() {
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
                        "id": "resp_local_shell_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Local shell history accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 10,
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "local_shell_item_1"
            }
        }))
        .expect("provider options deserialize");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_local_shell_1",
                                "local_shell",
                                json!({
                                    "action": {
                                        "type": "exec",
                                        "command": ["ls"],
                                        "timeoutMs": 1000,
                                        "user": "builder",
                                        "workingDirectory": "/tmp/work",
                                        "env": {
                                            "RUST_LOG": "debug"
                                        }
                                    }
                                }),
                            )
                            .with_provider_options(item_options),
                        ),
                    ])),
                    LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_local_shell_1",
                            "local_shell",
                            LanguageModelToolResultOutput::json(json!({
                                "output": "example output"
                            })),
                        )),
                    ])),
                ])
                .with_tool(open_responses_test_local_shell_tool())
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "local_shell_call",
                    "call_id": "call_local_shell_1",
                    "id": "local_shell_item_1",
                    "action": {
                        "type": "exec",
                        "command": ["ls"],
                        "timeout_ms": 1000,
                        "user": "builder",
                        "working_directory": "/tmp/work",
                        "env": {
                            "RUST_LOG": "debug"
                        }
                    }
                },
                {
                    "type": "local_shell_call_output",
                    "call_id": "call_local_shell_1",
                    "output": "example output"
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_shell_history_with_store_false() {
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
                        "id": "resp_shell_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Shell history accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 12,
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "shell_item_1"
            }
        }))
        .expect("provider options deserialize");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_shell_1",
                                "shell",
                                json!({
                                    "action": {
                                        "commands": ["ls -la"],
                                        "timeoutMs": 1000,
                                        "maxOutputLength": 2000
                                    }
                                }),
                            )
                            .with_provider_options(item_options),
                        ),
                    ])),
                    LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_shell_1",
                            "shell",
                            LanguageModelToolResultOutput::json(json!({
                                "output": [
                                    {
                                        "stdout": "ok\n",
                                        "stderr": "",
                                        "outcome": {
                                            "type": "exit",
                                            "exitCode": 0
                                        }
                                    }
                                ]
                            })),
                        )),
                    ])),
                ])
                .with_tool(open_responses_test_shell_tool())
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "shell_call",
                    "call_id": "call_shell_1",
                    "id": "shell_item_1",
                    "status": "completed",
                    "action": {
                        "commands": ["ls -la"],
                        "timeout_ms": 1000,
                        "max_output_length": 2000
                    }
                },
                {
                    "type": "shell_call_output",
                    "call_id": "call_shell_1",
                    "output": [
                        {
                            "stdout": "ok\n",
                            "stderr": "",
                            "outcome": {
                                "type": "exit",
                                "exit_code": 0
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_stored_assistant_shell_outputs() {
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
                        "id": "resp_assistant_shell_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Stored shell history accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 10,
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "shell_output_item"
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolResult(
                            LanguageModelToolResultPart::new(
                                "call_shell_stored",
                                "shell",
                                LanguageModelToolResultOutput::json(json!({
                                    "output": [
                                        {
                                            "stdout": "",
                                            "stderr": "timed out",
                                            "outcome": {
                                                "type": "timeout"
                                            }
                                        }
                                    ]
                                })),
                            )
                            .with_provider_options(item_options),
                        ),
                    ]),
                )])
                .with_tool(open_responses_test_shell_tool()),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "shell_call_output",
                    "call_id": "call_shell_stored",
                    "output": [
                        {
                            "stdout": "",
                            "stderr": "timed out",
                            "outcome": {
                                "type": "timeout"
                            }
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_apply_patch_history_with_store_false() {
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
                        "id": "resp_apply_patch_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Apply patch history accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 11,
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "apply_patch_item_1"
            }
        }))
        .expect("provider options deserialize");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "store": false
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_apply_patch_1",
                                "apply_patch",
                                json!({
                                    "callId": "call_apply_patch_1",
                                    "operation": {
                                        "type": "create_file",
                                        "path": "index.html",
                                        "diff": "+<!doctype html>\n+<html></html>"
                                    }
                                }),
                            )
                            .with_provider_options(item_options),
                        ),
                    ])),
                    LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_apply_patch_1",
                            "apply_patch",
                            LanguageModelToolResultOutput::json(json!({
                                "status": "completed",
                                "output": "Created index.html"
                            })),
                        )),
                    ])),
                ])
                .with_tool(open_responses_test_apply_patch_tool())
                .with_provider_options(provider_options),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "apply_patch_call",
                    "call_id": "call_apply_patch_1",
                    "id": "apply_patch_item_1",
                    "status": "completed",
                    "operation": {
                        "type": "create_file",
                        "path": "index.html",
                        "diff": "+<!doctype html>\n+<html></html>"
                    }
                },
                {
                    "type": "apply_patch_call_output",
                    "call_id": "call_apply_patch_1",
                    "status": "completed",
                    "output": "Created index.html"
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_stored_apply_patch_outputs() {
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
                        "id": "resp_stored_apply_patch_history",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Stored apply patch history accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 9,
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "apply_patch_item_2"
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_apply_patch_2",
                                "apply_patch",
                                json!({
                                    "callId": "call_apply_patch_2",
                                    "operation": {
                                        "type": "delete_file",
                                        "path": "temp.txt"
                                    }
                                }),
                            )
                            .with_provider_options(item_options),
                        ),
                    ])),
                    LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_apply_patch_2",
                            "apply_patch",
                            LanguageModelToolResultOutput::json(json!({
                                "status": "incomplete",
                                "output": "Deletion denied"
                            })),
                        )),
                    ])),
                ])
                .with_tool(open_responses_test_apply_patch_tool()),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "item_reference",
                    "id": "apply_patch_item_2"
                },
                {
                    "type": "apply_patch_call_output",
                    "call_id": "call_apply_patch_2",
                    "status": "incomplete",
                    "output": "Deletion denied"
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_custom_tool_calls() {
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
                        "id": "resp_custom_tool_calls",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Custom tool calls accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 12,
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
        let item_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "itemId": "custom_tool_item_3"
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Assistant(
                    LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_custom_1",
                                "write_sql",
                                JsonValue::String("SELECT * FROM users".to_string()),
                            ),
                        ),
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_custom_2",
                                "write_sql",
                                json!({
                                    "query": "SELECT 1"
                                }),
                            ),
                        ),
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call_custom_3",
                                "write_sql",
                                JsonValue::String("SELECT stored".to_string()),
                            )
                            .with_provider_options(item_options),
                        ),
                    ]),
                )])
                .with_tool(open_responses_test_custom_tool()),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "custom_tool_call",
                    "call_id": "call_custom_1",
                    "name": "write_sql",
                    "input": "SELECT * FROM users"
                },
                {
                    "type": "custom_tool_call",
                    "call_id": "call_custom_2",
                    "name": "write_sql",
                    "input": "{\"query\":\"SELECT 1\"}"
                },
                {
                    "type": "item_reference",
                    "id": "custom_tool_item_3"
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_reconstructs_custom_tool_outputs() {
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
                        "id": "resp_custom_tool_outputs",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Custom tool outputs accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 15,
                            "output_tokens": 5
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

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::Tool(
                    LanguageModelToolMessage::new(vec![
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_custom_text",
                            "write_sql",
                            LanguageModelToolResultOutput::text("Query executed successfully."),
                        )),
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_custom_json",
                            "write_sql",
                            LanguageModelToolResultOutput::json(json!({
                                "rows": [1, 2]
                            })),
                        )),
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_custom_denied",
                            "write_sql",
                            LanguageModelToolResultOutput::execution_denied()
                                .with_reason("User denied the tool execution"),
                        )),
                        LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                            "call_custom_content",
                            "write_sql",
                            LanguageModelToolResultOutput::content(vec![
                                LanguageModelToolResultContentPart::Text(
                                    LanguageModelTextPart::new("Here is the file:"),
                                ),
                                LanguageModelToolResultContentPart::File(
                                    LanguageModelFilePart::new(
                                        FileData::Url {
                                            url: Url::parse("https://example.com/test.pdf")
                                                .expect("valid URL"),
                                        },
                                        "application/pdf",
                                    ),
                                ),
                            ]),
                        )),
                    ]),
                )])
                .with_tool(open_responses_test_custom_tool()),
            ),
        );

        assert!(result.warnings.is_empty());
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
            request_body["input"],
            json!([
                {
                    "type": "custom_tool_call_output",
                    "call_id": "call_custom_text",
                    "output": "Query executed successfully."
                },
                {
                    "type": "custom_tool_call_output",
                    "call_id": "call_custom_json",
                    "output": "{\"rows\":[1,2]}"
                },
                {
                    "type": "custom_tool_call_output",
                    "call_id": "call_custom_denied",
                    "output": "User denied the tool execution"
                },
                {
                    "type": "custom_tool_call_output",
                    "call_id": "call_custom_content",
                    "output": [
                        {
                            "type": "input_text",
                            "text": "Here is the file:"
                        },
                        {
                            "type": "input_file",
                            "file_url": "https://example.com/test.pdf"
                        }
                    ]
                }
            ])
        );
    }

    #[test]
    fn open_responses_provider_stringifies_assistant_function_call_arguments() {
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
                        "id": "resp_tool_args",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Arguments accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 5,
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

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                    "Checking tools",
                )),
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call_object",
                    "get_weather",
                    json!({
                        "location": "Brisbane"
                    }),
                )),
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call_string",
                    "get_weather",
                    JsonValue::String("{\"location\":\"Berlin\"}".to_string()),
                )),
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call_null",
                    "get_weather",
                    JsonValue::Null,
                )),
            ])),
        ])));

        assert!(result.warnings.is_empty());
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
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "Checking tools"
                            }
                        ]
                    },
                    {
                        "type": "function_call",
                        "call_id": "call_object",
                        "name": "get_weather",
                        "arguments": "{\"location\":\"Brisbane\"}"
                    },
                    {
                        "type": "function_call",
                        "call_id": "call_string",
                        "name": "get_weather",
                        "arguments": "{\"location\":\"Berlin\"}"
                    },
                    {
                        "type": "function_call",
                        "call_id": "call_null",
                        "name": "get_weather",
                        "arguments": "{}"
                    }
                ]
            }))
        );
    }

    #[test]
    fn open_responses_provider_maps_reasoning_effort_and_summary_options() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: OpenResponsesTransport =
            Arc::new(move |request| -> OpenResponsesTransportFuture {
                captured_requests_for_transport
                    .lock()
                    .expect("captured requests mutex is not poisoned")
                    .push(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_reasoning",
                        "created_at": 1711115037,
                        "model": "gemma-7b-it",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Reasoning accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 5,
                            "output_tokens": 3
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = create_open_responses(
            OpenResponsesProviderSettings::new(
                "lmstudio",
                "https://api.lmstudio.test/v1/responses",
            )
            .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let model = provider.language_model("gemma-7b-it");
        let prompt = || {
            vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
                vec![LanguageModelUserContentPart::Text(
                    LanguageModelTextPart::new("Hello"),
                )],
            ))]
        };
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "lmstudio": {
                "reasoningSummary": "auto",
                "store": false,
                "metadata": {
                    "trace": "ignored"
                }
            }
        }))
        .expect("provider options deserialize");

        let minimal_result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(prompt())
                    .with_reasoning(LanguageModelReasoningEffort::Minimal)
                    .with_provider_options(provider_options),
            ),
        );
        let none_result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(prompt())
                    .with_reasoning(LanguageModelReasoningEffort::None),
            ),
        );
        let default_result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(prompt())
                    .with_reasoning(LanguageModelReasoningEffort::ProviderDefault),
            ),
        );

        assert_eq!(minimal_result.warnings.len(), 1);
        assert!(matches!(
            minimal_result.warnings.first(),
            Some(crate::warning::Warning::Compatibility { feature, details })
                if feature == "reasoning"
                    && details.as_deref() == Some(
                        "reasoning \"minimal\" is not directly supported by this model. mapped to effort \"low\"."
                    )
        ));
        assert!(none_result.warnings.is_empty());
        assert!(default_result.warnings.is_empty());

        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned");
        assert_eq!(requests.len(), 3);
        let bodies = requests
            .iter()
            .map(|request| {
                request
                    .body
                    .as_ref()
                    .and_then(ProviderApiRequestBody::as_text)
                    .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
                    .expect("request body is JSON")
            })
            .collect::<Vec<_>>();

        assert_eq!(
            bodies[0]["reasoning"],
            json!({
                "effort": "low",
                "summary": "auto"
            })
        );
        assert!(bodies[0].get("reasoningSummary").is_none());
        assert!(bodies[0].get("store").is_none());
        assert!(bodies[0].get("metadata").is_none());
        assert_eq!(
            bodies[1]["reasoning"],
            json!({
                "effort": "none"
            })
        );
        assert!(bodies[2].get("reasoning").is_none());
    }

    #[test]
    fn open_responses_provider_maps_openai_responses_provider_options_to_request_body() {
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
                        "id": "resp_openai_options",
                        "created_at": 1711115037,
                        "model": "gpt-5.1",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "{\"answer\":\"mapped\"}"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 6,
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
        let model = provider.language_model("gpt-5.1");
        let response_schema: JsonObject = serde_json::from_value(json!({
            "type": "object",
            "properties": {
                "answer": {
                    "type": "string"
                }
            },
            "required": ["answer"]
        }))
        .expect("schema deserializes");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "previousResponseId": "resp_prev",
                "maxToolCalls": 3,
                "parallelToolCalls": false,
                "promptCacheKey": "cache-key",
                "promptCacheRetention": "24h",
                "safetyIdentifier": "safe-user",
                "serviceTier": "priority",
                "textVerbosity": "low",
                "strictJsonSchema": false,
                "reasoningEffort": "high",
                "reasoningSummary": "detailed",
                "contextManagement": [
                    {
                        "type": "compaction",
                        "compactThreshold": 2048
                    }
                ],
                "logprobs": true,
                "passThroughUnsupportedFiles": true,
                "systemMessageMode": "developer",
                "forceReasoning": true,
                "caching": "auto"
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
                .with_response_format(
                    LanguageModelResponseFormat::json()
                        .with_schema(response_schema.clone())
                        .with_name("response"),
                )
                .with_provider_options(provider_options)
                .with_reasoning(LanguageModelReasoningEffort::Minimal),
            ),
        );

        assert!(result.warnings.is_empty());

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

        assert_eq!(request_body["previous_response_id"], "resp_prev");
        assert_eq!(request_body["max_tool_calls"], 3);
        assert_eq!(request_body["parallel_tool_calls"], false);
        assert_eq!(request_body["prompt_cache_key"], "cache-key");
        assert_eq!(request_body["prompt_cache_retention"], "24h");
        assert_eq!(request_body["safety_identifier"], "safe-user");
        assert_eq!(request_body["service_tier"], "priority");
        assert_eq!(
            request_body["context_management"],
            json!([
                {
                    "type": "compaction",
                    "compact_threshold": 2048
                }
            ])
        );
        assert_eq!(request_body["top_logprobs"], 20);
        assert_eq!(
            request_body["include"],
            json!(["message.output_text.logprobs"])
        );
        assert_eq!(
            request_body["text"],
            json!({
                "format": {
                    "type": "json_schema",
                    "name": "response",
                    "schema": response_schema,
                    "strict": false
                },
                "verbosity": "low"
            })
        );
        assert_eq!(
            request_body["reasoning"],
            json!({
                "effort": "high",
                "summary": "detailed"
            })
        );
        assert_eq!(request_body["caching"], "auto");

        for leaked_key in [
            "previousResponseId",
            "maxToolCalls",
            "parallelToolCalls",
            "promptCacheKey",
            "promptCacheRetention",
            "safetyIdentifier",
            "serviceTier",
            "textVerbosity",
            "strictJsonSchema",
            "reasoningEffort",
            "reasoningSummary",
            "contextManagement",
            "logprobs",
            "passThroughUnsupportedFiles",
            "systemMessageMode",
            "forceReasoning",
        ] {
            assert!(
                request_body.get(leaked_key).is_none(),
                "{leaked_key} should not leak into the Open Responses request body"
            );
        }
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
    fn open_responses_provider_converts_tool_result_file_content_outputs() {
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
                        "id": "resp_tool_files",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Tool output accepted"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 7,
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
        let image_data_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "imageDetail": "original"
            }
        }))
        .expect("provider options deserialize");
        let image_url_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "imageDetail": "high"
            }
        }))
        .expect("provider options deserialize");

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call_files",
                    "render_report",
                    LanguageModelToolResultOutput::content(vec![
                        LanguageModelToolResultContentPart::Text(LanguageModelTextPart::new(
                            "First result",
                        )),
                        LanguageModelToolResultContentPart::File(LanguageModelFilePart::new(
                            FileData::Data {
                                data: FileDataContent::Bytes(vec![0, 1, 2, 3]),
                            },
                            "image/png",
                        )
                        .with_provider_options(image_data_options)),
                        LanguageModelToolResultContentPart::File(
                            LanguageModelFilePart::new(
                                FileData::Url {
                                    url: Url::parse("https://example.com/photo.jpg")
                                        .expect("url parses"),
                                },
                                "image/jpeg",
                            )
                            .with_provider_options(image_url_options),
                        ),
                        LanguageModelToolResultContentPart::File(
                            LanguageModelFilePart::new(
                                FileData::Data {
                                    data: FileDataContent::Base64("JVBERi0=".to_string()),
                                },
                                "application/pdf",
                            )
                            .with_filename("report.pdf"),
                        ),
                        LanguageModelToolResultContentPart::File(LanguageModelFilePart::new(
                            FileData::Url {
                                url: Url::parse("https://example.com/report.pdf")
                                    .expect("url parses"),
                            },
                            "application/pdf",
                        )),
                    ]),
                )),
            ])),
        ])));

        assert!(result.warnings.is_empty());
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
                        "type": "function_call_output",
                        "call_id": "call_files",
                        "output": [
                            {
                                "type": "input_text",
                                "text": "First result"
                            },
                            {
                                "type": "input_image",
                                "image_url": "data:image/png;base64,AAECAw==",
                                "detail": "original"
                            },
                            {
                                "type": "input_image",
                                "image_url": "https://example.com/photo.jpg",
                                "detail": "high"
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
    fn open_responses_provider_resolves_top_level_image_media_types() {
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
                        "id": "resp_top_level_images",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Top-level images accepted"
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
        let png_base64 = "iVBORw0KGgo=";

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64(png_base64.to_string()),
                    },
                    "image/png",
                )),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64(png_base64.to_string()),
                    },
                    "image",
                )),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Url {
                        url: Url::parse("https://example.com/x.png").expect("url parses"),
                    },
                    "image",
                )),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64(png_base64.to_string()),
                    },
                    "image/*",
                )),
            ])),
        ])));

        assert!(result.warnings.is_empty());
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
                                "type": "input_image",
                                "image_url": "data:image/png;base64,iVBORw0KGgo="
                            },
                            {
                                "type": "input_image",
                                "image_url": "data:image/png;base64,iVBORw0KGgo="
                            },
                            {
                                "type": "input_image",
                                "image_url": "https://example.com/x.png"
                            },
                            {
                                "type": "input_image",
                                "image_url": "data:image/png;base64,iVBORw0KGgo="
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
    fn open_responses_provider_maps_additional_response_tool_items() {
        let transport: OpenResponsesTransport =
            Arc::new(move |_request| -> OpenResponsesTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_additional_tool_items",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "id": "custom_item",
                                "type": "custom_tool_call",
                                "call_id": "custom_1",
                                "name": "write_sql",
                                "input": "select 1"
                            },
                            {
                                "id": "tsc_1",
                                "type": "tool_search_call",
                                "execution": "server",
                                "call_id": null,
                                "status": "completed",
                                "arguments": {
                                    "goal": "Find a weather tool"
                                }
                            },
                            {
                                "id": "tso_1",
                                "type": "tool_search_output",
                                "execution": "server",
                                "call_id": null,
                                "status": "completed",
                                "tools": [
                                    {
                                        "type": "function",
                                        "name": "get_weather"
                                    }
                                ]
                            },
                            {
                                "id": "local_shell_item",
                                "type": "local_shell_call",
                                "call_id": "local_shell_1",
                                "action": {
                                    "type": "exec",
                                    "command": ["pwd"]
                                }
                            },
                            {
                                "id": "shell_item",
                                "type": "shell_call",
                                "call_id": "shell_1",
                                "status": "completed",
                                "action": {
                                    "commands": ["echo hi"]
                                }
                            },
                            {
                                "id": "shell_output_item",
                                "type": "shell_call_output",
                                "call_id": "shell_1",
                                "status": "completed",
                                "output": [
                                    {
                                        "stdout": "hi",
                                        "stderr": "",
                                        "outcome": {
                                            "type": "exit",
                                            "exit_code": 0
                                        }
                                    },
                                    {
                                        "stdout": "",
                                        "stderr": "timed out",
                                        "outcome": {
                                            "type": "timeout"
                                        }
                                    }
                                ]
                            },
                            {
                                "id": "patch_item",
                                "type": "apply_patch_call",
                                "call_id": "patch_1",
                                "status": "completed",
                                "operation": {
                                    "type": "update_file",
                                    "path": "src/lib.rs",
                                    "diff": "@@"
                                }
                            },
                            {
                                "id": "mcp_1",
                                "type": "mcp_call",
                                "status": "completed",
                                "arguments": "{\"query\":\"rust\"}",
                                "name": "lookup",
                                "server_label": "docs",
                                "output": "{\"answer\":\"ok\"}"
                            },
                            {
                                "id": "mcp_pending_1",
                                "type": "mcp_approval_request",
                                "server_label": "deployments",
                                "name": "deploy",
                                "arguments": "{\"target\":\"prod\"}",
                                "approval_request_id": "approval_1"
                            },
                            {
                                "id": "computer_1",
                                "type": "computer_call",
                                "status": "completed"
                            },
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Additional tools mapped"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 13,
                            "output_tokens": 8
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

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Use additional tools"),
                    )]),
                )])
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.tool_search",
                    "toolSearch",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.local_shell",
                    "localShell",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.shell",
                    "hostShell",
                    json_object(json!({
                        "environment": {
                            "type": "containerAuto"
                        }
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.apply_patch",
                    "patchTool",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(
                    LanguageModelProviderTool::new("openai.mcp", "mcpTool", JsonObject::new()),
                )),
            ),
        );

        assert_eq!(&result.finish_reason.unified, &FinishReason::ToolCalls);

        let tool_calls = result
            .content
            .iter()
            .filter_map(|part| match part {
                LanguageModelContent::ToolCall(tool_call) => Some(tool_call),
                _ => None,
            })
            .collect::<Vec<_>>();
        let tool_results = result
            .content
            .iter()
            .filter_map(|part| match part {
                LanguageModelContent::ToolResult(tool_result) => Some(tool_result),
                _ => None,
            })
            .collect::<Vec<_>>();
        let approvals = result
            .content
            .iter()
            .filter_map(|part| match part {
                LanguageModelContent::ToolApprovalRequest(approval) => Some(approval),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(tool_calls.len(), 8);
        assert_eq!(tool_results.len(), 4);
        assert_eq!(approvals.len(), 1);

        assert_eq!(tool_calls[0].tool_name, "write_sql");
        assert_eq!(
            serde_json::from_str::<JsonValue>(&tool_calls[0].input)
                .expect("custom tool input parses"),
            json!("select 1")
        );
        assert_eq!(tool_calls[1].tool_name, "toolSearch");
        assert_eq!(tool_calls[1].tool_call_id, "tsc_1");
        assert_eq!(tool_calls[1].provider_executed, Some(true));
        assert_eq!(
            serde_json::from_str::<JsonValue>(&tool_calls[1].input)
                .expect("tool search input parses"),
            json!({
                "arguments": {
                    "goal": "Find a weather tool"
                },
                "call_id": null
            })
        );
        assert_eq!(tool_results[0].tool_call_id, "tsc_1");
        assert_eq!(tool_results[0].tool_name, "toolSearch");
        assert_eq!(
            tool_results[0].result.as_value(),
            &json!({
                "tools": [
                    {
                        "type": "function",
                        "name": "get_weather"
                    }
                ]
            })
        );

        assert_eq!(tool_calls[2].tool_name, "localShell");
        assert_eq!(
            serde_json::from_str::<JsonValue>(&tool_calls[2].input)
                .expect("local shell input parses"),
            json!({
                "action": {
                    "type": "exec",
                    "command": ["pwd"]
                }
            })
        );
        assert_eq!(tool_calls[3].tool_name, "hostShell");
        assert_eq!(tool_calls[3].provider_executed, Some(true));
        assert_eq!(
            serde_json::from_str::<JsonValue>(&tool_calls[3].input).expect("shell input parses"),
            json!({
                "action": {
                    "commands": ["echo hi"]
                }
            })
        );
        assert_eq!(tool_results[1].tool_name, "hostShell");
        assert_eq!(
            tool_results[1].result.as_value(),
            &json!({
                "output": [
                    {
                        "stdout": "hi",
                        "stderr": "",
                        "outcome": {
                            "type": "exit",
                            "exitCode": 0
                        }
                    },
                    {
                        "stdout": "",
                        "stderr": "timed out",
                        "outcome": {
                            "type": "timeout"
                        }
                    }
                ]
            })
        );

        assert_eq!(tool_calls[4].tool_name, "patchTool");
        assert_eq!(
            serde_json::from_str::<JsonValue>(&tool_calls[4].input)
                .expect("apply patch input parses"),
            json!({
                "callId": "patch_1",
                "operation": {
                    "type": "update_file",
                    "path": "src/lib.rs",
                    "diff": "@@"
                }
            })
        );
        assert_eq!(tool_calls[5].tool_name, "mcp.lookup");
        assert_eq!(tool_calls[5].provider_executed, Some(true));
        assert_eq!(tool_calls[5].dynamic, Some(true));
        assert_eq!(
            serde_json::from_str::<JsonValue>(&tool_calls[5].input).expect("mcp input parses"),
            json!({
                "query": "rust"
            })
        );
        assert_eq!(tool_results[2].tool_name, "mcp.lookup");
        assert_eq!(tool_results[2].dynamic, Some(true));
        assert_eq!(
            tool_results[2].result.as_value(),
            &json!({
                "type": "call",
                "serverLabel": "docs",
                "name": "lookup",
                "arguments": "{\"query\":\"rust\"}",
                "output": "{\"answer\":\"ok\"}"
            })
        );

        assert_eq!(tool_calls[6].tool_name, "mcp.deploy");
        assert_eq!(tool_calls[6].provider_executed, Some(true));
        assert_eq!(tool_calls[6].dynamic, Some(true));
        assert_ne!(tool_calls[6].tool_call_id, "mcp_pending_1");
        assert_eq!(
            openai_metadata_value(&tool_calls[6].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("mcp_pending_1")
        );
        assert_eq!(
            openai_metadata_value(&tool_calls[6].provider_metadata, "approvalRequestId")
                .and_then(JsonValue::as_str),
            Some("approval_1")
        );
        assert_eq!(approvals[0].approval_id, "approval_1");
        assert_eq!(approvals[0].tool_call_id, tool_calls[6].tool_call_id);
        assert_eq!(tool_calls[7].tool_name, "computer_use");
        assert_eq!(tool_calls[7].input, "");
        assert_eq!(tool_calls[7].provider_executed, Some(true));
        assert_eq!(
            tool_results[3].result.as_value(),
            &json!({
                "type": "computer_use_tool_result",
                "status": "completed"
            })
        );
    }

    #[test]
    fn open_responses_provider_maps_text_sources_and_compaction_metadata() {
        let transport: OpenResponsesTransport =
            Arc::new(move |_request| -> OpenResponsesTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "id": "resp_metadata_items",
                        "created_at": 1711115037,
                        "model": "gpt-4.1-mini",
                        "output": [
                            {
                                "id": "reasoning_1",
                                "type": "reasoning",
                                "encrypted_content": "encrypted-reasoning",
                                "summary": []
                            },
                            {
                                "id": "message_1",
                                "type": "message",
                                "phase": "final",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Cited answer",
                                        "annotations": [
                                            {
                                                "type": "url_citation",
                                                "url": "https://example.com/article",
                                                "title": "Example Article"
                                            },
                                            {
                                                "type": "file_citation",
                                                "file_id": "file_123",
                                                "filename": "guide.md",
                                                "index": 7
                                            },
                                            {
                                                "type": "container_file_citation",
                                                "container_id": "container_123",
                                                "file_id": "cfile_123",
                                                "filename": "results.csv"
                                            },
                                            {
                                                "type": "file_path",
                                                "file_id": "path_file_123",
                                                "index": 3
                                            }
                                        ]
                                    }
                                ]
                            },
                            {
                                "id": "compaction_1",
                                "type": "compaction",
                                "encrypted_content": "encrypted-context"
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

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Use sources")),
            ])),
        ])));

        assert_eq!(&result.finish_reason.unified, &FinishReason::Stop);
        assert!(matches!(
            &result.content[0],
            LanguageModelContent::Reasoning(reasoning)
                if reasoning.text.is_empty()
                    && reasoning
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("itemId"))
                        .and_then(JsonValue::as_str)
                        == Some("reasoning_1")
                    && reasoning
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("reasoningEncryptedContent"))
                        .and_then(JsonValue::as_str)
                        == Some("encrypted-reasoning")
        ));
        assert!(matches!(
            &result.content[1],
            LanguageModelContent::Text(text)
                if text.text == "Cited answer"
                    && text
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("itemId"))
                        .and_then(JsonValue::as_str)
                        == Some("message_1")
                    && text
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("phase"))
                        .and_then(JsonValue::as_str)
                        == Some("final")
                    && text
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("annotations"))
                        .and_then(JsonValue::as_array)
                        .is_some_and(|annotations| annotations.len() == 4)
        ));

        let sources = result
            .content
            .iter()
            .filter_map(|part| match part {
                LanguageModelContent::Source(source) => Some(source),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(sources.len(), 4);
        assert!(matches!(
            sources[0],
            LanguageModelSource::Url(source)
                if source.id == "source-0"
                    && source.url == "https://example.com/article"
                    && source.title.as_deref() == Some("Example Article")
        ));
        assert!(matches!(
            sources[1],
            LanguageModelSource::Document(source)
                if source.id == "source-1"
                    && source.media_type == "text/plain"
                    && source.title == "guide.md"
                    && source.filename.as_deref() == Some("guide.md")
                    && source
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("fileId"))
                        .and_then(JsonValue::as_str)
                        == Some("file_123")
        ));
        assert!(matches!(
            sources[2],
            LanguageModelSource::Document(source)
                if source.id == "source-2"
                    && source.media_type == "text/plain"
                    && source.title == "results.csv"
                    && source
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("containerId"))
                        .and_then(JsonValue::as_str)
                        == Some("container_123")
        ));
        assert!(matches!(
            sources[3],
            LanguageModelSource::Document(source)
                if source.id == "source-3"
                    && source.media_type == "application/octet-stream"
                    && source.title == "path_file_123"
                    && source.filename.as_deref() == Some("path_file_123")
        ));
        assert!(matches!(
            result.content.last(),
            Some(LanguageModelContent::Custom(custom))
                if custom.kind == "openai.compaction"
                    && custom
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("type"))
                        .and_then(JsonValue::as_str)
                        == Some("compaction")
                    && custom
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("encryptedContent"))
                        .and_then(JsonValue::as_str)
                        == Some("encrypted-context")
        ));
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
        assert!(result.provider_metadata.is_none());
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
        assert_eq!(result.response.id.as_deref(), Some("resp_stream_error"));
        assert!(result.provider_metadata.is_none());
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
    fn open_responses_provider_stream_failed_response_sets_raw_reason_and_usage() {
        let transport: OpenResponsesTransport = Arc::new(
            move |_request| -> OpenResponsesTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream_failed","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"response.failed","response":{"id":"resp_stream_failed","created_at":1711115037,"model":"gpt-4.1-mini","status":"failed","error":{"type":"rate_limit_error","code":"rate_limit_exceeded","message":"rate limited","param":null},"usage":{"input_tokens":6,"input_tokens_details":{"cached_tokens":2},"output_tokens":4,"output_tokens_details":{"reasoning_tokens":1}}}}"#,
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
        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Say hello")),
            ])),
        ])));

        assert!(
            !result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::Error(_)))
        );
        let finish = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => Some(finish),
                _ => None,
            })
            .expect("stream includes finish part");
        assert_eq!(finish.finish_reason.unified, FinishReason::Error);
        assert_eq!(
            finish.finish_reason.raw.as_deref(),
            Some("rate_limit_exceeded")
        );
        assert_eq!(finish.usage.input_tokens.total, Some(6));
        assert_eq!(finish.usage.input_tokens.no_cache, Some(4));
        assert_eq!(finish.usage.input_tokens.cache_read, Some(2));
        assert_eq!(finish.usage.output_tokens.total, Some(4));
        assert_eq!(finish.usage.output_tokens.text, Some(3));
        assert_eq!(finish.usage.output_tokens.reasoning, Some(1));
    }

    #[test]
    fn open_responses_provider_streams_text_sources_reasoning_and_compaction_metadata() {
        let transport: OpenResponsesTransport = Arc::new(
            move |_request| -> OpenResponsesTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream_metadata","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"message_1","type":"message","phase":"final_answer","role":"assistant","content":[]}}"#,
                    "",
                    r#"data: {"type":"response.output_text.delta","item_id":"message_1","output_index":0,"content_index":0,"delta":"Cited answer"}"#,
                    "",
                    r#"data: {"type":"response.output_text.done","item_id":"message_1","output_index":0,"content_index":0,"text":"Cited answer"}"#,
                    "",
                    r#"data: {"type":"response.output_text.annotation.added","item_id":"message_1","output_index":0,"content_index":0,"annotation_index":0,"annotation":{"type":"url_citation","url":"https://example.com/article","title":"Example Article"}}"#,
                    "",
                    r#"data: {"type":"response.output_text.annotation.added","item_id":"message_1","output_index":0,"content_index":0,"annotation_index":1,"annotation":{"type":"file_citation","file_id":"file_123","filename":"guide.md","index":7}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"message_1","type":"message","phase":"final_answer","role":"assistant","content":[{"type":"output_text","text":"Cited answer"}]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":1,"item":{"id":"reasoning_1","type":"reasoning","encrypted_content":"encrypted-reasoning","summary":[]}}"#,
                    "",
                    r#"data: {"type":"response.reasoning_summary_text.delta","item_id":"reasoning_1","output_index":1,"summary_index":0,"delta":"thinking"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":1,"item":{"id":"reasoning_1","type":"reasoning","encrypted_content":"encrypted-reasoning","summary":[{"type":"summary_text","text":"thinking"}]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":2,"item":{"id":"compaction_1","type":"compaction","encrypted_content":"encrypted-context"}}"#,
                    "",
                    r#"data: {"type":"response.completed","response":{"id":"resp_stream_metadata","created_at":1711115037,"model":"gpt-4.1-mini","usage":{"input_tokens":7,"output_tokens":5}}}"#,
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

        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Use sources")),
            ])),
        ])));

        let text_start = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::TextStart(text_start) => Some(text_start),
                _ => None,
            })
            .expect("stream includes text start");
        assert_eq!(text_start.id, "message_1");
        assert_eq!(
            text_start
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("openai"))
                .and_then(|metadata| metadata.get("phase"))
                .and_then(JsonValue::as_str),
            Some("final_answer")
        );

        let sources = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::Source(source) => Some(source),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(sources.len(), 2);
        assert!(matches!(
            sources[0],
            LanguageModelSource::Url(source)
                if source.id == "source-0"
                    && source.url == "https://example.com/article"
                    && source.title.as_deref() == Some("Example Article")
        ));
        assert!(matches!(
            sources[1],
            LanguageModelSource::Document(source)
                if source.id == "source-1"
                    && source.title == "guide.md"
                    && source.filename.as_deref() == Some("guide.md")
                    && source
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("fileId"))
                        .and_then(JsonValue::as_str)
                        == Some("file_123")
        ));

        let text_end = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::TextEnd(text_end) => Some(text_end),
                _ => None,
            })
            .expect("stream includes text end");
        assert_eq!(
            text_end
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("openai"))
                .and_then(|metadata| metadata.get("annotations"))
                .and_then(JsonValue::as_array)
                .map(Vec::len),
            Some(2)
        );

        let reasoning_start = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::ReasoningStart(reasoning_start) => Some(reasoning_start),
                _ => None,
            })
            .expect("stream includes reasoning start");
        assert_eq!(reasoning_start.id, "reasoning_1:0");
        assert_eq!(
            reasoning_start
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("openai"))
                .and_then(|metadata| metadata.get("reasoningEncryptedContent"))
                .and_then(JsonValue::as_str),
            Some("encrypted-reasoning")
        );
        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::ReasoningDelta(delta) if delta.id == "reasoning_1:0" && delta.delta == "thinking"))
        );
        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::ReasoningEnd(end) if end.id == "reasoning_1:0"
                    && end
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("reasoningEncryptedContent"))
                        .and_then(JsonValue::as_str)
                        == Some("encrypted-reasoning")))
        );

        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::Custom(custom) if custom.kind == "openai.compaction"
                    && custom
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("openai"))
                        .and_then(|metadata| metadata.get("encryptedContent"))
                        .and_then(JsonValue::as_str)
                        == Some("encrypted-context")))
        );
    }

    #[test]
    fn open_responses_provider_streams_hosted_tool_outputs() {
        let transport: OpenResponsesTransport = Arc::new(
            move |_request| -> OpenResponsesTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream_hosted_tools","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"ws_123","type":"web_search_call","status":"in_progress"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"ws_123","type":"web_search_call","status":"completed","action":{"type":"search","query":"AI SDK Rust","sources":[{"type":"url","url":"https://example.com"}]}}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":1,"item":{"id":"fs_123","type":"file_search_call","status":"in_progress"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":1,"item":{"id":"fs_123","type":"file_search_call","status":"completed","queries":["rust sdk"],"results":[{"attributes":{"kind":"docs"},"file_id":"file_123","filename":"guide.md","score":0.91,"text":"Guide text"}]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":2,"item":{"id":"ci_123","type":"code_interpreter_call","status":"in_progress","container_id":"container_123"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":2,"item":{"id":"ci_123","type":"code_interpreter_call","status":"completed","code":"print(1)","container_id":"container_123","outputs":[{"type":"logs","logs":"1"}]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":3,"item":{"id":"ig_123","type":"image_generation_call","status":"in_progress"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":3,"item":{"id":"ig_123","type":"image_generation_call","status":"completed","result":"base64-image"}}"#,
                    "",
                    r#"data: {"type":"response.output_text.delta","item_id":"message_1","output_index":4,"content_index":0,"delta":"Hosted tools streamed"}"#,
                    "",
                    r#"data: {"type":"response.output_text.done","item_id":"message_1","output_index":4,"content_index":0,"text":"Hosted tools streamed"}"#,
                    "",
                    r#"data: {"type":"response.completed","response":{"id":"resp_stream_hosted_tools","created_at":1711115037,"model":"gpt-4.1-mini","usage":{"input_tokens":11,"output_tokens":7}}}"#,
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

        let result =
            poll_ready(
                model.do_stream(
                    LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                        LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                            LanguageModelTextPart::new("Use hosted tools"),
                        )]),
                    )])
                    .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                        "openai.web_search",
                        "liveSearch",
                        JsonObject::new(),
                    )))
                    .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                        "openai.file_search",
                        "docSearch",
                        JsonObject::new(),
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
                ),
            );

        let tool_calls = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::ToolCall(tool_call) => Some(tool_call),
                _ => None,
            })
            .collect::<Vec<_>>();
        let tool_results = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::ToolResult(tool_result) => Some(tool_result),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(tool_calls.len(), 4);
        assert_eq!(tool_results.len(), 4);
        assert!(
            tool_calls
                .iter()
                .all(|tool_call| tool_call.provider_executed == Some(true))
        );
        assert_eq!(tool_calls[0].tool_name, "liveSearch");
        assert_eq!(tool_results[0].tool_name, "liveSearch");
        assert_eq!(
            tool_results[0].result.as_value(),
            &json!({
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
        assert_eq!(tool_calls[1].tool_name, "docSearch");
        assert_eq!(
            tool_results[1].result.as_value(),
            &json!({
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
        assert_eq!(tool_calls[2].tool_name, "codeRunner");
        assert_eq!(
            tool_calls[2].input,
            json!({
                "code": "print(1)",
                "containerId": "container_123"
            })
            .to_string()
        );
        assert_eq!(
            tool_results[2].result.as_value(),
            &json!({
                "outputs": [
                    {
                        "type": "logs",
                        "logs": "1"
                    }
                ]
            })
        );
        assert_eq!(tool_calls[3].tool_name, "imageMaker");
        assert_eq!(
            tool_results[3].result.as_value(),
            &json!({
                "result": "base64-image"
            })
        );
        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::ToolInputStart(start) if start.id == "ws_123" && start.provider_executed == Some(true)))
        );
        assert_eq!(
            result.stream.iter().find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => {
                    Some(finish.finish_reason.unified.clone())
                }
                _ => None,
            }),
            Some(FinishReason::Stop)
        );
    }

    #[test]
    fn open_responses_provider_streams_tool_input_delta_refinements() {
        let transport: OpenResponsesTransport = Arc::new(
            move |_request| -> OpenResponsesTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream_tool_deltas","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"custom_item","type":"custom_tool_call","call_id":"custom_1","name":"sqlWriter","input":""}}"#,
                    "",
                    r#"data: {"type":"response.custom_tool_call_input.delta","output_index":0,"delta":"select "}"#,
                    "",
                    r#"data: {"type":"response.custom_tool_call_input.delta","output_index":0,"delta":"1"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"custom_item","type":"custom_tool_call","call_id":"custom_1","name":"sqlWriter","input":"select 1"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":1,"item":{"id":"ci_123","type":"code_interpreter_call","status":"in_progress","container_id":"container_123"}}"#,
                    "",
                    r#"data: {"type":"response.code_interpreter_call_code.delta","output_index":1,"delta":"print("}"#,
                    "",
                    r#"data: {"type":"response.code_interpreter_call_code.delta","output_index":1,"delta":"1)\n"}"#,
                    "",
                    r#"data: {"type":"response.code_interpreter_call_code.done","output_index":1,"code":"print(1)\n"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":1,"item":{"id":"ci_123","type":"code_interpreter_call","status":"completed","code":"print(1)\n","container_id":"container_123","outputs":[{"type":"logs","logs":"1"}]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":2,"item":{"id":"ig_123","type":"image_generation_call","status":"in_progress"}}"#,
                    "",
                    r#"data: {"type":"response.image_generation_call.partial_image","output_index":2,"item_id":"ig_123","partial_image_b64":"partial-base64"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":2,"item":{"id":"ig_123","type":"image_generation_call","status":"completed","result":"final-base64"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":3,"item":{"id":"patch_1","type":"apply_patch_call","call_id":"patch_call_1","operation":{"type":"update_file","path":"README.md"}}}"#,
                    "",
                    r#"data: {"type":"response.apply_patch_call_operation_diff.delta","output_index":3,"delta":"@@\n-old\n+new\n"}"#,
                    "",
                    r#"data: {"type":"response.apply_patch_call_operation_diff.done","output_index":3,"diff":"@@\n-old\n+new\n"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":3,"item":{"id":"patch_1","type":"apply_patch_call","call_id":"patch_call_1","status":"completed","operation":{"type":"update_file","path":"README.md","diff":"@@\n-old\n+new\n"}}}"#,
                    "",
                    r#"data: {"type":"response.completed","response":{"id":"resp_stream_tool_deltas","created_at":1711115037,"model":"gpt-4.1-mini","usage":{"input_tokens":15,"output_tokens":9}}}"#,
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

        let result =
            poll_ready(
                model.do_stream(
                    LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                        LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                            LanguageModelTextPart::new("Use streaming tool deltas"),
                        )]),
                    )])
                    .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                        "openai.code_interpreter",
                        "codeRunner",
                        JsonObject::new(),
                    )))
                    .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                        "openai.image_generation",
                        "imageMaker",
                        JsonObject::new(),
                    )))
                    .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                        "openai.apply_patch",
                        "patchTool",
                        JsonObject::new(),
                    ))),
                ),
            );

        let input_deltas_for = |tool_call_id: &str| {
            result
                .stream
                .iter()
                .filter_map(|part| match part {
                    LanguageModelStreamPart::ToolInputDelta(delta) if delta.id == tool_call_id => {
                        Some(delta.delta.as_str())
                    }
                    _ => None,
                })
                .fold(String::new(), |mut input, delta| {
                    input.push_str(delta);
                    input
                })
        };
        let tool_call_by_id = |tool_call_id: &str| {
            result
                .stream
                .iter()
                .find_map(|part| match part {
                    LanguageModelStreamPart::ToolCall(tool_call)
                        if tool_call.tool_call_id == tool_call_id =>
                    {
                        Some(tool_call)
                    }
                    _ => None,
                })
                .expect("stream includes expected tool call")
        };

        assert_eq!(input_deltas_for("custom_1"), "select 1");
        assert_eq!(
            tool_call_by_id("custom_1").input,
            json!("select 1").to_string()
        );

        assert_eq!(
            input_deltas_for("ci_123"),
            r#"{"containerId":"container_123","code":"print(1)\n"}"#
        );
        assert_eq!(
            tool_call_by_id("ci_123").input,
            json!({
                "code": "print(1)\n",
                "containerId": "container_123"
            })
            .to_string()
        );
        assert_eq!(tool_call_by_id("ci_123").provider_executed, Some(true));

        let image_results = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::ToolResult(tool_result)
                    if tool_result.tool_call_id == "ig_123" =>
                {
                    Some(tool_result)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(image_results.len(), 2);
        assert_eq!(image_results[0].preliminary, Some(true));
        assert_eq!(
            image_results[0].result.as_value(),
            &json!({
                "result": "partial-base64"
            })
        );
        assert_eq!(image_results[1].preliminary, None);
        assert_eq!(
            image_results[1].result.as_value(),
            &json!({
                "result": "final-base64"
            })
        );

        assert_eq!(
            input_deltas_for("patch_call_1"),
            r#"{"callId":"patch_call_1","operation":{"type":"update_file","path":"README.md","diff":"@@\n-old\n+new\n"}}"#
        );
        assert_eq!(
            tool_call_by_id("patch_call_1").input,
            json!({
                "callId": "patch_call_1",
                "operation": {
                    "type": "update_file",
                    "path": "README.md",
                    "diff": "@@\n-old\n+new\n"
                }
            })
            .to_string()
        );
    }

    #[test]
    fn open_responses_provider_streams_additional_tool_items() {
        let transport: OpenResponsesTransport = Arc::new(
            move |_request| -> OpenResponsesTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream_extra_tools","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"custom_item","type":"custom_tool_call","call_id":"custom_1","name":"write_sql","input":"select 1"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":1,"item":{"id":"tsc_1","type":"tool_search_call","execution":"server","call_id":null,"status":"completed","arguments":{"goal":"Find a weather tool"}}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":2,"item":{"id":"tso_1","type":"tool_search_output","execution":"server","call_id":null,"status":"completed","tools":[{"type":"function","name":"get_weather"}]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":3,"item":{"id":"local_1","type":"local_shell_call","call_id":"local_call_1","action":{"type":"exec","command":"pwd","timeout_ms":1000}}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":4,"item":{"id":"shell_1","type":"shell_call","call_id":"shell_call_1","action":{"commands":["echo hi"]}}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":5,"item":{"id":"shell_out_1","type":"shell_call_output","call_id":"shell_call_1","output":[{"stdout":"hi\n","stderr":"","outcome":{"type":"exit","exit_code":0}}]}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":6,"item":{"id":"patch_1","type":"apply_patch_call","call_id":"patch_call_1","status":"completed","operation":{"type":"update_file","path":"README.md","diff":"@@\n"}}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":7,"item":{"id":"mcp_1","type":"mcp_call","server_label":"server","name":"lookup","arguments":"{\"query\":\"rust\"}","output":{"ok":true}}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":8,"item":{"id":"mcp_approval_1","type":"mcp_approval_request","approval_request_id":"approval_1","name":"approve","arguments":"{}"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":9,"item":{"id":"mcp_call_after_approval","type":"mcp_call","approval_request_id":"approval_1","server_label":"server","name":"approve","arguments":"{}","output":{"approved":true}}}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":10,"item":{"id":"computer_1","type":"computer_call","status":"completed"}}"#,
                    "",
                    r#"data: {"type":"response.completed","response":{"id":"resp_stream_extra_tools","created_at":1711115037,"model":"gpt-4.1-mini","usage":{"input_tokens":13,"output_tokens":8}}}"#,
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

        let result = poll_ready(
            model.do_stream(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Use additional tools"),
                    )]),
                )])
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.tool_search",
                    "toolSearch",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.local_shell",
                    "localShell",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.shell",
                    "hostShell",
                    json_object(json!({
                        "environment": {
                            "type": "containerAuto"
                        }
                    })),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.apply_patch",
                    "patchTool",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(
                    LanguageModelProviderTool::new("openai.mcp", "mcpTool", JsonObject::new()),
                )),
            ),
        );

        let tool_calls = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::ToolCall(tool_call) => Some(tool_call),
                _ => None,
            })
            .collect::<Vec<_>>();
        let tool_results = result
            .stream
            .iter()
            .filter_map(|part| match part {
                LanguageModelStreamPart::ToolResult(tool_result) => Some(tool_result),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(tool_calls.len(), 9);
        assert_eq!(tool_results.len(), 5);
        assert_eq!(tool_calls[0].tool_call_id, "custom_1");
        assert_eq!(tool_calls[0].tool_name, "write_sql");
        assert_eq!(tool_calls[0].input, "\"select 1\"");
        assert_eq!(
            openai_metadata_value(&tool_calls[0].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("custom_item")
        );
        assert_eq!(tool_calls[1].tool_name, "toolSearch");
        assert_eq!(tool_calls[1].provider_executed, Some(true));
        assert_eq!(
            openai_metadata_value(&tool_calls[1].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("tsc_1")
        );
        assert_eq!(tool_results[0].tool_call_id, "tsc_1");
        assert_eq!(
            openai_metadata_value(&tool_results[0].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("tso_1")
        );
        assert_eq!(
            tool_results[0].result.as_value(),
            &json!({
                "tools": [
                    {
                        "type": "function",
                        "name": "get_weather"
                    }
                ]
            })
        );
        assert_eq!(tool_calls[2].tool_name, "localShell");
        assert_eq!(
            openai_metadata_value(&tool_calls[2].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("local_1")
        );
        assert_eq!(tool_calls[3].tool_name, "hostShell");
        assert_eq!(tool_calls[3].provider_executed, Some(true));
        assert_eq!(
            openai_metadata_value(&tool_calls[3].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("shell_1")
        );
        assert_eq!(
            tool_results[1].result.as_value(),
            &json!({
                "output": [
                    {
                        "stdout": "hi\n",
                        "stderr": "",
                        "outcome": {
                            "type": "exit",
                            "exitCode": 0
                        }
                    }
                ]
            })
        );
        assert_eq!(tool_calls[4].tool_name, "patchTool");
        assert_eq!(
            openai_metadata_value(&tool_calls[4].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("patch_1")
        );
        assert_eq!(tool_calls[5].tool_name, "mcp.lookup");
        assert_eq!(tool_calls[5].provider_executed, Some(true));
        assert_eq!(tool_calls[5].dynamic, Some(true));
        assert_eq!(tool_results[2].tool_name, "mcp.lookup");
        assert_eq!(tool_results[2].dynamic, Some(true));
        assert_eq!(
            openai_metadata_value(&tool_results[2].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("mcp_1")
        );
        assert_eq!(tool_calls[6].tool_name, "mcp.approve");
        assert_ne!(tool_calls[6].tool_call_id, "mcp_approval_1");
        assert_eq!(
            openai_metadata_value(&tool_calls[6].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("mcp_approval_1")
        );
        assert_eq!(
            openai_metadata_value(&tool_calls[6].provider_metadata, "approvalRequestId")
                .and_then(JsonValue::as_str),
            Some("approval_1")
        );
        let approval_tool_call_id = tool_calls[6].tool_call_id.clone();
        assert!(
            result
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::ToolApprovalRequest(approval) if approval.approval_id == "approval_1" && approval.tool_call_id == approval_tool_call_id.as_str()))
        );
        assert_eq!(tool_calls[7].tool_name, "mcp.approve");
        assert_eq!(tool_calls[7].tool_call_id, approval_tool_call_id);
        assert_eq!(tool_results[3].tool_call_id, tool_calls[7].tool_call_id);
        assert_eq!(
            openai_metadata_value(&tool_results[3].provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("mcp_call_after_approval")
        );
        assert_eq!(tool_calls[8].tool_name, "computer_use");
        assert_eq!(
            tool_results[4].result.as_value(),
            &json!({
                "type": "computer_use_tool_result",
                "status": "completed"
            })
        );
        assert_eq!(
            result.stream.iter().find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => {
                    Some(finish.finish_reason.unified.clone())
                }
                _ => None,
            }),
            Some(FinishReason::ToolCalls)
        );
    }

    #[test]
    fn open_responses_streams_function_call_argument_deltas() {
        let transport: OpenResponsesTransport = Arc::new(
            move |_request| -> OpenResponsesTransportFuture {
                let sse = [
                    r#"data: {"type":"response.created","response":{"id":"resp_stream_tool","created_at":1711115037,"model":"gpt-4.1-mini"}}"#,
                    "",
                    r#"data: {"type":"response.output_item.added","output_index":0,"item":{"id":"fc_1","type":"function_call","call_id":"call_weather","name":"weather","arguments":"","namespace":"weather_ns"}}"#,
                    "",
                    r#"data: {"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":0,"delta":"{\"location\""}"#,
                    "",
                    r#"data: {"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":0,"delta":":\"Brisbane\"}"}"#,
                    "",
                    r#"data: {"type":"response.function_call_arguments.done","item_id":"fc_1","output_index":0,"arguments":"{\"location\":\"Brisbane\"}"}"#,
                    "",
                    r#"data: {"type":"response.output_item.done","output_index":0,"item":{"id":"fc_1","type":"function_call","call_id":"call_weather","name":"weather","arguments":"","namespace":"weather_ns"}}"#,
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
            openai_metadata_value(&tool_call.provider_metadata, "itemId")
                .and_then(JsonValue::as_str),
            Some("fc_1")
        );
        assert_eq!(
            openai_metadata_value(&tool_call.provider_metadata, "namespace")
                .and_then(JsonValue::as_str),
            Some("weather_ns")
        );
        let input_end = stream_result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::ToolInputEnd(input_end) => Some(input_end),
                _ => None,
            })
            .expect("stream includes tool input end");
        assert_eq!(
            openai_metadata_value(&input_end.provider_metadata, "namespace")
                .and_then(JsonValue::as_str),
            Some("weather_ns")
        );
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
