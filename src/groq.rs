use std::collections::BTreeMap;
use std::env;
use std::future::{Future, ready};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;

use crate::file_data::FileDataContent;
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    FinishReason, InputTokenUsage, LanguageModel, LanguageModelCallOptions,
    LanguageModelErrorStreamPart, LanguageModelFinishReason, LanguageModelFunctionTool,
    LanguageModelGenerateResult, LanguageModelRequest, LanguageModelResponseFormat,
    LanguageModelStreamPart, LanguageModelStreamResult, LanguageModelTool, LanguageModelUsage,
    OutputTokenUsage,
};
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
use crate::warning::Warning;

/// Default base URL for upstream `@ai-sdk/groq` API calls.
pub const DEFAULT_GROQ_BASE_URL: &str = "https://api.groq.com/openai/v1";

const GROQ_BROWSER_SEARCH_TOOL_MARKER: &str = "__groq_browser_search";
const GROQ_UNSUPPORTED_TOOL_MARKER: &str = "__groq_unsupported_provider_tool";
const GROQ_BROWSER_SEARCH_SUPPORTED_MODELS: &[&str] =
    &["openai/gpt-oss-20b", "openai/gpt-oss-120b"];

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
    current_date: Arc<dyn Fn() -> OffsetDateTime + Send + Sync>,
}

/// Groq chat language model with provider-specific request and response parity.
#[derive(Clone)]
pub struct GroqChatLanguageModel {
    inner: OpenAICompatibleChatLanguageModel,
}

/// Groq transcription model for `/audio/transcriptions` calls.
#[derive(Clone)]
pub struct GroqTranscriptionModel {
    model_id: String,
    base_url: String,
    settings: GroqProviderSettings,
    transport: OpenAICompatibleTransport,
    current_date: Arc<dyn Fn() -> OffsetDateTime + Send + Sync>,
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
            current_date: Arc::new(OffsetDateTime::now_utc),
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

    /// Replaces the response timestamp provider. This is primarily useful for tests.
    pub fn with_current_date<F>(mut self, current_date: F) -> Self
    where
        F: Fn() -> OffsetDateTime + Send + Sync + 'static,
    {
        self.current_date = Arc::new(current_date);
        self
    }

    /// Creates a Groq chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> GroqChatLanguageModel {
        self.chat(model_id)
    }

    /// Creates a Groq chat language model.
    pub fn chat(&self, model_id: impl Into<String>) -> GroqChatLanguageModel {
        GroqChatLanguageModel::new(self.openai_compatible_provider().chat_model(model_id))
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
            Arc::clone(&self.current_date),
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
                .with_transform_request_body(groq_transform_chat_request_body)
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
    type LanguageModel = GroqChatLanguageModel;
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

impl GroqChatLanguageModel {
    fn new(inner: OpenAICompatibleChatLanguageModel) -> Self {
        Self { inner }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        self.inner.provider()
    }

    /// Returns whether structured outputs are enabled for this chat model.
    pub fn supports_structured_outputs(&self) -> bool {
        self.inner.supports_structured_outputs()
    }

    async fn do_generate_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelGenerateResult {
        let (options, extra_warnings) = match groq_prepare_chat_options(self.model_id(), options) {
            Ok(result) => result,
            Err(message) => {
                return groq_chat_error_generate_result(
                    message,
                    json!({
                        "model": self.model_id()
                    }),
                );
            }
        };

        let mut result = self.inner.do_generate(options).await;
        result.usage = groq_chat_usage(
            result
                .response
                .as_ref()
                .and_then(|response| response.body.as_ref())
                .and_then(|body| body.get("usage")),
        );

        for warning in extra_warnings {
            result = result.with_warning(warning);
        }

        result
    }

    async fn do_stream_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let (options, extra_warnings) = match groq_prepare_chat_options(self.model_id(), options) {
            Ok(result) => result,
            Err(message) => {
                return groq_chat_error_stream_result(
                    message,
                    json!({
                        "model": self.model_id()
                    }),
                );
            }
        };

        let mut result = self.inner.do_stream(options).await;
        groq_append_stream_warnings(&mut result.stream, extra_warnings);
        groq_rewrite_stream_finish_usage(&mut result.stream);
        result
    }
}

impl LanguageModel for GroqChatLanguageModel {
    type SupportedUrlsFuture<'a>
        = <OpenAICompatibleChatLanguageModel as LanguageModel>::SupportedUrlsFuture<'a>
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
        GroqChatLanguageModel::provider(self)
    }

    fn model_id(&self) -> &str {
        GroqChatLanguageModel::model_id(self)
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        self.inner.supported_urls()
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(self.do_stream_result(options))
    }
}

impl GroqTranscriptionModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: GroqProviderSettings,
        transport: OpenAICompatibleTransport,
        current_date: Arc<dyn Fn() -> OffsetDateTime + Send + Sync>,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            base_url: base_url.into(),
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
        "groq.transcription"
    }

    async fn do_generate_result(
        &self,
        options: TranscriptionModelCallOptions,
    ) -> TranscriptionModelResult {
        let timestamp = (self.current_date)();
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
pub fn groq(model_id: impl Into<String>) -> GroqChatLanguageModel {
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

fn groq_prepare_chat_options(
    model_id: &str,
    mut options: LanguageModelCallOptions,
) -> Result<(LanguageModelCallOptions, Vec<Warning>), String> {
    let groq_options = options
        .provider_options
        .as_ref()
        .and_then(|provider_options| provider_options.get("groq"));

    if let Some(groq_options) = groq_options {
        groq_validate_chat_provider_options(groq_options)?;
    }

    let mut warnings = Vec::new();
    if groq_options
        .and_then(|options| options.get("structuredOutputs"))
        .and_then(JsonValue::as_bool)
        == Some(false)
        && matches!(
            options.response_format.as_ref(),
            Some(LanguageModelResponseFormat::Json {
                schema: Some(_),
                ..
            })
        )
    {
        warnings.push(Warning::Unsupported {
            feature: "responseFormat".to_string(),
            details: Some(
                "JSON response format schema is only supported with structuredOutputs".to_string(),
            ),
        });
    }
    warnings.extend(groq_prepare_browser_search_tools(model_id, &mut options));
    Ok((options, warnings))
}

fn groq_validate_chat_provider_options(options: &JsonObject) -> Result<(), String> {
    groq_validate_enum_option(options, "reasoningFormat", &["parsed", "raw", "hidden"])?;
    groq_validate_enum_option(
        options,
        "reasoningEffort",
        &["none", "default", "low", "medium", "high"],
    )?;
    groq_validate_enum_option(
        options,
        "serviceTier",
        &["on_demand", "performance", "flex", "auto"],
    )?;
    groq_validate_string_option(options, "user")?;
    groq_validate_bool_option(options, "parallelToolCalls")?;
    groq_validate_bool_option(options, "structuredOutputs")?;
    groq_validate_bool_option(options, "strictJsonSchema")?;
    Ok(())
}

fn groq_validate_enum_option(
    options: &JsonObject,
    name: &str,
    allowed: &[&str],
) -> Result<(), String> {
    let Some(value) = options.get(name) else {
        return Ok(());
    };
    let Some(value) = value.as_str() else {
        return Err(format!("Groq provider option `{name}` must be a string"));
    };
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "Groq provider option `{name}` must be one of {}",
            allowed.join(", ")
        ))
    }
}

