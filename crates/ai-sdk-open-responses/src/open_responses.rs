use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::convert::Infallible;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;

use ai_sdk_openai_compatible::openai_compatible::{
    OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel,
};
use ai_sdk_provider::file_data::{FileData, FileDataContent, ProviderReference};
use ai_sdk_provider::headers::Headers;
use ai_sdk_provider::json::{JsonObject, JsonValue, NonNullJsonValue};
use ai_sdk_provider::language_model::{
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
use ai_sdk_provider::provider::{
    ApiCallError, ModelType, NoSuchModelError, Provider, ProviderMetadata, ProviderOptions,
    SpecificationVersion,
};
use ai_sdk_provider::warning::Warning;
use ai_sdk_provider_utils::{
    FetchErrorInfo, HandledFetchError, ParseJsonResult, PostJsonToApiOptions, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, ReasoningLevel, RuntimeEnvironment, ToolNameMapping,
    combine_headers, convert_to_base64, create_event_source_response_handler,
    create_json_error_response_handler, create_json_response_handler, create_tool_name_mapping,
    generate_id, get_top_level_media_type, map_reasoning_to_provider_effort, post_json_to_api,
    resolve_full_media_type, with_user_agent_suffix,
};

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

    /// Deprecated file id prefixes recognized in prompt file data strings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_id_prefixes: Vec<String>,
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
            file_id_prefixes: Vec::new(),
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

    /// Adds a deprecated file id prefix recognized in prompt file data strings.
    pub fn with_file_id_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.file_id_prefixes.push(prefix.into());
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
            &self.config.settings.file_id_prefixes,
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
            &self.config.settings.file_id_prefixes,
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
    file_id_prefixes: &[String],
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
    let pass_through_unsupported_files = open_responses_pass_through_unsupported_files_enabled(
        provider_options_name,
        provider_options,
    );
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
        pass_through_unsupported_files,
        file_id_prefixes,
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

    let allowed_tools = open_responses_provider_option_value(
        provider_options_name,
        provider_options,
        &["allowedTools", "allowed_tools"],
    );
    let (tools, tool_choice) = open_responses_prepare_tools(
        &options.tools,
        &options.tool_choice,
        allowed_tools,
        &mut warnings,
    )?;
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
    pass_through_unsupported_files: bool,
    file_id_prefixes: &'a [String],
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
    let pass_through_unsupported_files = options.pass_through_unsupported_files;
    let file_id_prefixes = options.file_id_prefixes;
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

                for (index, part) in message.content.iter().enumerate() {
                    match part {
                        LanguageModelUserContentPart::Text(text) => {
                            content.push(json!({
                                "type": "input_text",
                                "text": text.text
                            }));
                        }
                        LanguageModelUserContentPart::File(file) => {
                            content.push(open_responses_file_part(
                                file,
                                provider_options_name,
                                OpenResponsesFilePartContext::Prompt {
                                    index,
                                    pass_through_unsupported_files,
                                    file_id_prefixes,
                                },
                            )?);
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
    reasoning: &ai_sdk_provider::language_model::LanguageModelReasoningPart,
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
    reasoning: &ai_sdk_provider::language_model::LanguageModelReasoningPart,
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

fn open_responses_pass_through_unsupported_files_enabled(
    provider_options_name: &str,
    provider_options: Option<&ProviderOptions>,
) -> bool {
    open_responses_provider_option_value(
        provider_options_name,
        provider_options,
        &[
            "passThroughUnsupportedFiles",
            "pass_through_unsupported_files",
        ],
    )
    .and_then(JsonValue::as_bool)
    .unwrap_or(false)
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

#[derive(Clone, Copy)]
enum OpenResponsesFilePartContext<'a> {
    Prompt {
        index: usize,
        pass_through_unsupported_files: bool,
        file_id_prefixes: &'a [String],
    },
    ToolResult,
}

fn open_responses_file_part(
    file: &LanguageModelFilePart,
    provider_options_name: &str,
    context: OpenResponsesFilePartContext<'_>,
) -> Result<JsonValue, String> {
    let top_level_media_type = get_top_level_media_type(&file.media_type);

    match &file.data {
        FileData::Reference { reference } => {
            if matches!(context, OpenResponsesFilePartContext::ToolResult) {
                return Err(
                    "Open Responses file parts with provider references are not implemented yet."
                        .to_string(),
                );
            }

            let file_id = reference
                .provider_id(provider_options_name)
                .map_err(|error| error.message().to_string())?;

            if top_level_media_type == "image" {
                Ok(open_responses_image_file_reference_part(
                    file_id,
                    open_responses_image_detail(file, provider_options_name),
                ))
            } else {
                Ok(json!({
                    "type": "input_file",
                    "file_id": file_id
                }))
            }
        }
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
            let prompt_file_id = match context {
                OpenResponsesFilePartContext::Prompt {
                    file_id_prefixes, ..
                } => open_responses_file_data_id(data, file_id_prefixes),
                OpenResponsesFilePartContext::ToolResult => None,
            };

            if top_level_media_type == "image" {
                if let Some(file_id) = prompt_file_id {
                    Ok(open_responses_image_file_reference_part(
                        file_id,
                        open_responses_image_detail(file, provider_options_name),
                    ))
                } else {
                    let full_media_type = resolve_full_media_type(file)
                        .map_err(|error| error.message().to_string())?;
                    let data_uri =
                        format!("data:{full_media_type};base64,{}", convert_to_base64(data));
                    Ok(open_responses_image_file_part(
                        data_uri,
                        open_responses_image_detail(file, provider_options_name),
                    ))
                }
            } else {
                let full_media_type =
                    resolve_full_media_type(file).map_err(|error| error.message().to_string())?;
                if let OpenResponsesFilePartContext::Prompt {
                    pass_through_unsupported_files,
                    ..
                } = context
                    && full_media_type != "application/pdf"
                    && !pass_through_unsupported_files
                {
                    return Err(format!("file part media type {full_media_type}"));
                }

                if let Some(file_id) = prompt_file_id {
                    Ok(json!({
                        "type": "input_file",
                        "file_id": file_id
                    }))
                } else {
                    let filename = match context {
                        OpenResponsesFilePartContext::Prompt { index, .. } => {
                            file.filename.clone().unwrap_or_else(|| {
                                if full_media_type == "application/pdf" {
                                    format!("part-{index}.pdf")
                                } else {
                                    format!("part-{index}")
                                }
                            })
                        }
                        OpenResponsesFilePartContext::ToolResult => {
                            file.filename.clone().unwrap_or_else(|| "data".to_string())
                        }
                    };
                    let data_uri =
                        format!("data:{full_media_type};base64,{}", convert_to_base64(data));

                    Ok(json!({
                        "type": "input_file",
                        "filename": filename,
                        "file_data": data_uri
                    }))
                }
            }
        }
    }
}

fn open_responses_file_data_id<'a>(
    data: &'a FileDataContent,
    file_id_prefixes: &[String],
) -> Option<&'a str> {
    let FileDataContent::Base64(value) = data else {
        return None;
    };

    file_id_prefixes
        .iter()
        .any(|prefix| value.starts_with(prefix))
        .then_some(value.as_str())
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

fn open_responses_image_file_reference_part(file_id: &str, detail: Option<JsonValue>) -> JsonValue {
    let mut part = JsonObject::new();
    part.insert(
        "type".to_string(),
        JsonValue::String("input_image".to_string()),
    );
    part.insert(
        "file_id".to_string(),
        JsonValue::String(file_id.to_string()),
    );
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
                        match open_responses_file_part(
                            file,
                            provider_options_name,
                            OpenResponsesFilePartContext::ToolResult,
                        ) {
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
    allowed_tools: Option<&JsonValue>,
    warnings: &mut Vec<Warning>,
) -> Result<(Option<Vec<JsonValue>>, Option<JsonValue>), String> {
    let provider_tool_names = open_responses_provider_tool_names();
    let tool_name_mapping = create_tool_name_mapping(tools.iter().flatten(), &provider_tool_names);
    let mut custom_provider_tool_names = BTreeSet::new();

    let prepared_tools = if let Some(tools) = tools.as_ref() {
        let mut prepared_tools = Vec::new();

        for tool in tools {
            match tool {
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

                    prepared_tools.push(JsonValue::Object(function));
                }
                LanguageModelTool::Provider(tool) => {
                    if let Some(tool) = open_responses_prepare_provider_tool(
                        tool,
                        warnings,
                        &mut custom_provider_tool_names,
                    )? {
                        prepared_tools.push(tool);
                    }
                }
            }
        }

        (!prepared_tools.is_empty()).then_some(prepared_tools)
    } else {
        None
    };

    let prepared_tool_choice =
        open_responses_allowed_tools_choice(allowed_tools, &tool_name_mapping).or_else(|| {
            tool_choice.as_ref().map(|choice| match choice {
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
            })
        });

    Ok((prepared_tools, prepared_tool_choice))
}

fn open_responses_allowed_tools_choice(
    allowed_tools: Option<&JsonValue>,
    tool_name_mapping: &ToolNameMapping,
) -> Option<JsonValue> {
    let allowed_tools = allowed_tools?.as_object()?;
    let tool_names = allowed_tools
        .get("toolNames")
        .or_else(|| allowed_tools.get("tool_names"))?
        .as_array()?
        .iter()
        .filter_map(JsonValue::as_str)
        .map(|name| {
            json!({
                "type": "function",
                "name": tool_name_mapping.to_provider_tool_name(name)
            })
        })
        .collect::<Vec<_>>();

    if tool_names.is_empty() {
        return None;
    }

    let mode = allowed_tools
        .get("mode")
        .and_then(JsonValue::as_str)
        .filter(|mode| matches!(*mode, "auto" | "required"))
        .unwrap_or("auto");

    Some(json!({
        "type": "allowed_tools",
        "mode": mode,
        "tools": tool_names
    }))
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
) -> Result<Option<JsonValue>, String> {
    let prepared = match tool.id.as_str() {
        "openai.file_search" => open_responses_file_search_tool(&tool.args),
        "openai.local_shell" => open_responses_tool_with_type("local_shell"),
        "openai.shell" => open_responses_shell_tool(&tool.args)?,
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
            return Ok(None);
        }
    };

    Ok(Some(JsonValue::Object(prepared)))
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

fn open_responses_shell_tool(args: &JsonObject) -> Result<JsonObject, String> {
    let mut tool = open_responses_tool_with_type("shell");

    if let Some(environment) = args.get("environment").and_then(JsonValue::as_object) {
        let mapped_environment = open_responses_shell_environment(environment)?;
        tool.insert(
            "environment".to_string(),
            JsonValue::Object(mapped_environment),
        );
    }

    Ok(tool)
}

fn open_responses_shell_environment(environment: &JsonObject) -> Result<JsonObject, String> {
    match environment.get("type").and_then(JsonValue::as_str) {
        Some("containerReference") => {
            let mut mapped_environment = open_responses_tool_with_type("container_reference");
            open_responses_insert_arg(
                &mut mapped_environment,
                "container_id",
                environment,
                "containerId",
            );
            Ok(mapped_environment)
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
                let mut mapped_skills = Vec::new();
                for skill in skills {
                    if let Some(skill) = open_responses_shell_skill(skill)? {
                        mapped_skills.push(JsonValue::Object(skill));
                    }
                }
                mapped_environment.insert("skills".to_string(), JsonValue::Array(mapped_skills));
            }

            Ok(mapped_environment)
        }
        _ => {
            let mut mapped_environment = open_responses_tool_with_type("local");
            open_responses_insert_arg(&mut mapped_environment, "skills", environment, "skills");
            Ok(mapped_environment)
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

fn open_responses_shell_skill(skill: &JsonValue) -> Result<Option<JsonObject>, String> {
    let Some(skill) = skill.as_object() else {
        return Ok(None);
    };

    if matches!(
        skill.get("type").and_then(JsonValue::as_str),
        Some("skillReference")
    ) {
        let mut mapped_skill = open_responses_tool_with_type("skill_reference");
        let skill_id = open_responses_shell_skill_reference_id(skill)?;
        mapped_skill.insert("skill_id".to_string(), JsonValue::String(skill_id));
        mapped_skill.insert(
            "version".to_string(),
            open_responses_arg(skill, "version")
                .unwrap_or_else(|| JsonValue::String("latest".to_string())),
        );
        return Ok(Some(mapped_skill));
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

    Ok(Some(mapped_skill))
}

fn open_responses_shell_skill_reference_id(skill: &JsonObject) -> Result<String, String> {
    let Some(reference) = skill.get("providerReference") else {
        return Err("Open Responses shell skillReference requires providerReference".to_string());
    };
    let reference = serde_json::from_value::<ProviderReference>(reference.clone())
        .map_err(|error| format!("invalid Open Responses shell providerReference: {error}"))?;

    reference
        .provider_id("openai")
        .map(str::to_string)
        .map_err(|error| error.to_string())
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
    use super::map_open_responses_finish_reason;
    use ai_sdk_provider::language_model::FinishReason;

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
}