fn groq_validate_string_option(options: &JsonObject, name: &str) -> Result<(), String> {
    let Some(value) = options.get(name) else {
        return Ok(());
    };
    if value.is_string() {
        Ok(())
    } else {
        Err(format!("Groq provider option `{name}` must be a string"))
    }
}

fn groq_validate_bool_option(options: &JsonObject, name: &str) -> Result<(), String> {
    let Some(value) = options.get(name) else {
        return Ok(());
    };
    if value.is_boolean() {
        Ok(())
    } else {
        Err(format!("Groq provider option `{name}` must be a boolean"))
    }
}

fn groq_prepare_browser_search_tools(
    model_id: &str,
    options: &mut LanguageModelCallOptions,
) -> Vec<Warning> {
    let Some(tools) = options.tools.take() else {
        return Vec::new();
    };

    let mut warnings = Vec::new();
    let mut prepared_tools = Vec::new();

    for tool in tools {
        match tool {
            LanguageModelTool::Provider(provider_tool)
                if provider_tool.id == "groq.browser_search" =>
            {
                if groq_supports_browser_search(model_id) {
                    prepared_tools.push(LanguageModelTool::Function(
                        LanguageModelFunctionTool::new(
                            GROQ_BROWSER_SEARCH_TOOL_MARKER,
                            JsonObject::new(),
                        ),
                    ));
                } else {
                    prepared_tools.push(LanguageModelTool::Function(
                        LanguageModelFunctionTool::new(
                            GROQ_UNSUPPORTED_TOOL_MARKER,
                            JsonObject::new(),
                        ),
                    ));
                    warnings.push(Warning::Unsupported {
                        feature: "provider-defined tool groq.browser_search".to_string(),
                        details: Some(format!(
                            "Browser search is only supported on the following models: {}. Current model: {model_id}",
                            GROQ_BROWSER_SEARCH_SUPPORTED_MODELS.join(", ")
                        )),
                    });
                }
            }
            other => prepared_tools.push(other),
        }
    }

    options.tools = Some(prepared_tools);
    warnings
}

fn groq_supports_browser_search(model_id: &str) -> bool {
    GROQ_BROWSER_SEARCH_SUPPORTED_MODELS.contains(&model_id)
}

fn groq_transform_chat_request_body(mut request_body: JsonValue) -> JsonValue {
    let JsonValue::Object(body) = &mut request_body else {
        return request_body;
    };

    if let Some(JsonValue::String(reasoning_effort)) = body.get_mut("reasoning_effort") {
        match reasoning_effort.as_str() {
            "minimal" => *reasoning_effort = "low".to_string(),
            "xhigh" => *reasoning_effort = "high".to_string(),
            _ => {}
        }
    }

    if let Some(reasoning_format) = body.remove("reasoningFormat") {
        body.insert("reasoning_format".to_string(), reasoning_format);
    }

    if body.get("structuredOutputs").and_then(JsonValue::as_bool) == Some(false)
        && body
            .get("response_format")
            .and_then(|response_format| response_format.get("type"))
            .and_then(JsonValue::as_str)
            == Some("json_schema")
    {
        body.insert(
            "response_format".to_string(),
            json!({
                "type": "json_object"
            }),
        );
    }

    body.remove("structuredOutputs");
    groq_transform_chat_messages(body);
    groq_transform_chat_tools(body);
    request_body
}

fn groq_transform_chat_messages(body: &mut JsonObject) {
    let Some(JsonValue::Array(messages)) = body.get_mut("messages") else {
        return;
    };

    for message in messages {
        let JsonValue::Object(message) = message else {
            continue;
        };

        if message.get("role").and_then(JsonValue::as_str) != Some("assistant") {
            continue;
        }

        if let Some(reasoning) = message.remove("reasoning_content") {
            message.insert("reasoning".to_string(), reasoning);
        }

        if message
            .get("content")
            .is_some_and(|content| content.is_null())
        {
            message.insert("content".to_string(), JsonValue::String(String::new()));
        }
    }
}

fn groq_transform_chat_tools(body: &mut JsonObject) {
    let Some(JsonValue::Array(tools)) = body.get_mut("tools") else {
        return;
    };

    let mut transformed_tools = Vec::new();
    for tool in std::mem::take(tools) {
        let is_browser_search_marker = tool
            .get("type")
            .and_then(JsonValue::as_str)
            .is_some_and(|tool_type| tool_type == "function")
            && tool
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(JsonValue::as_str)
                .is_some_and(|name| name == GROQ_BROWSER_SEARCH_TOOL_MARKER);

        if is_browser_search_marker {
            transformed_tools.push(json!({
                "type": "browser_search"
            }));
            continue;
        }

        let is_unsupported_tool_marker = tool
            .get("type")
            .and_then(JsonValue::as_str)
            .is_some_and(|tool_type| tool_type == "function")
            && tool
                .get("function")
                .and_then(|function| function.get("name"))
                .and_then(JsonValue::as_str)
                .is_some_and(|name| name == GROQ_UNSUPPORTED_TOOL_MARKER);

        if !is_unsupported_tool_marker {
            transformed_tools.push(tool);
        }
    }

    *tools = transformed_tools;
}

fn groq_chat_usage(value: Option<&JsonValue>) -> LanguageModelUsage {
    let Some(value) = value.and_then(JsonValue::as_object) else {
        return LanguageModelUsage::default();
    };

    let input_total = groq_json_u64(value.get("prompt_tokens")).unwrap_or_default();
    let output_total = groq_json_u64(value.get("completion_tokens")).unwrap_or_default();
    let reasoning_tokens = value
        .get("completion_tokens_details")
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(|value| groq_json_u64(Some(value)));
    let text_tokens = reasoning_tokens.map_or(output_total, |reasoning_tokens| {
        output_total.saturating_sub(reasoning_tokens)
    });

    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: Some(input_total),
            no_cache: Some(input_total),
            cache_read: None,
            cache_write: None,
        },
        output_tokens: OutputTokenUsage {
            total: Some(output_total),
            text: Some(text_tokens),
            reasoning: reasoning_tokens,
        },
        raw: Some(value.clone()),
    }
}

fn groq_json_u64(value: Option<&JsonValue>) -> Option<u64> {
    match value? {
        JsonValue::Number(number) => number.as_u64(),
        _ => None,
    }
}

fn groq_rewrite_stream_finish_usage(stream: &mut [LanguageModelStreamPart]) {
    for part in stream {
        let LanguageModelStreamPart::Finish(finish) = part else {
            continue;
        };

        finish.usage = match finish.usage.raw.clone() {
            Some(raw) => groq_chat_usage(Some(&JsonValue::Object(raw))),
            None => LanguageModelUsage::default(),
        };
    }
}

fn groq_append_stream_warnings(stream: &mut Vec<LanguageModelStreamPart>, warnings: Vec<Warning>) {
    if warnings.is_empty() {
        return;
    }

    if let Some(LanguageModelStreamPart::StreamStart(start)) = stream
        .iter_mut()
        .find(|part| matches!(part, LanguageModelStreamPart::StreamStart(_)))
    {
        start.warnings.extend(warnings);
    } else {
        stream.insert(
            0,
            LanguageModelStreamPart::StreamStart(
                crate::language_model::LanguageModelStreamStart::new(warnings),
            ),
        );
    }
}

fn groq_chat_error_generate_result(
    message: impl Into<String>,
    request_body: JsonValue,
) -> LanguageModelGenerateResult {
    LanguageModelGenerateResult::new(
        Vec::new(),
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: Some("groq-error".to_string()),
        },
        LanguageModelUsage::default(),
    )
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_provider_metadata(groq_error_metadata(message.into()))
}

fn groq_chat_error_stream_result(
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

fn groq_error_metadata(message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut extra = JsonObject::new();
    extra.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert("groq".to_string(), extra);
    metadata
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
        ProviderApiRequestMethod::Post => execute_groq_post_request(request),
    }
}

fn execute_groq_post_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut headers = request.headers.clone();
    let mut builder = ureq::post(&request.url);

    let response = match request.body {
        Some(ProviderApiRequestBody::FormData { content }) => {
            let boundary = "ai-sdk-rust-groq-boundary";
            headers.insert(
                "content-type".to_string(),
                format!("multipart/form-data; boundary={boundary}"),
            );

            for (name, value) in &headers {
                builder = builder.header(name.as_str(), value.as_str());
            }

            builder
                .config()
                .http_status_as_error(false)
                .build()
                .send(groq_multipart_body(&content, boundary))
        }
        Some(ProviderApiRequestBody::Text { content }) => {
            for (name, value) in &headers {
                builder = builder.header(name.as_str(), value.as_str());
            }

            builder
                .config()
                .http_status_as_error(false)
                .build()
                .send(content)
        }
        Some(ProviderApiRequestBody::Bytes { content }) => {
            for (name, value) in &headers {
                builder = builder.header(name.as_str(), value.as_str());
            }

            builder
                .config()
                .http_status_as_error(false)
                .build()
                .send(content)
        }
        None => {
            for (name, value) in &headers {
                builder = builder.header(name.as_str(), value.as_str());
            }

            builder
                .config()
                .http_status_as_error(false)
                .build()
                .send_empty()
        }
    };

    groq_provider_api_response(response)
}

fn groq_multipart_body(form_data: &FormData, boundary: &str) -> Vec<u8> {
    let mut body = Vec::new();

    for entry in &form_data.entries {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let name = groq_multipart_escape(&entry.name);

        match &entry.value {
            FormDataValue::Text { value } => {
                body.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
                );
                body.extend_from_slice(value.as_bytes());
            }
            FormDataValue::Bytes { value } => {
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(value);
            }
        }

        body.extend_from_slice(b"\r\n");
    }

    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

fn groq_multipart_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\r', '\n'], "")
}

fn groq_provider_api_response(
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
        DEFAULT_GROQ_BASE_URL, GroqChatLanguageModel, GroqProvider, GroqProviderSettings,
        create_groq, groq, groq_chat_usage,
    };
    use crate::file_data::{FileData, FileDataContent, ProviderReference};
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::{JsonObject, JsonValue};
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelAssistantContentPart,
        LanguageModelAssistantMessage, LanguageModelCallOptions, LanguageModelContent,
        LanguageModelFilePart, LanguageModelFunctionTool, LanguageModelMessage,
        LanguageModelProviderTool, LanguageModelReasoningEffort, LanguageModelReasoningPart,
        LanguageModelResponseFormat, LanguageModelStreamPart, LanguageModelTextPart,
        LanguageModelTool, LanguageModelToolCallPart, LanguageModelToolChoice,
        LanguageModelToolContentPart, LanguageModelToolMessage, LanguageModelToolResultOutput,
        LanguageModelToolResultPart, LanguageModelUserContentPart, LanguageModelUserMessage,
    };
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::prompt::Prompt;
    use crate::provider::{ModelType, Provider, ProviderOptions, ProviderWithTranscriptionModel};
    use crate::provider_utils::{
        FormDataValue, ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod,
        ProviderApiResponse,
    };
    use crate::transcription_model::{TranscriptionModel, TranscriptionModelCallOptions};
    use crate::warning::Warning;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::env;
    use std::future::{Future, ready};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use time::OffsetDateTime;
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

    fn groq_prompt() -> Vec<LanguageModelMessage> {
        vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![LanguageModelUserContentPart::Text(
                LanguageModelTextPart::new("Hello"),
            )],
        ))]
    }

    fn groq_text_response() -> JsonValue {
        json!({
            "id": "chatcmpl-groq",
            "created": 1711115037,
            "model": "gemma2-9b-it",
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
    }

    fn groq_model_with_response(
        model_id: &str,
        response: JsonValue,
    ) -> (GroqChatLanguageModel, Arc<Mutex<Vec<ProviderApiRequest>>>) {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                captured_requests_for_transport
                    .lock()
                    .expect("captured requests mutex is not poisoned")
                    .push(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    response.clone().to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_groq".to_string(),
                )])))))
            });
        let model = create_groq(
            GroqProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.groq.test/openai/v1"),
        )
        .with_transport(transport)
        .language_model(model_id);

        (model, captured_requests)
    }

    fn groq_model_with_stream(
        model_id: &str,
        stream: &str,
    ) -> (GroqChatLanguageModel, Arc<Mutex<Vec<ProviderApiRequest>>>) {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let stream = stream.to_string();
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                captured_requests_for_transport
                    .lock()
                    .expect("captured requests mutex is not poisoned")
                    .push(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    stream.clone(),
                )
                .with_headers(Headers::from([
                    ("content-type".to_string(), "text/event-stream".to_string()),
                    ("x-request-id".to_string(), "req_groq_stream".to_string()),
                ])))))
            });
        let model = create_groq(
            GroqProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.groq.test/openai/v1"),
        )
        .with_transport(transport)
        .language_model(model_id);

        (model, captured_requests)
    }

    fn groq_request_body_for_options(
        model_id: &str,
        options: LanguageModelCallOptions,
    ) -> (
        crate::language_model::LanguageModelGenerateResult,
        Option<JsonValue>,
    ) {
        let (model, captured_requests) = groq_model_with_response(model_id, groq_text_response());
        let result = poll_ready(model.do_generate(options));
        let request_body = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")
            .first()
            .and_then(|request| request.body.as_ref())
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok());

        (result, request_body)
    }

    fn groq_schema() -> crate::json::JsonObject {
        json!({
            "type": "object",
            "properties": {
                "value": {
                    "type": "string"
                }
            },
            "required": ["value"],
            "additionalProperties": false,
            "$schema": "http://json-schema.org/draft-07/schema#"
        })
        .as_object()
        .expect("schema is an object")
        .clone()
    }

    fn empty_schema() -> crate::json::JsonObject {
        json!({
            "type": "object",
            "properties": {}
        })
        .as_object()
        .expect("schema is an object")
        .clone()
    }

    fn groq_provider_options(value: JsonValue) -> ProviderOptions {
        serde_json::from_value(value).expect("provider options deserialize")
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
    fn groq_chat_do_generate_maps_content_usage_response_and_request_metadata() {
        let (model, captured_requests) = groq_model_with_response(
            "gemma2-9b-it",
            json!({
                "id": "chatcmpl-test",
                "created": 1711115037,
                "model": "llama-3.3-70b-versatile",
                "choices": [
                    {
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": "Final answer",
                            "reasoning": "I checked the tool first.",
                            "tool_calls": [
                                {
                                    "id": "call_1",
                                    "type": "function",
                                    "function": {
                                        "name": "lookup",
                                        "arguments": "{\"city\":\"Brisbane\"}"
                                    }
                                }
                            ]
                        },
                        "finish_reason": "eos"
                    }
                ],
                "usage": {
                    "prompt_tokens": 20,
                    "completion_tokens": 50,
                    "total_tokens": 70,
                    "prompt_tokens_details": {
                        "cached_tokens": 15
                    },
                    "completion_tokens_details": {
                        "reasoning_tokens": 21
                    }
                }
            }),
        );

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(groq_prompt())));

        assert_eq!(result.finish_reason.unified, FinishReason::Other);
        assert_eq!(result.finish_reason.raw.as_deref(), Some("eos"));
        assert!(matches!(
            result.content.first(),
            Some(LanguageModelContent::Text(text)) if text.text == "Final answer"
        ));
        assert!(result.content.iter().any(|part| matches!(
            part,
            LanguageModelContent::Reasoning(reasoning)
                if reasoning.text == "I checked the tool first."
        )));
        assert!(result.content.iter().any(|part| matches!(
            part,
            LanguageModelContent::ToolCall(tool_call)
                if tool_call.tool_call_id == "call_1"
                    && tool_call.tool_name == "lookup"
                    && tool_call.input == "{\"city\":\"Brisbane\"}"
        )));
        assert_eq!(result.usage.input_tokens.total, Some(20));
        assert_eq!(result.usage.input_tokens.no_cache, Some(20));
        assert_eq!(result.usage.input_tokens.cache_read, None);
        assert_eq!(result.usage.output_tokens.total, Some(50));
        assert_eq!(result.usage.output_tokens.text, Some(29));
        assert_eq!(result.usage.output_tokens.reasoning, Some(21));
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("chatcmpl-test")
        );
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.model_id.as_deref()),
            Some("llama-3.3-70b-versatile")
        );
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_groq")
        );

        let request_body = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")[0]
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body,
            json!({
                "model": "gemma2-9b-it",
                "messages": [
                    {
                        "role": "user",
                        "content": "Hello"
                    }
                ]
            })
        );
    }

    #[test]
    fn groq_chat_maps_reasoning_effort_variants_and_provider_override() {
        let (_result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(groq_prompt())
                .with_reasoning(LanguageModelReasoningEffort::High),
        );
        assert_eq!(
            request_body
                .as_ref()
                .and_then(|body| body.get("reasoning_effort")),
            Some(&json!("high"))
        );

        let (_result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(groq_prompt())
                .with_reasoning(LanguageModelReasoningEffort::Minimal),
        );
        assert_eq!(
            request_body
                .as_ref()
                .and_then(|body| body.get("reasoning_effort")),
            Some(&json!("low"))
        );

        let (_result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(groq_prompt())
                .with_reasoning(LanguageModelReasoningEffort::Xhigh),
        );
        assert_eq!(
            request_body
                .as_ref()
                .and_then(|body| body.get("reasoning_effort")),
            Some(&json!("high"))
        );

        let (_result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(groq_prompt())
                .with_reasoning(LanguageModelReasoningEffort::None),
        );
        assert!(
            request_body
                .as_ref()
                .is_some_and(|body| body.get("reasoning_effort").is_none())
        );

        let provider_options = groq_provider_options(json!({
            "groq": {
                "reasoningEffort": "high"
            }
        }));
        let (_result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(groq_prompt())
                .with_reasoning(LanguageModelReasoningEffort::Medium)
                .with_provider_options(provider_options),
        );
        assert_eq!(
            request_body
                .as_ref()
                .and_then(|body| body.get("reasoning_effort")),
            Some(&json!("high"))
        );
    }

    #[test]
    fn groq_chat_passes_provider_options_response_formats_and_validation() {
        let provider_options = groq_provider_options(json!({
            "groq": {
                "reasoningFormat": "hidden",
                "user": "test-user-id",
                "parallelToolCalls": false,
                "serviceTier": "performance",
                "strictJsonSchema": false
            }
        }));
        let (_result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(groq_prompt())
                .with_response_format(
                    LanguageModelResponseFormat::json()
                        .with_name("test-name")
                        .with_description("test description")
                        .with_schema(groq_schema()),
                )
                .with_provider_options(provider_options),
        );
        let request_body = request_body.expect("request body is captured");
        assert_eq!(request_body["reasoning_format"], "hidden");
        assert_eq!(request_body["user"], "test-user-id");
        assert_eq!(request_body["parallel_tool_calls"], false);
        assert_eq!(request_body["service_tier"], "performance");
        assert_eq!(
            request_body["response_format"]["json_schema"]["strict"],
            false
        );

        let provider_options = groq_provider_options(json!({
            "groq": {
                "structuredOutputs": false
            }
        }));
        let (result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(groq_prompt())
                .with_response_format(
                    LanguageModelResponseFormat::json().with_schema(groq_schema()),
                )
                .with_provider_options(provider_options),
        );
        assert_eq!(
            request_body
                .as_ref()
                .and_then(|body| body.get("response_format")),
            Some(&json!({
                "type": "json_object"
            }))
        );
        assert!(result.warnings.iter().any(|warning| matches!(
            warning,
            Warning::Unsupported { feature, details }
                if feature == "responseFormat"
                    && details.as_deref()
                        == Some("JSON response format schema is only supported with structuredOutputs")
        )));

        for service_tier in ["on_demand", "performance", "flex", "auto"] {
            let (_result, request_body) = groq_request_body_for_options(
                "gemma2-9b-it",
                LanguageModelCallOptions::new(groq_prompt()).with_provider_options(
                    groq_provider_options(json!({
                        "groq": {
                            "serviceTier": service_tier
                        }
                    })),
                ),
            );
            assert_eq!(
                request_body
                    .as_ref()
                    .and_then(|body| body.get("service_tier")),
                Some(&json!(service_tier))
            );
        }

        for reasoning_effort in ["none", "default", "low", "medium", "high"] {
            let (_result, request_body) = groq_request_body_for_options(
                "gemma2-9b-it",
                LanguageModelCallOptions::new(groq_prompt()).with_provider_options(
                    groq_provider_options(json!({
                        "groq": {
                            "reasoningEffort": reasoning_effort
                        }
                    })),
                ),
            );
            assert_eq!(
                request_body
                    .as_ref()
                    .and_then(|body| body.get("reasoning_effort")),
                Some(&json!(reasoning_effort))
            );
        }

        for invalid_options in [
            json!({ "groq": { "reasoningEffort": "minimal" } }),
            json!({ "groq": { "reasoningEffort": "ultra-high" } }),
            json!({ "groq": { "serviceTier": "priority" } }),
            json!({ "groq": { "parallelToolCalls": "yes" } }),
        ] {
            let (result, request_body) = groq_request_body_for_options(
                "gemma2-9b-it",
                LanguageModelCallOptions::new(groq_prompt())
                    .with_provider_options(groq_provider_options(invalid_options)),
            );
            assert!(request_body.is_none());
            assert_eq!(result.finish_reason.unified, FinishReason::Error);
            assert!(
                result
                    .provider_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("groq"))
                    .and_then(|metadata| metadata.get("errorMessage"))
                    .and_then(JsonValue::as_str)
                    .is_some()
            );
        }
    }

    #[test]
    fn groq_prepare_tools_maps_function_choices_strict_and_browser_search() {
        let (_result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(groq_prompt()),
        );
        assert!(
            request_body.as_ref().is_some_and(
                |body| body.get("tools").is_none() && body.get("tool_choice").is_none()
            )
        );

        let mut empty_tool_options = LanguageModelCallOptions::new(groq_prompt());
        empty_tool_options.tools = Some(Vec::new());
        let (_result, request_body) =
            groq_request_body_for_options("gemma2-9b-it", empty_tool_options);
        assert!(
            request_body.as_ref().is_some_and(
                |body| body.get("tools").is_none() && body.get("tool_choice").is_none()
            )
        );

        let (_result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(groq_prompt())
                .with_tool(LanguageModelTool::Function(
                    LanguageModelFunctionTool::new("strictTool", empty_schema())
                        .with_description("A strict tool")
                        .with_strict(true),
                ))
                .with_tool(LanguageModelTool::Function(
                    LanguageModelFunctionTool::new("nonStrictTool", empty_schema())
                        .with_description("A non-strict tool")
                        .with_strict(false),
                ))
                .with_tool(LanguageModelTool::Function(
                    LanguageModelFunctionTool::new("defaultTool", empty_schema())
                        .with_description("A tool without strict setting"),
                ))
                .with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "strictTool".to_string(),
                }),
        );
        let request_body = request_body.expect("request body is captured");
        assert_eq!(
            request_body["tools"],
            json!([
                {
                    "type": "function",
                    "function": {
                        "name": "strictTool",
                        "description": "A strict tool",
                        "parameters": {
                            "type": "object",
                            "properties": {}
                        },
                        "strict": true
                    }
                },
                {
                    "type": "function",
                    "function": {
                        "name": "nonStrictTool",
                        "description": "A non-strict tool",
                        "parameters": {
                            "type": "object",
                            "properties": {}
                        },
                        "strict": false
                    }
                },
                {
                    "type": "function",
                    "function": {
                        "name": "defaultTool",
                        "description": "A tool without strict setting",
                        "parameters": {
                            "type": "object",
                            "properties": {}
                        }
                    }
                }
            ])
        );
        assert_eq!(
            request_body["tool_choice"],
            json!({
                "type": "function",
                "function": {
                    "name": "strictTool"
                }
            })
        );

        for (choice, expected) in [
            (LanguageModelToolChoice::Auto, json!("auto")),
            (LanguageModelToolChoice::Required, json!("required")),
            (LanguageModelToolChoice::None, json!("none")),
        ] {
            let (_result, request_body) = groq_request_body_for_options(
                "gemma2-9b-it",
                LanguageModelCallOptions::new(groq_prompt())
                    .with_tool(LanguageModelTool::Function(
                        LanguageModelFunctionTool::new("testFunction", empty_schema())
                            .with_description("Test"),
                    ))
                    .with_tool_choice(choice),
            );
            assert_eq!(
                request_body
                    .as_ref()
                    .and_then(|body| body.get("tool_choice")),
                Some(&expected)
            );
        }

        let (result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(groq_prompt()).with_tool(LanguageModelTool::Provider(
                LanguageModelProviderTool::new(
                    "some.unsupported_tool",
                    "unsupported_tool",
                    JsonObject::new(),
                ),
            )),
        );
        assert_eq!(
            request_body.as_ref().and_then(|body| body.get("tools")),
            Some(&json!([]))
        );
        assert!(result.warnings.iter().any(|warning| matches!(
            warning,
            Warning::Unsupported { feature, details: None }
                if feature == "provider-defined tool some.unsupported_tool"
        )));

        for supported_model in ["openai/gpt-oss-20b", "openai/gpt-oss-120b"] {
            let (result, request_body) = groq_request_body_for_options(
                supported_model,
                LanguageModelCallOptions::new(groq_prompt()).with_tool(
                    LanguageModelTool::Provider(LanguageModelProviderTool::new(
                        "groq.browser_search",
                        "browser_search",
                        JsonObject::new(),
                    )),
                ),
            );
            assert!(result.warnings.is_empty());
            assert_eq!(
                request_body.as_ref().and_then(|body| body.get("tools")),
                Some(&json!([
                    {
                        "type": "browser_search"
                    }
                ]))
            );
        }

        let (result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(groq_prompt()).with_tool(LanguageModelTool::Provider(
                LanguageModelProviderTool::new(
                    "groq.browser_search",
                    "browser_search",
                    JsonObject::new(),
                ),
            )),
        );
        assert_eq!(
            request_body.as_ref().and_then(|body| body.get("tools")),
            Some(&json!([]))
        );
        assert!(result.warnings.iter().any(|warning| matches!(
            warning,
            Warning::Unsupported { feature, details }
                if feature == "provider-defined tool groq.browser_search"
                    && details.as_deref()
                        == Some("Browser search is only supported on the following models: openai/gpt-oss-20b, openai/gpt-oss-120b. Current model: gemma2-9b-it")
        )));

        let (_result, request_body) = groq_request_body_for_options(
            "openai/gpt-oss-20b",
            LanguageModelCallOptions::new(groq_prompt())
                .with_tool(LanguageModelTool::Function(
                    LanguageModelFunctionTool::new("test-tool", empty_schema())
                        .with_description("A test tool"),
                ))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "groq.browser_search",
                    "browser_search",
                    JsonObject::new(),
                )))
                .with_tool_choice(LanguageModelToolChoice::Required),
        );
        assert_eq!(
            request_body.as_ref().and_then(|body| body.get("tools")),
            Some(&json!([
                {
                    "type": "function",
                    "function": {
                        "name": "test-tool",
                        "description": "A test tool",
                        "parameters": {
                            "type": "object",
                            "properties": {}
                        }
                    }
                },
                {
                    "type": "browser_search"
                }
            ]))
        );
        assert_eq!(
            request_body
                .as_ref()
                .and_then(|body| body.get("tool_choice")),
            Some(&json!("required"))
        );
    }

    #[test]
    fn groq_chat_converts_messages_images_reasoning_tool_results_and_rejects_provider_refs() {
        let (_result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                LanguageModelUserMessage::new(vec![
                    LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
                    LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                        FileData::Data {
                            data: FileDataContent::Base64("AAECAw==".to_string()),
                        },
                        "image/png",
                    )),
                ]),
            )]),
        );
        assert_eq!(
            request_body.as_ref().and_then(|body| body.get("messages")),
            Some(&json!([
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "Hello"
                        },
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": "data:image/png;base64,AAECAw=="
                            }
                        }
                    ]
                }
            ]))
        );

        let (_result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::File(
                    LanguageModelFilePart::new(
                        FileData::Data {
                            data: FileDataContent::Bytes(vec![
                                0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n',
                            ]),
                        },
                        "image",
                    ),
                )]),
            )]),
        );
        assert_eq!(
            request_body
                .as_ref()
                .and_then(|body| body.get("messages"))
                .and_then(|messages| messages.get(0))
                .and_then(|message| message.get("content"))
                .and_then(|content| content.get(0)),
            Some(&json!({
                "type": "image_url",
                "image_url": {
                    "url": "data:image/png;base64,iVBORw0KGgo="
                }
            }))
        );

        let (_result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::File(
                    LanguageModelFilePart::new(
                        FileData::Url {
                            url: Url::parse("https://example.com/x.png").expect("url parses"),
                        },
                        "image",
                    ),
                )]),
            )]),
        );
        assert_eq!(
            request_body
                .as_ref()
                .and_then(|body| body.get("messages"))
                .and_then(|messages| messages.get(0))
                .and_then(|message| message.get("content"))
                .and_then(|content| content.get(0)),
            Some(&json!({
                "type": "image_url",
                "image_url": {
                    "url": "https://example.com/x.png"
                }
            }))
        );

        let (_result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(vec![
                LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                    LanguageModelAssistantContentPart::Reasoning(LanguageModelReasoningPart::new(
                        "I think the tool will return the correct value.",
                    )),
                    LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                        "quux",
                        "thwomp",
                        json!({
                            "foo": "bar123"
                        }),
                    )),
                ])),
                LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                    LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                        "quux",
                        "thwomp",
                        LanguageModelToolResultOutput::json(json!({
                            "oof": "321rab"
                        })),
                    )),
                ])),
            ]),
        );
        assert_eq!(
            request_body.as_ref().and_then(|body| body.get("messages")),
            Some(&json!([
                {
                    "role": "assistant",
                    "content": "",
                    "reasoning": "I think the tool will return the correct value.",
                    "tool_calls": [
                        {
                            "id": "quux",
                            "type": "function",
                            "function": {
                                "name": "thwomp",
                                "arguments": "{\"foo\":\"bar123\"}"
                            }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "content": "{\"oof\":\"321rab\"}",
                    "tool_call_id": "quux"
                }
            ]))
        );

        let (result, request_body) = groq_request_body_for_options(
            "gemma2-9b-it",
            LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::File(
                    LanguageModelFilePart::new(
                        FileData::Reference {
                            reference: ProviderReference::from_map(BTreeMap::from([(
                                "groq".to_string(),
                                "file-ref-123".to_string(),
                            )]))
                            .expect("provider reference is valid"),
                        },
                        "image/png",
                    ),
                )]),
            )]),
        );
        assert!(request_body.is_none());
        assert_eq!(result.finish_reason.unified, FinishReason::Error);
        assert!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("groq"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str)
                .is_some_and(|message| message.contains("file parts with provider references"))
        );
    }

    #[test]
    fn groq_chat_converts_usage_with_reasoning_edges() {
        let default_usage = groq_chat_usage(None);
        assert_eq!(default_usage.input_tokens.total, None);
        assert_eq!(default_usage.output_tokens.text, None);
        assert_eq!(default_usage.raw, None);

        let basic = groq_chat_usage(Some(&json!({
            "prompt_tokens": 20,
            "completion_tokens": 10
        })));
        assert_eq!(basic.input_tokens.total, Some(20));
        assert_eq!(basic.input_tokens.no_cache, Some(20));
        assert_eq!(basic.input_tokens.cache_read, None);
        assert_eq!(basic.output_tokens.total, Some(10));
        assert_eq!(basic.output_tokens.text, Some(10));
        assert_eq!(basic.output_tokens.reasoning, None);

        for (reasoning_value, expected_reasoning, expected_text) in [
            (json!(21), Some(21), Some(19)),
            (JsonValue::Null, None, Some(40)),
            (json!(0), Some(0), Some(40)),
            (json!(40), Some(40), Some(0)),
        ] {
            let usage = groq_chat_usage(Some(&json!({
                "prompt_tokens": 79,
                "completion_tokens": 40,
                "completion_tokens_details": {
                    "reasoning_tokens": reasoning_value
                }
            })));
            assert_eq!(usage.output_tokens.reasoning, expected_reasoning);
            assert_eq!(usage.output_tokens.text, expected_text);
        }

        let null_details = groq_chat_usage(Some(&json!({
            "prompt_tokens": 20,
            "completion_tokens": 10,
            "completion_tokens_details": null
        })));
        assert_eq!(null_details.output_tokens.reasoning, None);
        assert_eq!(null_details.output_tokens.text, Some(10));

        let missing = groq_chat_usage(Some(&json!({})));
        assert_eq!(missing.input_tokens.total, Some(0));
        assert_eq!(missing.input_tokens.no_cache, Some(0));
        assert_eq!(missing.output_tokens.total, Some(0));
        assert_eq!(missing.output_tokens.text, Some(0));
        assert_eq!(missing.output_tokens.reasoning, None);
    }

    #[test]
    fn groq_chat_do_stream_maps_text_reasoning_tools_usage_raw_chunks_and_errors() {
        let stream = concat!(
            "data: {\"id\":\"chatcmpl-stream\",\"created\":1711115037,\"model\":\"gemma2-9b-it\",\"choices\":[{\"index\":0,\"delta\":{\"reasoning\":\"Think\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-stream\",\"created\":1711115037,\"model\":\"gemma2-9b-it\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello \"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-stream\",\"created\":1711115037,\"model\":\"gemma2-9b-it\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{\\\"city\\\"\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-stream\",\"created\":1711115037,\"model\":\"gemma2-9b-it\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"Brisbane\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chatcmpl-stream\",\"created\":1711115037,\"model\":\"gemma2-9b-it\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"x_groq\":{\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":30,\"completion_tokens_details\":{\"reasoning_tokens\":10}}}}\n\n",
            "data: [DONE]\n\n",
        );
        let (model, captured_requests) = groq_model_with_stream("gemma2-9b-it", stream);
        let result = poll_ready(
            model.do_stream(
                LanguageModelCallOptions::new(groq_prompt())
                    .with_header("x-call", "stream")
                    .with_include_raw_chunks(true),
            ),
        );

        assert!(result.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::Raw(raw)
                if raw.raw_value
                    .get("id")
                    .and_then(JsonValue::as_str)
                    == Some("chatcmpl-stream")
        )));
        assert!(result.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::ReasoningDelta(reasoning)
                if reasoning.delta == "Think"
        )));
        assert!(result.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::TextDelta(text) if text.delta == "Hello "
        )));
        assert!(result.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::ToolInputStart(tool)
                if tool.id == "call_1" && tool.tool_name == "lookup"
        )));
        assert!(result.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::ToolCall(tool)
                if tool.tool_call_id == "call_1"
                    && tool.tool_name == "lookup"
                    && tool.input == "{\"city\":\"Brisbane\"}"
        )));
        let finish = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => Some(finish),
                _ => None,
            })
            .expect("finish part is emitted");
        assert_eq!(finish.finish_reason.unified, FinishReason::ToolCalls);
        assert_eq!(finish.usage.input_tokens.total, Some(12));
        assert_eq!(finish.usage.input_tokens.no_cache, Some(12));
        assert_eq!(finish.usage.input_tokens.cache_read, None);
        assert_eq!(finish.usage.output_tokens.total, Some(30));
        assert_eq!(finish.usage.output_tokens.text, Some(20));
        assert_eq!(finish.usage.output_tokens.reasoning, Some(10));

        let request_body = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned")[0]
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(request_body["stream"], true);
        let captured_requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned");
        let request = &captured_requests[0];
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("x-call").map(String::as_str),
            Some("stream")
        );
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_groq_stream")
        );
        drop(captured_requests);

        let (model, _captured_requests) = groq_model_with_stream(
            "gemma2-9b-it",
            concat!(
                "data: {\"id\":\"chatcmpl-one\",\"created\":1711115037,\"model\":\"gemma2-9b-it\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_one\",\"type\":\"function\",\"function\":{\"name\":\"lookup\",\"arguments\":\"{\\\"value\\\":\\\"Sparkle Day\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
                "data: {\"id\":\"chatcmpl-one\",\"created\":1711115037,\"model\":\"gemma2-9b-it\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"x_groq\":{\"usage\":{\"prompt_tokens\":1,\"completion_tokens\":1}}}\n\n",
                "data: [DONE]\n\n",
            ),
        );
        let one_chunk = poll_ready(model.do_stream(LanguageModelCallOptions::new(groq_prompt())));
        assert!(one_chunk.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::ToolCall(tool)
                if tool.tool_call_id == "call_one"
                    && tool.tool_name == "lookup"
                    && tool.input == "{\"value\":\"Sparkle Day\"}"
        )));

        let (model, _captured_requests) = groq_model_with_stream(
            "gemma2-9b-it",
            concat!(
                "data: {\"id\":\"chatcmpl-duplicate\",\"created\":1711115037,\"model\":\"gemma2-9b-it\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_duplicate\",\"type\":\"function\",\"function\":{\"name\":\"searchGoogle\"}}]},\"finish_reason\":null}],\"usage\":{\"prompt_tokens\":226,\"completion_tokens\":7}}\n\n",
                "data: {\"id\":\"chatcmpl-duplicate\",\"created\":1711115037,\"model\":\"gemma2-9b-it\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"query\\\": \\\"latest news on ai\\\"}\"}}]},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":226,\"completion_tokens\":20}}\n\n",
                "data: {\"id\":\"chatcmpl-duplicate\",\"created\":1711115037,\"model\":\"gemma2-9b-it\",\"choices\":[],\"usage\":{\"prompt_tokens\":226,\"completion_tokens\":20}}\n\n",
                "data: [DONE]\n\n",
            ),
        );
        let duplicate = poll_ready(model.do_stream(LanguageModelCallOptions::new(groq_prompt())));
        let duplicate_tool_calls = duplicate
            .stream
            .iter()
            .filter(|part| matches!(part, LanguageModelStreamPart::ToolCall(_)))
            .count();
        assert_eq!(duplicate_tool_calls, 1);

        let (model, _captured_requests) = groq_model_with_stream(
            "gemma2-9b-it",
            concat!(
                "data: {\"error\":{\"message\":\"Incorrect API key provided\",\"type\":\"invalid_request_error\"}}\n\n",
                "data: [DONE]\n\n",
            ),
        );
        let error_result =
            poll_ready(model.do_stream(LanguageModelCallOptions::new(groq_prompt())));
        assert!(error_result.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::Error(error)
                if error.error.as_str() == Some("Incorrect API key provided")
        )));
        let finish = error_result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => Some(finish),
                _ => None,
            })
            .expect("finish part is emitted");
        assert_eq!(finish.finish_reason.unified, FinishReason::Error);
    }

    #[test]
    fn groq_chat_streaming_maps_unparsable_stream_parts() {
        let (model, _captured_requests) =
            groq_model_with_stream("gemma2-9b-it", "data: {unparsable}\n\ndata: [DONE]\n\n");

        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(groq_prompt())));

        assert!(matches!(
            result.stream.first(),
            Some(LanguageModelStreamPart::StreamStart(start)) if start.warnings.is_empty()
        ));
        assert!(result.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::Error(error)
                if error
                    .error
                    .get("message")
                    .and_then(JsonValue::as_str)
                    .is_some_and(|message| message.contains("JSON parsing failed"))
        )));
        let finish = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => Some(finish),
                _ => None,
            })
            .expect("finish part is emitted");
        assert_eq!(finish.finish_reason.unified, FinishReason::Error);
        assert_eq!(finish.usage, Default::default());
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
    fn groq_transcription_uses_custom_current_date_and_maps_errors() {
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
                        "text": "dated transcript"
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_dated_transcription".to_string(),
                )])))))
            });
        let provider = create_groq(
            GroqProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.groq.test/openai/v1"),
        )
        .with_transport(transport)
        .with_current_date(|| OffsetDateTime::from_unix_timestamp(0).expect("epoch is valid"));
        let result = poll_ready(
            provider
                .transcription("whisper-large-v3-turbo")
                .do_generate(TranscriptionModelCallOptions::new(
                    FileDataContent::Bytes(vec![1, 2, 3]),
                    "audio/wav",
                )),
        );

        assert_eq!(result.text, "dated transcript");
        assert_eq!(
            result.response.timestamp,
            OffsetDateTime::from_unix_timestamp(0).expect("epoch is valid")
        );
        assert_eq!(result.response.model_id, "whisper-large-v3-turbo");
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_dated_transcription")
        );
        assert!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .is_some()
        );

        let transport: OpenAICompatibleTransport =
            Arc::new(move |_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    400,
                    "Bad Request",
                    json!({
                        "error": {
                            "message": "Invalid file format",
                            "type": "invalid_request_error"
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_failed_transcription".to_string(),
                )])))))
            });
        let provider = create_groq(
            GroqProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.groq.test/openai/v1"),
        )
        .with_transport(transport);
        let result = poll_ready(
            provider
                .transcription("whisper-large-v3-turbo")
                .do_generate(TranscriptionModelCallOptions::new(
                    FileDataContent::Bytes(vec![1, 2, 3]),
                    "audio/wav",
                )),
        );

        assert_eq!(result.text, "");
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("groq"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("Invalid file format")
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_failed_transcription")
        );
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

    #[test]
    #[ignore = "requires GROQ_API_KEY and performs live Groq chat/browser-search/transcription calls"]
    fn live_groq_chat_tools_and_transcription_validate_provider_contract() {
        if env::var("GROQ_API_KEY").is_err() {
            eprintln!("skipping live Groq test: GROQ_API_KEY is not set");
            return;
        }

        let provider = GroqProvider::new();
        let chat_model_id = env::var("AI_SDK_RUST_GROQ_CHAT_MODEL")
            .or_else(|_| env::var("GROQ_CHAT_MODEL"))
            .unwrap_or_else(|_| "llama-3.1-8b-instant".to_string());
        let chat =
            poll_ready(provider.language_model(chat_model_id.clone()).do_generate(
                LanguageModelCallOptions::new(groq_prompt()).with_max_output_tokens(24),
            ));
        assert_ne!(chat.finish_reason.unified, FinishReason::Error);
        assert!(
            chat.content.iter().any(|part| matches!(
                part,
                LanguageModelContent::Text(text) if !text.text.trim().is_empty()
            )),
            "live chat call returned no text content: {:?}",
            chat
        );
        assert_eq!(
            chat.response
                .as_ref()
                .and_then(|response| response.model_id.as_deref()),
            Some(chat_model_id.as_str())
        );

        let streamed = poll_ready(
            provider.language_model(chat_model_id).do_stream(
                LanguageModelCallOptions::new(groq_prompt())
                    .with_max_output_tokens(24)
                    .with_include_raw_chunks(true),
            ),
        );
        assert!(
            streamed
                .stream
                .iter()
                .all(|part| !matches!(part, LanguageModelStreamPart::Error(_))),
            "live stream returned an error part: {:?}",
            streamed.stream
        );
        assert!(streamed.stream.iter().any(|part| matches!(
            part,
            LanguageModelStreamPart::TextDelta(text) if !text.delta.is_empty()
        )));
        assert!(
            streamed
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::Raw(_)))
        );

        let browser_model_id = env::var("AI_SDK_RUST_GROQ_BROWSER_SEARCH_MODEL")
            .or_else(|_| env::var("GROQ_BROWSER_SEARCH_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-oss-20b".to_string());
        let browser = poll_ready(
            provider.language_model(browser_model_id).do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new(
                            "Find one current headline and answer in one sentence.",
                        ),
                    )]),
                )])
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "groq.browser_search",
                    "browser_search",
                    JsonObject::new(),
                )))
                .with_max_output_tokens(64),
            ),
        );
        assert_ne!(browser.finish_reason.unified, FinishReason::Error);

        let transcription_model_id = env::var("AI_SDK_RUST_GROQ_TRANSCRIPTION_MODEL")
            .or_else(|_| env::var("GROQ_TRANSCRIPTION_MODEL"))
            .unwrap_or_else(|_| "whisper-large-v3-turbo".to_string());
        let transcription = poll_ready(
            provider
                .transcription(transcription_model_id.clone())
                .do_generate(TranscriptionModelCallOptions::new(
                    FileDataContent::Bytes(silent_wav_bytes()),
                    "audio/wav",
                )),
        );
        assert!(
            transcription.provider_metadata.is_none(),
            "live transcription returned provider metadata: {:?}",
            transcription.provider_metadata
        );
        assert_eq!(transcription.response.model_id, transcription_model_id);
    }

    fn silent_wav_bytes() -> Vec<u8> {
        let sample_rate = 8_000u32;
        let sample_count = sample_rate / 2;
        let data_size = sample_count * 2;
        let riff_size = 36 + data_size;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&riff_size.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_size.to_le_bytes());
        bytes.resize(bytes.len() + data_size as usize, 0);
        bytes
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
