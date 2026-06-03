use std::collections::BTreeMap;
use std::env;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use url::Url;

use crate::file_data::{FileDataContent, ProviderReference};
use crate::files::{Files, FilesUploadFileCallOptions, FilesUploadFileData, FilesUploadFileResult};
use crate::headers::Headers;
use crate::image_model::{
    ImageModel, ImageModelCallOptions, ImageModelFile, ImageModelProviderMetadata,
    ImageModelProviderMetadataEntry, ImageModelResponse, ImageModelResult,
};
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    FinishReason, InputTokenUsage, LanguageModel, LanguageModelCallOptions, LanguageModelContent,
    LanguageModelFinishReason, LanguageModelGenerateResult, LanguageModelProviderTool,
    LanguageModelSource, LanguageModelStreamPart, LanguageModelStreamResult,
    LanguageModelStreamStart, LanguageModelSupportedUrls, LanguageModelTool, LanguageModelToolCall,
    LanguageModelToolChoice, LanguageModelUrlSource, LanguageModelUsage, OutputTokenUsage,
};
use crate::open_responses::{
    OpenResponsesLanguageModel, OpenResponsesProvider, OpenResponsesProviderSettings,
    OpenResponsesTransport,
};
use crate::openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleProvider,
    OpenAICompatibleProviderSettings, OpenAICompatibleTransport,
};
use crate::provider::{
    ApiCallError, ModelType, NoSuchModelError, Provider, ProviderMetadata, ProviderWithFiles,
    ProviderWithVideoModel, SpecificationVersion,
};
use crate::provider_utils::{
    DelayOptions, FetchErrorInfo, FormData, FormDataValue, GetFromApiOptions, HandledFetchError,
    PostFormDataToApiOptions, PostJsonToApiOptions, ProviderApiRequest, ProviderApiRequestBody,
    ProviderApiRequestMethod, ProviderApiResponse, ProviderApiResponseHandlerError,
    RuntimeEnvironment, combine_headers, convert_base64_to_bytes, convert_to_base64,
    create_json_error_response_handler, create_json_response_handler, delay_with_options,
    post_form_data_to_api, post_json_to_api, without_trailing_slash,
};
use crate::video_model::{
    VideoModel, VideoModelCallOptions, VideoModelFile, VideoModelResponse, VideoModelResult,
    VideoModelVideoData,
};
use crate::warning::Warning;

/// Default base URL for upstream `@ai-sdk/xai` API calls.
pub const DEFAULT_XAI_BASE_URL: &str = "https://api.x.ai/v1";

/// Settings for the upstream xAI provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XaiProviderSettings {
    /// Base URL for xAI API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// xAI API key. When omitted, `XAI_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl XaiProviderSettings {
    /// Creates empty xAI provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the xAI API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the xAI API key.
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

/// Upstream xAI provider foundation.
#[derive(Clone)]
pub struct XaiProvider {
    settings: XaiProviderSettings,
    openai_transport: Option<OpenAICompatibleTransport>,
    responses_transport: Option<OpenResponsesTransport>,
    responses_request_context: Arc<Mutex<Option<XaiResponsesRequestContext>>>,
}

impl XaiProvider {
    /// Creates an xAI provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(XaiProviderSettings::new())
    }

    /// Creates a provider from explicit xAI settings.
    pub fn from_settings(settings: XaiProviderSettings) -> Self {
        Self {
            settings,
            openai_transport: None,
            responses_transport: None,
            responses_request_context: Arc::new(Mutex::new(None)),
        }
    }

    /// Sets the xAI API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the xAI API base URL for this provider.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.settings.base_url = Some(base_url.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    /// Replaces the OpenAI-compatible HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: OpenAICompatibleTransport) -> Self {
        self.openai_transport = Some(transport);
        self
    }

    /// Replaces the Responses API HTTP transport. This is primarily useful for tests.
    pub fn with_responses_transport(mut self, transport: OpenResponsesTransport) -> Self {
        self.responses_transport = Some(transport);
        self
    }

    /// Creates the default xAI Responses language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> XaiResponsesLanguageModel {
        self.responses(model_id)
    }

    /// Creates an xAI Responses language model.
    pub fn responses(&self, model_id: impl Into<String>) -> XaiResponsesLanguageModel {
        XaiResponsesLanguageModel::new(
            self.open_responses_provider().language_model(model_id),
            Arc::clone(&self.responses_request_context),
        )
    }

    /// Creates an xAI chat language model.
    pub fn chat_model(&self, model_id: impl Into<String>) -> XaiChatLanguageModel {
        XaiChatLanguageModel::new(self.openai_compatible_provider().chat_model(model_id))
    }

    /// Alias for [`XaiProvider::chat_model`].
    pub fn chat(&self, model_id: impl Into<String>) -> XaiChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Reports that xAI does not expose embedding models through this Rust slice.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Deprecated upstream alias for [`XaiProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Creates an xAI image model.
    pub fn image_model(&self, model_id: impl Into<String>) -> XaiImageModel {
        XaiImageModel::new(
            model_id,
            xai_base_url(&self.settings),
            xai_provider_headers(&self.settings),
            self.openai_transport
                .as_ref()
                .cloned()
                .unwrap_or_else(default_xai_transport),
        )
    }

    /// Alias for [`XaiProvider::image_model`].
    pub fn image(&self, model_id: impl Into<String>) -> XaiImageModel {
        self.image_model(model_id)
    }

    /// Creates an xAI video model.
    pub fn video_model(&self, model_id: impl Into<String>) -> XaiVideoModel {
        XaiVideoModel::new(
            model_id,
            xai_base_url(&self.settings),
            xai_provider_headers(&self.settings),
            self.openai_transport
                .as_ref()
                .cloned()
                .unwrap_or_else(default_xai_transport),
        )
    }

    /// Alias for [`XaiProvider::video_model`].
    pub fn video(&self, model_id: impl Into<String>) -> XaiVideoModel {
        self.video_model(model_id)
    }

    /// Returns the xAI Files API interface.
    pub fn files(&self) -> XaiFiles {
        XaiFiles::new(
            xai_base_url(&self.settings),
            xai_provider_headers(&self.settings),
            self.openai_transport
                .as_ref()
                .cloned()
                .unwrap_or_else(default_xai_transport),
        )
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings =
            OpenAICompatibleProviderSettings::new("xai", xai_base_url(&self.settings))
                .with_include_usage(true)
                .with_supports_structured_outputs(true)
                .with_user_agent_suffix(format!("ai-sdk/xai/{}", crate::VERSION))
                .with_transform_request_body(transform_xai_chat_request_body)
                .with_error_to_message(xai_error_to_message);

        if let Some(api_key) = xai_api_key(self.settings.api_key.as_ref()) {
            settings = settings.with_api_key(api_key);
        }

        for (name, value) in &self.settings.headers {
            settings = settings.with_header(name.clone(), value.clone());
        }

        let provider = OpenAICompatibleProvider::from_settings(settings);

        if let Some(transport) = &self.openai_transport {
            provider.with_transport(Arc::clone(transport))
        } else {
            provider
        }
    }

    fn open_responses_provider(&self) -> OpenResponsesProvider {
        let mut settings = OpenResponsesProviderSettings::new(
            "xai",
            format!("{}/responses", xai_base_url(&self.settings)),
        )
        .with_user_agent_suffix(format!("ai-sdk/xai/{}", crate::VERSION))
        .with_file_id_prefix("file-");

        if let Some(api_key) = xai_api_key(self.settings.api_key.as_ref()) {
            settings = settings.with_api_key(api_key);
        }

        for (name, value) in &self.settings.headers {
            settings = settings.with_header(name.clone(), value.clone());
        }

        let provider = OpenResponsesProvider::from_settings(settings);
        let transport = self
            .responses_transport
            .as_ref()
            .cloned()
            .unwrap_or_else(default_xai_transport);
        let transport = xai_responses_transforming_transport(
            transport,
            Arc::clone(&self.responses_request_context),
        );

        provider.with_transport(transport)
    }
}

impl Default for XaiProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for XaiProvider {
    type LanguageModel = XaiResponsesLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = XaiImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(XaiProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        XaiProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        Ok(XaiProvider::image_model(self, model_id))
    }
}

impl ProviderWithFiles for XaiProvider {
    type Files = XaiFiles;

    fn files(&self) -> Self::Files {
        XaiProvider::files(self)
    }
}

impl ProviderWithVideoModel for XaiProvider {
    type VideoModel = XaiVideoModel;

    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
        Ok(XaiProvider::video_model(self, model_id))
    }
}

#[derive(Clone)]
pub struct XaiChatLanguageModel {
    inner: OpenAICompatibleChatLanguageModel,
}

impl XaiChatLanguageModel {
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
}

impl LanguageModel for XaiChatLanguageModel {
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

    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    fn provider(&self) -> &str {
        self.inner.provider()
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        self.inner.supported_urls()
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        let original_options = options.clone();
        Box::pin(async move {
            let mut result = self.inner.do_generate(options).await;
            result.usage = xai_chat_usage_from_raw(result.usage.raw.as_ref());
            xai_add_chat_citations(&mut result);
            xai_append_chat_warnings(&mut result.warnings, &original_options);
            result
        })
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        let original_options = options.clone();
        Box::pin(async move {
            let mut result = self.inner.do_stream(options).await;
            xai_adjust_stream_usage(&mut result.stream, xai_chat_usage_from_raw);
            xai_append_stream_start_warnings(&mut result.stream, &original_options);
            result
        })
    }
}

#[derive(Clone)]
pub struct XaiResponsesLanguageModel {
    inner: OpenResponsesLanguageModel,
    request_context: Arc<Mutex<Option<XaiResponsesRequestContext>>>,
}

impl XaiResponsesLanguageModel {
    fn new(
        inner: OpenResponsesLanguageModel,
        request_context: Arc<Mutex<Option<XaiResponsesRequestContext>>>,
    ) -> Self {
        Self {
            inner,
            request_context,
        }
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        self.inner.provider()
    }
}

impl LanguageModel for XaiResponsesLanguageModel {
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
        self.inner.provider()
    }

    fn model_id(&self) -> &str {
        self.inner.model_id()
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(xai_responses_supported_urls())
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        let context = XaiResponsesRequestContext::from_options(&options);
        let delegate_options = xai_responses_delegate_options(options);
        let request_context = Arc::clone(&self.request_context);

        Box::pin(async move {
            *request_context
                .lock()
                .expect("xAI responses request context mutex is not poisoned") = Some(context);

            let mut result = self.inner.do_generate(delegate_options).await;
            result.usage = xai_responses_usage_from_raw(result.usage.raw.as_ref());
            xai_responses_replace_content(&mut result);
            xai_responses_add_provider_metadata(&mut result);
            result
        })
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        let context = XaiResponsesRequestContext::from_options(&options);
        let delegate_options = xai_responses_delegate_options(options);
        let request_context = Arc::clone(&self.request_context);

        Box::pin(async move {
            *request_context
                .lock()
                .expect("xAI responses request context mutex is not poisoned") = Some(context);

            let mut result = self.inner.do_stream(delegate_options).await;
            xai_adjust_stream_usage(&mut result.stream, xai_responses_usage_from_raw);
            result
        })
    }
}

#[derive(Clone)]
struct XaiResponsesRequestContext {
    tools: Option<Vec<LanguageModelTool>>,
    tool_choice: Option<LanguageModelToolChoice>,
}

impl XaiResponsesRequestContext {
    fn from_options(options: &LanguageModelCallOptions) -> Self {
        Self {
            tools: options.tools.clone(),
            tool_choice: options.tool_choice.clone(),
        }
    }
}

#[derive(Clone)]
pub struct XaiImageModel {
    model_id: String,
    base_url: String,
    headers: BTreeMap<String, Option<String>>,
    transport: OpenAICompatibleTransport,
}

impl XaiImageModel {
    fn new(
        model_id: impl Into<String>,
        base_url: String,
        headers: BTreeMap<String, Option<String>>,
        transport: OpenAICompatibleTransport,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            base_url,
            headers,
            transport,
        }
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        "xai.image"
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }
}

impl ImageModel for XaiImageModel {
    type MaxImagesPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    fn provider(&self) -> &str {
        "xai.image"
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
        ready(Some(3))
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(async move { self.do_generate_result(options).await })
    }
}

#[derive(Clone)]
pub struct XaiFiles {
    base_url: String,
    headers: BTreeMap<String, Option<String>>,
    transport: OpenAICompatibleTransport,
}

impl XaiFiles {
    fn new(
        base_url: String,
        headers: BTreeMap<String, Option<String>>,
        transport: OpenAICompatibleTransport,
    ) -> Self {
        Self {
            base_url,
            headers,
            transport,
        }
    }
}

impl Files for XaiFiles {
    type UploadFileFuture<'a>
        = Pin<Box<dyn Future<Output = FilesUploadFileResult> + Send + 'a>>
    where
        Self: 'a;

    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    fn provider(&self) -> &str {
        "xai.files"
    }

    fn upload_file(&self, options: FilesUploadFileCallOptions) -> Self::UploadFileFuture<'_> {
        Box::pin(async move { self.upload_file_result(options).await })
    }
}

#[derive(Clone)]
pub struct XaiVideoModel {
    model_id: String,
    base_url: String,
    headers: BTreeMap<String, Option<String>>,
    transport: OpenAICompatibleTransport,
}

impl XaiVideoModel {
    fn new(
        model_id: impl Into<String>,
        base_url: String,
        headers: BTreeMap<String, Option<String>>,
        transport: OpenAICompatibleTransport,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            base_url,
            headers,
            transport,
        }
    }

    /// Returns the provider id for this model.
    pub fn provider(&self) -> &str {
        "xai.video"
    }

    /// Returns the provider-specific model id.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }
}

impl VideoModel for XaiVideoModel {
    type MaxVideosPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = VideoModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    fn provider(&self) -> &str {
        "xai.video"
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn max_videos_per_call(&self) -> Self::MaxVideosPerCallFuture<'_> {
        ready(Some(1))
    }

    fn do_generate(&self, options: VideoModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(async move { self.do_generate_result(options).await })
    }
}

/// Creates an xAI provider with explicit settings.
pub fn create_xai(settings: XaiProviderSettings) -> XaiProvider {
    XaiProvider::from_settings(settings)
}

/// Creates an xAI Responses language model using default provider settings.
pub fn xai(model_id: impl Into<String>) -> XaiResponsesLanguageModel {
    XaiProvider::new().language_model(model_id)
}

fn xai_base_url(settings: &XaiProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_XAI_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn xai_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("XAI_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn xai_provider_headers(settings: &XaiProviderSettings) -> BTreeMap<String, Option<String>> {
    let mut headers = BTreeMap::new();

    if let Some(api_key) = xai_api_key(settings.api_key.as_ref()) {
        headers.insert(
            "Authorization".to_string(),
            Some(format!("Bearer {api_key}")),
        );
    }

    for (name, value) in &settings.headers {
        headers.insert(name.clone(), Some(value.clone()));
    }

    headers.insert(
        "user-agent".to_string(),
        Some(format!("ai-sdk/xai/{}", crate::VERSION)),
    );
    headers
}

fn xai_call_headers(
    provider_headers: &BTreeMap<String, Option<String>>,
    call_headers: Option<&Headers>,
) -> BTreeMap<String, Option<String>> {
    let call_headers = call_headers.map(|headers| {
        headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone())))
            .collect::<Vec<_>>()
    });

    combine_headers([
        Some(provider_headers.clone().into_iter().collect::<Vec<_>>()),
        call_headers,
    ])
}

fn xai_error_to_message(error: &JsonValue) -> Option<String> {
    error
        .get("error")
        .and_then(|error| error.get("message").or_else(|| error.get("error")))
        .and_then(JsonValue::as_str)
        .or_else(|| error.get("message").and_then(JsonValue::as_str))
        .or_else(|| error.get("detail").and_then(JsonValue::as_str))
        .map(ToString::to_string)
}

fn transform_xai_chat_request_body(body: JsonValue) -> JsonValue {
    let JsonValue::Object(mut body) = body else {
        return body;
    };

    if let Some(max_tokens) = body.remove("max_tokens") {
        body.entry("max_completion_tokens".to_string())
            .or_insert(max_tokens);
    }

    body.remove("frequency_penalty");
    body.remove("presence_penalty");
    body.remove("stop");

    if let Some(reasoning_effort) = body.get("reasoning_effort").and_then(JsonValue::as_str) {
        if let Some(effort) = xai_reasoning_effort(reasoning_effort) {
            body.insert("reasoning_effort".to_string(), JsonValue::String(effort));
        } else {
            body.remove("reasoning_effort");
        }
    }

    if let Some(search_parameters) = body.remove("searchParameters") {
        body.insert(
            "search_parameters".to_string(),
            xai_search_parameters(search_parameters),
        );
    }

    if let Some(top_logprobs) = body.remove("topLogprobs") {
        body.insert("top_logprobs".to_string(), top_logprobs);
    }
    if body.contains_key("top_logprobs") && !body.contains_key("logprobs") {
        body.insert("logprobs".to_string(), JsonValue::Bool(true));
    }

    if let Some(tools) = body.remove("tools") {
        body.insert(
            "tools".to_string(),
            xai_remove_additional_properties_false(tools),
        );
    }

    JsonValue::Object(body)
}

fn xai_reasoning_effort(value: &str) -> Option<String> {
    match value {
        "minimal" | "low" => Some("low".to_string()),
        "medium" => Some("medium".to_string()),
        "high" | "xhigh" => Some("high".to_string()),
        "none" | "provider-default" => None,
        other => Some(other.to_string()),
    }
}

fn xai_search_parameters(value: JsonValue) -> JsonValue {
    let JsonValue::Object(search_parameters) = value else {
        return value;
    };

    let mut mapped = JsonObject::new();
    xai_insert_mapped(&mut mapped, &search_parameters, "mode", "mode");
    xai_insert_mapped(
        &mut mapped,
        &search_parameters,
        "return_citations",
        "returnCitations",
    );
    xai_insert_mapped(&mut mapped, &search_parameters, "from_date", "fromDate");
    xai_insert_mapped(&mut mapped, &search_parameters, "to_date", "toDate");
    xai_insert_mapped(
        &mut mapped,
        &search_parameters,
        "max_search_results",
        "maxSearchResults",
    );

    if let Some(sources) = search_parameters
        .get("sources")
        .and_then(JsonValue::as_array)
    {
        mapped.insert(
            "sources".to_string(),
            JsonValue::Array(
                sources
                    .iter()
                    .filter_map(|source| source.as_object())
                    .map(|source| {
                        let mut mapped_source = JsonObject::new();
                        xai_insert_mapped(&mut mapped_source, source, "type", "type");
                        xai_insert_mapped(&mut mapped_source, source, "safe_search", "safeSearch");
                        xai_insert_mapped(
                            &mut mapped_source,
                            source,
                            "excluded_websites",
                            "excludedWebsites",
                        );
                        xai_insert_mapped(
                            &mut mapped_source,
                            source,
                            "allowed_websites",
                            "allowedWebsites",
                        );
                        xai_insert_mapped(&mut mapped_source, source, "x_handles", "xHandles");
                        xai_insert_mapped(&mut mapped_source, source, "country", "country");
                        JsonValue::Object(mapped_source)
                    })
                    .collect(),
            ),
        );
    }

    JsonValue::Object(mapped)
}

fn xai_insert_mapped(
    target: &mut JsonObject,
    source: &JsonObject,
    target_key: &str,
    source_key: &str,
) {
    if let Some(value) = source.get(source_key).filter(|value| !value.is_null()) {
        target.insert(target_key.to_string(), value.clone());
    }
}

fn xai_append_chat_warnings(warnings: &mut Vec<Warning>, options: &LanguageModelCallOptions) {
    if options.top_k.is_some()
        && !warnings
            .iter()
            .any(|warning| xai_warning_feature(warning) == Some("topK"))
    {
        warnings.push(Warning::Unsupported {
            feature: "topK".to_string(),
            details: None,
        });
    }
    if options.frequency_penalty.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "frequencyPenalty".to_string(),
            details: None,
        });
    }
    if options.presence_penalty.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "presencePenalty".to_string(),
            details: None,
        });
    }
    if options.stop_sequences.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "stopSequences".to_string(),
            details: None,
        });
    }
}

fn xai_warning_feature(warning: &Warning) -> Option<&str> {
    match warning {
        Warning::Unsupported { feature, .. } => Some(feature.as_str()),
        _ => None,
    }
}

fn xai_append_stream_start_warnings(
    stream: &mut Vec<LanguageModelStreamPart>,
    options: &LanguageModelCallOptions,
) {
    let mut warnings = Vec::new();
    xai_append_chat_warnings(&mut warnings, options);
    if warnings.is_empty() {
        return;
    }

    match stream.first_mut() {
        Some(LanguageModelStreamPart::StreamStart(start)) => {
            start.warnings.extend(warnings);
        }
        _ => stream.insert(
            0,
            LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(warnings)),
        ),
    }
}

fn xai_chat_usage_from_raw(raw: Option<&JsonObject>) -> LanguageModelUsage {
    let Some(raw) = raw else {
        return LanguageModelUsage::default();
    };

    let input_total = xai_json_u64(raw.get("prompt_tokens")).unwrap_or_default();
    let cache_read = raw
        .get("prompt_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    let completion_tokens = xai_json_u64(raw.get("completion_tokens")).unwrap_or_default();
    let reasoning_tokens = raw
        .get("completion_tokens_details")
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();

    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: Some(if cache_read > input_total {
                input_total + cache_read
            } else {
                input_total
            }),
            no_cache: Some(input_total.saturating_sub(cache_read)),
            cache_read: Some(cache_read),
            cache_write: None,
        },
        output_tokens: OutputTokenUsage {
            total: Some(completion_tokens + reasoning_tokens),
            text: Some(completion_tokens),
            reasoning: Some(reasoning_tokens),
        },
        raw: Some(raw.clone()),
    }
}

fn xai_responses_usage_from_raw(raw: Option<&JsonObject>) -> LanguageModelUsage {
    let Some(raw) = raw else {
        return LanguageModelUsage::default();
    };

    let input_tokens = xai_json_u64(raw.get("input_tokens")).unwrap_or_default();
    let cached_input_tokens = raw
        .get("input_tokens_details")
        .and_then(|details| details.get("cached_tokens"))
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();
    let output_tokens = xai_json_u64(raw.get("output_tokens")).unwrap_or_default();
    let reasoning_tokens = raw
        .get("output_tokens_details")
        .and_then(|details| details.get("reasoning_tokens"))
        .and_then(JsonValue::as_u64)
        .unwrap_or_default();

    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: Some(if cached_input_tokens > input_tokens {
                input_tokens + cached_input_tokens
            } else {
                input_tokens
            }),
            no_cache: Some(input_tokens.saturating_sub(cached_input_tokens)),
            cache_read: Some(cached_input_tokens),
            cache_write: None,
        },
        output_tokens: OutputTokenUsage {
            total: Some(output_tokens),
            text: Some(output_tokens.saturating_sub(reasoning_tokens)),
            reasoning: Some(reasoning_tokens),
        },
        raw: Some(raw.clone()),
    }
}

fn xai_json_u64(value: Option<&JsonValue>) -> Option<u64> {
    value.and_then(JsonValue::as_u64)
}

fn xai_adjust_stream_usage(
    stream: &mut [LanguageModelStreamPart],
    usage_from_raw: fn(Option<&JsonObject>) -> LanguageModelUsage,
) {
    for part in stream {
        if let LanguageModelStreamPart::Finish(finish) = part {
            finish.usage = usage_from_raw(finish.usage.raw.as_ref());
            if let Some(cost) = finish
                .usage
                .raw
                .as_ref()
                .and_then(|usage| usage.get("cost_in_usd_ticks"))
                .and_then(JsonValue::as_u64)
            {
                let mut metadata = finish.provider_metadata.take().unwrap_or_default();
                metadata
                    .entry("xai".to_string())
                    .or_default()
                    .insert("costInUsdTicks".to_string(), json!(cost));
                finish.provider_metadata = Some(metadata);
            }
        }
    }
}

fn xai_add_chat_citations(result: &mut LanguageModelGenerateResult) {
    let citations = result
        .response
        .as_ref()
        .and_then(|response| response.body.as_ref())
        .and_then(|body| body.get("citations"))
        .and_then(JsonValue::as_array);

    let Some(citations) = citations else {
        return;
    };

    for (index, citation) in citations.iter().enumerate() {
        let Some(url) = citation
            .as_str()
            .or_else(|| citation.get("url").and_then(JsonValue::as_str))
        else {
            continue;
        };
        let mut source = LanguageModelUrlSource::new(format!("source-{index}"), url);
        if let Some(title) = citation.get("title").and_then(JsonValue::as_str) {
            source = source.with_title(title);
        }
        result
            .content
            .push(LanguageModelContent::Source(LanguageModelSource::Url(
                source,
            )));
    }
}

fn xai_responses_supported_urls() -> LanguageModelSupportedUrls {
    BTreeMap::from([
        ("image/*".to_string(), vec!["^https?://.*".to_string()]),
        (
            "application/pdf".to_string(),
            vec!["^https?://.*".to_string()],
        ),
        ("text/*".to_string(), vec!["^https?://.*".to_string()]),
    ])
}

fn xai_responses_delegate_options(
    mut options: LanguageModelCallOptions,
) -> LanguageModelCallOptions {
    options.tools = None;
    options.tool_choice = None;
    options
}

fn xai_responses_transforming_transport(
    transport: OpenResponsesTransport,
    context: Arc<Mutex<Option<XaiResponsesRequestContext>>>,
) -> OpenResponsesTransport {
    Arc::new(move |request| {
        let transport = Arc::clone(&transport);
        let context = context
            .lock()
            .expect("xAI responses request context mutex is not poisoned")
            .clone();
        Box::pin(async move {
            let request = xai_responses_request_with_tools(request, context);
            transport(request).await
        })
    })
}

fn xai_responses_request_with_tools(
    mut request: ProviderApiRequest,
    context: Option<XaiResponsesRequestContext>,
) -> ProviderApiRequest {
    let Some(context) = context else {
        return request;
    };
    let Some(body) = request
        .body
        .as_ref()
        .and_then(ProviderApiRequestBody::as_text)
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
    else {
        return request;
    };
    let JsonValue::Object(mut body) = body else {
        return request;
    };

    let (tools, tool_choice) = xai_responses_prepare_tools(&context);
    if let Some(tools) = tools {
        body.insert("tools".to_string(), JsonValue::Array(tools));
    } else {
        body.remove("tools");
    }

    if let Some(tool_choice) = tool_choice {
        body.insert("tool_choice".to_string(), tool_choice);
    } else {
        body.remove("tool_choice");
    }

    let body = JsonValue::Object(body);
    request.request_body_values = body.clone();
    request.body = Some(ProviderApiRequestBody::text(body.to_string()));
    request
}

fn xai_responses_prepare_tools(
    context: &XaiResponsesRequestContext,
) -> (Option<Vec<JsonValue>>, Option<JsonValue>) {
    let Some(tools) = context.tools.as_ref().filter(|tools| !tools.is_empty()) else {
        return (None, None);
    };

    let prepared_tools = tools
        .iter()
        .filter_map(xai_responses_prepare_tool)
        .collect::<Vec<_>>();
    let tool_choice = xai_responses_tool_choice(context);

    (
        (!prepared_tools.is_empty()).then_some(prepared_tools),
        tool_choice,
    )
}

fn xai_responses_prepare_tool(tool: &LanguageModelTool) -> Option<JsonValue> {
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
                xai_remove_additional_properties_false(JsonValue::Object(
                    tool.input_schema.clone(),
                )),
            );
            if let Some(strict) = tool.strict {
                function.insert("strict".to_string(), JsonValue::Bool(strict));
            }
            Some(JsonValue::Object(function))
        }
        LanguageModelTool::Provider(tool) => xai_responses_prepare_provider_tool(tool),
    }
}

fn xai_responses_prepare_provider_tool(tool: &LanguageModelProviderTool) -> Option<JsonValue> {
    let mut prepared = JsonObject::new();
    match tool.id.as_str() {
        "xai.web_search" => {
            prepared.insert(
                "type".to_string(),
                JsonValue::String("web_search".to_string()),
            );
            xai_insert_mapped(
                &mut prepared,
                &tool.args,
                "allowed_domains",
                "allowedDomains",
            );
            xai_insert_mapped(
                &mut prepared,
                &tool.args,
                "excluded_domains",
                "excludedDomains",
            );
            xai_insert_mapped(
                &mut prepared,
                &tool.args,
                "enable_image_search",
                "enableImageSearch",
            );
            xai_insert_mapped(
                &mut prepared,
                &tool.args,
                "enable_image_understanding",
                "enableImageUnderstanding",
            );
        }
        "xai.x_search" => {
            prepared.insert(
                "type".to_string(),
                JsonValue::String("x_search".to_string()),
            );
            xai_insert_mapped(
                &mut prepared,
                &tool.args,
                "allowed_x_handles",
                "allowedXHandles",
            );
            xai_insert_mapped(
                &mut prepared,
                &tool.args,
                "excluded_x_handles",
                "excludedXHandles",
            );
            xai_insert_mapped(&mut prepared, &tool.args, "from_date", "fromDate");
            xai_insert_mapped(&mut prepared, &tool.args, "to_date", "toDate");
            xai_insert_mapped(
                &mut prepared,
                &tool.args,
                "enable_image_understanding",
                "enableImageUnderstanding",
            );
            xai_insert_mapped(
                &mut prepared,
                &tool.args,
                "enable_video_understanding",
                "enableVideoUnderstanding",
            );
        }
        "xai.code_execution" => {
            prepared.insert(
                "type".to_string(),
                JsonValue::String("code_interpreter".to_string()),
            );
        }
        "xai.view_image" => {
            prepared.insert(
                "type".to_string(),
                JsonValue::String("view_image".to_string()),
            );
        }
        "xai.view_x_video" => {
            prepared.insert(
                "type".to_string(),
                JsonValue::String("view_x_video".to_string()),
            );
        }
        "xai.file_search" | "openai.file_search" => {
            prepared.insert(
                "type".to_string(),
                JsonValue::String("file_search".to_string()),
            );
            xai_insert_mapped(
                &mut prepared,
                &tool.args,
                "vector_store_ids",
                "vectorStoreIds",
            );
            xai_insert_mapped(
                &mut prepared,
                &tool.args,
                "max_num_results",
                "maxNumResults",
            );
        }
        "xai.mcp" | "openai.mcp" => {
            prepared.insert("type".to_string(), JsonValue::String("mcp".to_string()));
            xai_insert_mapped(&mut prepared, &tool.args, "server_url", "serverUrl");
            xai_insert_mapped(&mut prepared, &tool.args, "server_label", "serverLabel");
            xai_insert_mapped(
                &mut prepared,
                &tool.args,
                "server_description",
                "serverDescription",
            );
            xai_insert_mapped(&mut prepared, &tool.args, "allowed_tools", "allowedTools");
            xai_insert_mapped(&mut prepared, &tool.args, "headers", "headers");
            xai_insert_mapped(&mut prepared, &tool.args, "authorization", "authorization");
        }
        "openai.web_search" => {
            prepared.insert(
                "type".to_string(),
                JsonValue::String("web_search".to_string()),
            );
        }
        "openai.custom" => {
            prepared.insert("type".to_string(), JsonValue::String("custom".to_string()));
            prepared.insert("name".to_string(), JsonValue::String(tool.name.clone()));
            xai_insert_mapped(&mut prepared, &tool.args, "description", "description");
            xai_insert_mapped(&mut prepared, &tool.args, "format", "format");
        }
        _ => return None,
    }

    Some(JsonValue::Object(prepared))
}

fn xai_responses_tool_choice(context: &XaiResponsesRequestContext) -> Option<JsonValue> {
    let choice = context.tool_choice.as_ref()?;
    match choice {
        LanguageModelToolChoice::Auto => Some(JsonValue::String("auto".to_string())),
        LanguageModelToolChoice::None => Some(JsonValue::String("none".to_string())),
        LanguageModelToolChoice::Required => Some(JsonValue::String("required".to_string())),
        LanguageModelToolChoice::Tool { tool_name } => {
            let selected_tool = context
                .tools
                .as_ref()
                .and_then(|tools| tools.iter().find(|tool| xai_tool_name(tool) == tool_name));
            match selected_tool {
                Some(LanguageModelTool::Function(_)) => Some(json!({
                    "type": "function",
                    "name": tool_name
                })),
                Some(LanguageModelTool::Provider(tool)) if tool.id == "openai.custom" => {
                    Some(json!({
                        "type": "custom",
                        "name": tool_name
                    }))
                }
                Some(LanguageModelTool::Provider(tool)) if tool.id.starts_with("openai.") => {
                    xai_responses_provider_tool_choice_type(tool).map(|tool_type| {
                        json!({
                            "type": tool_type
                        })
                    })
                }
                Some(LanguageModelTool::Provider(_)) | None => None,
            }
        }
    }
}

fn xai_responses_provider_tool_choice_type(
    tool: &LanguageModelProviderTool,
) -> Option<&'static str> {
    match tool.id.as_str() {
        "openai.web_search" => Some("web_search"),
        "openai.file_search" => Some("file_search"),
        "openai.mcp" => Some("mcp"),
        _ => None,
    }
}

fn xai_tool_name(tool: &LanguageModelTool) -> &str {
    match tool {
        LanguageModelTool::Function(tool) => &tool.name,
        LanguageModelTool::Provider(tool) => &tool.name,
    }
}

fn xai_remove_additional_properties_false(value: JsonValue) -> JsonValue {
    match value {
        JsonValue::Object(mut object) => {
            if object.get("additionalProperties") == Some(&JsonValue::Bool(false)) {
                object.remove("additionalProperties");
            }
            for value in object.values_mut() {
                *value = xai_remove_additional_properties_false(value.clone());
            }
            JsonValue::Object(object)
        }
        JsonValue::Array(values) => JsonValue::Array(
            values
                .into_iter()
                .map(xai_remove_additional_properties_false)
                .collect(),
        ),
        other => other,
    }
}

fn xai_responses_add_provider_metadata(result: &mut LanguageModelGenerateResult) {
    let cost = result
        .usage
        .raw
        .as_ref()
        .and_then(|usage| usage.get("cost_in_usd_ticks"))
        .and_then(JsonValue::as_u64);

    let Some(cost) = cost else {
        return;
    };

    let mut metadata = result.provider_metadata.take().unwrap_or_default();
    metadata
        .entry("xai".to_string())
        .or_default()
        .insert("costInUsdTicks".to_string(), json!(cost));
    result.provider_metadata = Some(metadata);
}

fn xai_responses_replace_content(result: &mut LanguageModelGenerateResult) {
    let Some(body) = result
        .response
        .as_ref()
        .and_then(|response| response.body.as_ref())
    else {
        return;
    };
    let Some(output) = body.get("output").and_then(JsonValue::as_array) else {
        return;
    };

    let mut xai_content = Vec::new();
    for part in output {
        let Some(part_type) = part.get("type").and_then(JsonValue::as_str) else {
            continue;
        };
        let tool_name = match part_type {
            "x_search_call" => Some("x_search"),
            "code_execution_call" => Some("code_execution"),
            "view_image_call" => Some("view_image"),
            "view_x_video_call" => Some("view_x_video"),
            _ => None,
        };
        let Some(default_tool_name) = tool_name else {
            continue;
        };
        let tool_call_id = part
            .get("id")
            .and_then(JsonValue::as_str)
            .unwrap_or_default();
        let tool_name = part
            .get("name")
            .and_then(JsonValue::as_str)
            .filter(|name| !name.is_empty())
            .unwrap_or(default_tool_name);
        let input = part
            .get("arguments")
            .and_then(JsonValue::as_str)
            .unwrap_or("{}");
        xai_content.push(LanguageModelContent::ToolCall(
            LanguageModelToolCall::new(tool_call_id, tool_name, input).with_provider_executed(true),
        ));
    }

    if !xai_content.is_empty() {
        result.content = xai_content;
        result.finish_reason = LanguageModelFinishReason {
            unified: FinishReason::ToolCalls,
            raw: result.finish_reason.raw.clone(),
        };
    }
}

impl XaiImageModel {
    async fn do_generate_result(&self, options: ImageModelCallOptions) -> ImageModelResult {
        let warnings = xai_image_warnings(&options);
        let body = xai_image_request_body(&self.model_id, &options);
        let endpoint = if options
            .files
            .as_ref()
            .is_some_and(|files| !files.is_empty())
        {
            "images/edits"
        } else {
            "images/generations"
        };
        let post_options = PostJsonToApiOptions::new(format!("{}/{endpoint}", self.base_url), body)
            .with_headers(xai_call_headers(&self.headers, options.headers.as_ref()))
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| transport(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    |value| serde_json::from_value::<JsonValue>(value.clone()),
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| Ok(xai_json_error_response(request, response)),
        )
        .await
        {
            Ok(response) => {
                self.image_result_from_response(response.value, response.response_headers, warnings)
                    .await
            }
            Err(error) => xai_image_result_from_error(&self.model_id, error, warnings),
        }
    }

    async fn image_result_from_response(
        &self,
        response: JsonValue,
        response_headers: Option<Headers>,
        warnings: Vec<Warning>,
    ) -> ImageModelResult {
        let mut images = Vec::new();
        for item in response
            .get("data")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(base64) = item.get("b64_json").and_then(JsonValue::as_str) {
                images.push(FileDataContent::Base64(base64.to_string()));
            } else if let Some(url) = item.get("url").and_then(JsonValue::as_str)
                && let Some(bytes) = self.download_image_url(url).await
            {
                images.push(FileDataContent::Bytes(bytes));
            }
        }

        let mut result = ImageModelResult::new(
            images,
            xai_image_response_metadata(&self.model_id, response_headers),
        )
        .with_provider_metadata(xai_image_provider_metadata(&response));

        for warning in warnings {
            result = result.with_warning(warning);
        }

        result
    }

    async fn download_image_url(&self, url: &str) -> Option<Vec<u8>> {
        let request = ProviderApiRequest::get(url, Headers::new());
        let response = (self.transport)(request).await.ok()?;
        if let Some(bytes) = response.bytes_body() {
            return Some(bytes.to_vec());
        }
        response.text_body().map(|body| body.as_bytes().to_vec())
    }
}

fn xai_image_warnings(options: &ImageModelCallOptions) -> Vec<Warning> {
    let mut warnings = Vec::new();
    if options.size.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "size".to_string(),
            details: Some("xAI image models use aspectRatio instead of size.".to_string()),
        });
    }
    if options.seed.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "seed".to_string(),
            details: None,
        });
    }
    if options.mask.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "mask".to_string(),
            details: None,
        });
    }
    warnings
}

fn xai_image_request_body(model_id: &str, options: &ImageModelCallOptions) -> JsonValue {
    let mut body = JsonObject::new();
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    body.insert(
        "prompt".to_string(),
        JsonValue::String(options.prompt.clone().unwrap_or_default()),
    );
    body.insert("n".to_string(), json!(options.n));
    body.insert(
        "response_format".to_string(),
        JsonValue::String("b64_json".to_string()),
    );

    if let Some(aspect_ratio) = &options.aspect_ratio {
        body.insert(
            "aspect_ratio".to_string(),
            JsonValue::String(aspect_ratio.clone()),
        );
    }

    if let Some(provider_options) = options.provider_options.get("xai") {
        xai_merge_camel_provider_options(&mut body, provider_options, &["aspectRatio"]);
        if !body.contains_key("aspect_ratio")
            && let Some(aspect_ratio) = provider_options.get("aspectRatio")
        {
            body.insert("aspect_ratio".to_string(), aspect_ratio.clone());
        }
    }

    if let Some(files) = options.files.as_ref().filter(|files| !files.is_empty()) {
        let images = files.iter().map(xai_image_file_value).collect::<Vec<_>>();
        if images.len() == 1 {
            body.insert("image".to_string(), images[0].clone());
        } else {
            body.insert("images".to_string(), JsonValue::Array(images));
        }
    }

    JsonValue::Object(body)
}

fn xai_image_file_value(file: &ImageModelFile) -> JsonValue {
    let url = match file {
        ImageModelFile::File {
            media_type, data, ..
        } => {
            format!("data:{media_type};base64,{}", convert_to_base64(data))
        }
        ImageModelFile::Url { url, .. } => url.to_string(),
    };
    json!({
        "url": url,
        "type": "image_url"
    })
}

fn xai_image_provider_metadata(response: &JsonValue) -> ImageModelProviderMetadata {
    let images = response
        .get("data")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .map(|item| {
            let mut metadata = JsonObject::new();
            if let Some(revised_prompt) = item.get("revised_prompt").and_then(JsonValue::as_str) {
                metadata.insert(
                    "revisedPrompt".to_string(),
                    JsonValue::String(revised_prompt.to_string()),
                );
            }
            JsonValue::Object(metadata)
        })
        .collect::<Vec<_>>();
    let mut entry = ImageModelProviderMetadataEntry::new(images);
    if let Some(cost) = response
        .get("usage")
        .and_then(|usage| usage.get("cost_in_usd_ticks"))
        .and_then(JsonValue::as_u64)
    {
        entry = entry.with_extra("costInUsdTicks", json!(cost));
    }
    ImageModelProviderMetadata::from([("xai".to_string(), entry)])
}

fn xai_image_response_metadata(model_id: &str, headers: Option<Headers>) -> ImageModelResponse {
    let mut response = ImageModelResponse::new(OffsetDateTime::now_utc(), model_id);
    if let Some(headers) = headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }
    response
}

fn xai_image_result_from_error(
    model_id: &str,
    error: HandledFetchError,
    warnings: Vec<Warning>,
) -> ImageModelResult {
    let (message, headers) = xai_fetch_error_message_headers(error);
    let mut result =
        ImageModelResult::new(Vec::new(), xai_image_response_metadata(model_id, headers))
            .with_provider_metadata(ImageModelProviderMetadata::from([(
                "xai".to_string(),
                ImageModelProviderMetadataEntry::new(Vec::new())
                    .with_extra("errorMessage", JsonValue::String(message)),
            )]));

    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

impl XaiFiles {
    async fn upload_file_result(
        &self,
        options: FilesUploadFileCallOptions,
    ) -> FilesUploadFileResult {
        let form_data = xai_file_upload_form_data(&options);
        let post_options =
            PostFormDataToApiOptions::new(format!("{}/files", self.base_url), form_data)
                .with_headers(self.headers.clone())
                .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        match post_form_data_to_api(
            post_options,
            move |request| transport(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    |value| serde_json::from_value::<JsonValue>(value.clone()),
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| Ok(xai_json_error_response(request, response)),
        )
        .await
        {
            Ok(response) => xai_file_upload_result_from_response(response.value, &options),
            Err(error) => xai_file_upload_result_from_error(error, &options),
        }
    }
}

fn xai_file_upload_form_data(options: &FilesUploadFileCallOptions) -> FormData {
    let mut form_data = FormData::new();
    let file_bytes = match &options.data {
        FilesUploadFileData::Data { data } => xai_file_content_bytes(data),
        FilesUploadFileData::Text { text } => text.as_bytes().to_vec(),
    };
    form_data.append("file", FormDataValue::bytes(file_bytes));
    if let Some(team_id) = options
        .provider_options
        .as_ref()
        .and_then(|options| options.get("xai"))
        .and_then(|xai| xai.get("teamId"))
        .and_then(JsonValue::as_str)
    {
        form_data.append("team_id", FormDataValue::text(team_id));
    }
    form_data
}

fn xai_file_content_bytes(data: &FileDataContent) -> Vec<u8> {
    match data {
        FileDataContent::Bytes(bytes) => bytes.clone(),
        FileDataContent::Base64(base64) => {
            convert_base64_to_bytes(base64).unwrap_or_else(|_| base64.as_bytes().to_vec())
        }
    }
}

fn xai_file_upload_result_from_response(
    response: JsonValue,
    options: &FilesUploadFileCallOptions,
) -> FilesUploadFileResult {
    let file_id = response
        .get("id")
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_string();
    let provider_reference =
        ProviderReference::try_from(BTreeMap::from([("xai".to_string(), file_id)]))
            .expect("xAI provider reference is valid");
    let mut result =
        FilesUploadFileResult::new(provider_reference).with_media_type(options.media_type.clone());
    if let Some(filename) = response
        .get("filename")
        .and_then(JsonValue::as_str)
        .or(options.filename.as_deref())
    {
        result = result.with_filename(filename);
    }

    let mut metadata = JsonObject::new();
    for (target, source) in [
        ("filename", "filename"),
        ("bytes", "bytes"),
        ("createdAt", "created_at"),
    ] {
        if let Some(value) = response.get(source).filter(|value| !value.is_null()) {
            metadata.insert(target.to_string(), value.clone());
        }
    }
    if !metadata.is_empty() {
        result =
            result.with_provider_metadata(ProviderMetadata::from([("xai".to_string(), metadata)]));
    }
    result
}

fn xai_file_upload_result_from_error(
    error: HandledFetchError,
    options: &FilesUploadFileCallOptions,
) -> FilesUploadFileResult {
    let (message, _) = xai_fetch_error_message_headers(error);
    let provider_reference =
        ProviderReference::try_from(BTreeMap::from([("xai".to_string(), String::new())]))
            .expect("xAI provider reference is valid");
    FilesUploadFileResult::new(provider_reference)
        .with_media_type(options.media_type.clone())
        .with_provider_metadata(ProviderMetadata::from([(
            "xai".to_string(),
            JsonObject::from_iter([("errorMessage".to_string(), JsonValue::String(message))]),
        )]))
}

impl XaiVideoModel {
    async fn do_generate_result(&self, options: VideoModelCallOptions) -> VideoModelResult {
        let warnings = xai_video_warnings(&options);
        if let Some(message) = xai_video_options_error(&options) {
            return xai_video_result_from_message(&self.model_id, message, None, warnings);
        }
        let (endpoint, body) = xai_video_request(&self.model_id, &options);
        let provider_options = options.provider_options.get("xai");
        let poll_interval_ms = xai_video_poll_millis(provider_options, "pollIntervalMs", 5000);
        let poll_timeout_ms = xai_video_poll_millis(provider_options, "pollTimeoutMs", 600000);
        let post_options =
            PostJsonToApiOptions::new(format!("{}/{}", self.base_url, endpoint), body)
                .with_headers(xai_call_headers(&self.headers, options.headers.as_ref()))
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.transport);

        let request_response = match post_json_to_api(
            post_options,
            move |request| transport(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    |value| serde_json::from_value::<JsonValue>(value.clone()),
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| Ok(xai_json_error_response(request, response)),
        )
        .await
        {
            Ok(response) => response,
            Err(error) => return xai_video_result_from_error(&self.model_id, error, warnings),
        };

        let Some(request_id) = request_response
            .value
            .get("request_id")
            .or_else(|| request_response.value.get("requestId"))
            .and_then(JsonValue::as_str)
            .map(ToString::to_string)
        else {
            return xai_video_result_from_message(
                &self.model_id,
                "xAI video response did not include request_id".to_string(),
                request_response.response_headers,
                warnings,
            );
        };

        let started_at = Instant::now();
        let create_headers = request_response.response_headers.clone();

        loop {
            let mut delay_options = DelayOptions::new();
            if let Some(abort_signal) = options.abort_signal.clone() {
                delay_options = delay_options.with_abort_signal(abort_signal);
            }
            if delay_with_options(Some(poll_interval_ms), delay_options)
                .await
                .is_err()
            {
                return xai_video_result_from_message(
                    &self.model_id,
                    "xAI video polling was aborted".to_string(),
                    create_headers.clone(),
                    warnings,
                );
            }
            if started_at.elapsed() > Duration::from_millis(poll_timeout_ms.max(0) as u64) {
                return xai_video_result_from_message(
                    &self.model_id,
                    format!("Video generation timed out after {poll_timeout_ms}ms"),
                    create_headers.clone(),
                    warnings,
                );
            }

            let get_options =
                GetFromApiOptions::new(format!("{}/videos/{}", self.base_url, request_id))
                    .with_headers(xai_call_headers(&self.headers, options.headers.as_ref()))
                    .with_environment(RuntimeEnvironment::unknown())
                    .with_optional_abort_signal(options.abort_signal.clone());
            let transport = Arc::clone(&self.transport);

            match crate::provider_utils::get_from_api(
                get_options,
                move |request| transport(request),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        |value| serde_json::from_value::<JsonValue>(value.clone()),
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| Ok(xai_json_error_response(request, response)),
            )
            .await
            {
                Ok(response) => match xai_video_poll_result(
                    &self.model_id,
                    &request_id,
                    response.value,
                    response.response_headers.or_else(|| create_headers.clone()),
                    warnings.clone(),
                ) {
                    XaiVideoPollResult::Done(result) => return result,
                    XaiVideoPollResult::Pending => continue,
                },
                Err(error) => return xai_video_result_from_error(&self.model_id, error, warnings),
            }
        }
    }
}

fn xai_video_options_error(options: &VideoModelCallOptions) -> Option<String> {
    let reference_urls = options
        .provider_options
        .get("xai")
        .and_then(|xai| xai.get("referenceImageUrls"))
        .and_then(JsonValue::as_array)?;
    if reference_urls.is_empty() {
        return Some("xAI referenceImageUrls must include at least one URL".to_string());
    }
    if reference_urls.len() > 7 {
        return Some("xAI referenceImageUrls supports at most 7 images".to_string());
    }
    if reference_urls.iter().any(|url| url.as_str() == Some("")) {
        return Some("xAI referenceImageUrls cannot include empty URLs".to_string());
    }
    None
}

fn xai_video_mode(provider_options: Option<&JsonObject>) -> String {
    provider_options
        .and_then(|xai| xai.get("mode"))
        .and_then(JsonValue::as_str)
        .map(ToString::to_string)
        .or_else(|| {
            provider_options
                .and_then(|xai| xai.get("videoUrl"))
                .and_then(JsonValue::as_str)
                .map(|_| "edit-video".to_string())
        })
        .or_else(|| {
            provider_options
                .and_then(|xai| xai.get("referenceImageUrls"))
                .and_then(JsonValue::as_array)
                .filter(|urls| !urls.is_empty())
                .map(|_| "reference-to-video".to_string())
        })
        .unwrap_or_else(|| "text-to-video".to_string())
}

fn xai_video_poll_millis(provider_options: Option<&JsonObject>, key: &str, default_ms: i64) -> i64 {
    provider_options
        .and_then(|xai| xai.get(key))
        .and_then(JsonValue::as_i64)
        .unwrap_or(default_ms)
}

fn xai_video_request(model_id: &str, options: &VideoModelCallOptions) -> (&'static str, JsonValue) {
    let provider_options = options.provider_options.get("xai");
    let mode = xai_video_mode(provider_options);

    let endpoint = match mode.as_str() {
        "edit-video" => "videos/edits",
        "extend-video" => "videos/extensions",
        _ => "videos/generations",
    };

    let mut body = JsonObject::new();
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    body.insert(
        "prompt".to_string(),
        JsonValue::String(options.prompt.clone().unwrap_or_default()),
    );

    if mode != "edit-video" {
        if let Some(duration) = options.duration {
            body.insert("duration".to_string(), json!(duration));
        }
    }
    if !matches!(mode.as_str(), "edit-video" | "extend-video") {
        if let Some(aspect_ratio) = &options.aspect_ratio {
            body.insert(
                "aspect_ratio".to_string(),
                JsonValue::String(aspect_ratio.clone()),
            );
        }
        if let Some(resolution) = xai_video_resolution(options, provider_options) {
            body.insert("resolution".to_string(), JsonValue::String(resolution));
        }
    }

    if let Some(provider_options) = provider_options {
        xai_merge_camel_provider_options(
            &mut body,
            provider_options,
            &[
                "mode",
                "pollIntervalMs",
                "pollTimeoutMs",
                "videoUrl",
                "referenceImageUrls",
                "resolution",
            ],
        );
        if let Some(video_url) = provider_options.get("videoUrl").and_then(JsonValue::as_str) {
            body.insert("video".to_string(), json!({ "url": video_url }));
        }
        if let Some(reference_urls) = provider_options
            .get("referenceImageUrls")
            .and_then(JsonValue::as_array)
        {
            body.insert(
                "reference_images".to_string(),
                JsonValue::Array(
                    reference_urls
                        .iter()
                        .filter_map(JsonValue::as_str)
                        .map(|url| json!({ "url": url }))
                        .collect(),
                ),
            );
        }
    }

    if let Some(image) = options.image.as_ref() {
        body.insert(
            "image".to_string(),
            json!({ "url": xai_video_file_url(image) }),
        );
    }

    (endpoint, JsonValue::Object(body))
}

fn xai_video_warnings(options: &VideoModelCallOptions) -> Vec<Warning> {
    let mut warnings = Vec::new();
    let provider_options = options.provider_options.get("xai");
    let mode = xai_video_mode(provider_options);
    let is_edit = mode == "edit-video";
    let is_extension = mode == "extend-video";
    if options.fps.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "fps".to_string(),
            details: None,
        });
    }
    if options.seed.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "seed".to_string(),
            details: None,
        });
    }
    if options.n > 1 {
        warnings.push(Warning::Unsupported {
            feature: "n".to_string(),
            details: Some("xAI supports one video per call.".to_string()),
        });
    }
    if is_edit && options.duration.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "duration".to_string(),
            details: Some("xAI video editing does not support custom duration.".to_string()),
        });
    }
    if is_edit && options.aspect_ratio.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "aspectRatio".to_string(),
            details: Some("xAI video editing does not support custom aspect ratio.".to_string()),
        });
    }
    if is_edit
        && (options.resolution.is_some()
            || provider_options
                .and_then(|xai| xai.get("resolution"))
                .is_some())
    {
        warnings.push(Warning::Unsupported {
            feature: "resolution".to_string(),
            details: Some("xAI video editing does not support custom resolution.".to_string()),
        });
    }
    if is_extension && options.aspect_ratio.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "aspectRatio".to_string(),
            details: Some("xAI video extension does not support custom aspect ratio.".to_string()),
        });
    }
    if is_extension
        && (options.resolution.is_some()
            || provider_options
                .and_then(|xai| xai.get("resolution"))
                .is_some())
    {
        warnings.push(Warning::Unsupported {
            feature: "resolution".to_string(),
            details: Some("xAI video extension does not support custom resolution.".to_string()),
        });
    }
    if !matches!(mode.as_str(), "edit-video" | "extend-video")
        && options.resolution.is_some()
        && xai_video_resolution(options, provider_options).is_none()
    {
        warnings.push(Warning::Unsupported {
            feature: "resolution".to_string(),
            details: Some(
                "Unrecognized resolution; use providerOptions.xai.resolution with 480p or 720p."
                    .to_string(),
            ),
        });
    }
    warnings
}

fn xai_video_resolution(
    options: &VideoModelCallOptions,
    provider_options: Option<&JsonObject>,
) -> Option<String> {
    provider_options
        .and_then(|xai| xai.get("resolution"))
        .and_then(JsonValue::as_str)
        .map(ToString::to_string)
        .or_else(|| match options.resolution.as_deref() {
            Some("1280x720") => Some("720p".to_string()),
            Some("854x480") | Some("640x480") => Some("480p".to_string()),
            _ => None,
        })
}

fn xai_video_file_url(file: &VideoModelFile) -> String {
    match file {
        VideoModelFile::File {
            media_type, data, ..
        } => {
            format!("data:{media_type};base64,{}", convert_to_base64(data))
        }
        VideoModelFile::Url { url, .. } => url.to_string(),
    }
}

fn xai_video_result_from_response(
    model_id: &str,
    request_id: &str,
    response: JsonValue,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
) -> VideoModelResult {
    let status = response.get("status").and_then(JsonValue::as_str);
    if matches!(status, Some("failed" | "expired" | "canceled")) {
        return xai_video_result_from_message(
            model_id,
            format!("xAI video generation {status:?}"),
            headers,
            warnings,
        );
    }
    if response
        .get("video")
        .and_then(|video| video.get("respect_moderation"))
        .and_then(JsonValue::as_bool)
        == Some(false)
    {
        return xai_video_result_from_message(
            model_id,
            "Video generation was blocked due to a content policy violation.".to_string(),
            headers,
            warnings,
        );
    }

    let Some(video_url) = response
        .get("video")
        .and_then(|video| video.get("url"))
        .and_then(JsonValue::as_str)
        .or_else(|| response.get("url").and_then(JsonValue::as_str))
    else {
        return xai_video_result_from_message(
            model_id,
            "xAI video response did not include a video URL".to_string(),
            headers,
            warnings,
        );
    };
    let Ok(url) = Url::parse(video_url) else {
        return xai_video_result_from_message(
            model_id,
            "xAI video response included an invalid video URL".to_string(),
            headers,
            warnings,
        );
    };

    let mut metadata = JsonObject::new();
    metadata.insert(
        "requestId".to_string(),
        JsonValue::String(request_id.to_string()),
    );
    metadata.insert(
        "videoUrl".to_string(),
        JsonValue::String(video_url.to_string()),
    );
    if let Some(duration) = response
        .get("video")
        .and_then(|video| video.get("duration"))
        .or_else(|| response.get("duration"))
        .filter(|value| !value.is_null())
    {
        metadata.insert("duration".to_string(), duration.clone());
    }
    if let Some(progress) = response.get("progress").filter(|value| !value.is_null()) {
        metadata.insert("progress".to_string(), progress.clone());
    }
    if let Some(cost) = response
        .get("usage")
        .and_then(|usage| usage.get("cost_in_usd_ticks"))
        .or_else(|| response.get("cost_in_usd_ticks"))
        .filter(|value| !value.is_null())
    {
        metadata.insert("costInUsdTicks".to_string(), cost.clone());
    }

    let mut result = VideoModelResult::new(
        vec![VideoModelVideoData::url(url, "video/mp4")],
        xai_video_response_metadata(model_id, headers),
    )
    .with_provider_metadata(ProviderMetadata::from([("xai".to_string(), metadata)]));

    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

enum XaiVideoPollResult {
    Done(VideoModelResult),
    Pending,
}

fn xai_video_poll_result(
    model_id: &str,
    request_id: &str,
    response: JsonValue,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
) -> XaiVideoPollResult {
    let status = response.get("status").and_then(JsonValue::as_str);
    if matches!(status, Some("pending" | "in_progress" | "processing")) {
        return XaiVideoPollResult::Pending;
    }
    XaiVideoPollResult::Done(xai_video_result_from_response(
        model_id, request_id, response, headers, warnings,
    ))
}

fn xai_video_result_from_error(
    model_id: &str,
    error: HandledFetchError,
    warnings: Vec<Warning>,
) -> VideoModelResult {
    let (message, headers) = xai_fetch_error_message_headers(error);
    xai_video_result_from_message(model_id, message, headers, warnings)
}

fn xai_video_result_from_message(
    model_id: &str,
    message: String,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
) -> VideoModelResult {
    let mut result =
        VideoModelResult::new(Vec::new(), xai_video_response_metadata(model_id, headers))
            .with_provider_metadata(ProviderMetadata::from([(
                "xai".to_string(),
                JsonObject::from_iter([("errorMessage".to_string(), JsonValue::String(message))]),
            )]));

    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

fn xai_video_response_metadata(model_id: &str, headers: Option<Headers>) -> VideoModelResponse {
    let mut response = VideoModelResponse::new(OffsetDateTime::now_utc(), model_id);
    if let Some(headers) = headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }
    response
}

fn xai_merge_camel_provider_options(
    body: &mut JsonObject,
    provider_options: &JsonObject,
    exclude: &[&str],
) {
    for (key, value) in provider_options {
        if exclude.contains(&key.as_str()) || value.is_null() {
            continue;
        }
        body.insert(xai_camel_to_snake(key), value.clone());
    }
}

fn xai_camel_to_snake(key: &str) -> String {
    let mut output = String::new();
    for character in key.chars() {
        if character.is_ascii_uppercase() {
            output.push('_');
            output.push(character.to_ascii_lowercase());
        } else {
            output.push(character);
        }
    }
    output
}

fn xai_json_error_response(
    request: &ProviderApiRequest,
    response: &ProviderApiResponse,
) -> crate::provider_utils::ResponseHandlerResult<ApiCallError> {
    create_json_error_response_handler(
        response.json_error_response_handler_options(request),
        |value| serde_json::from_value::<JsonValue>(value.clone()),
        |error| xai_error_to_message(error).unwrap_or_else(|| "Unknown error".to_string()),
        |_, _| None,
    )
}

fn xai_fetch_error_message_headers(error: HandledFetchError) -> (String, Option<Headers>) {
    match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
        ),
    }
}

fn default_xai_transport() -> OpenAICompatibleTransport {
    Arc::new(|request| Box::pin(ready(execute_xai_request(request))))
}

fn execute_xai_request(request: ProviderApiRequest) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_xai_get_request(request),
        ProviderApiRequestMethod::Post => execute_xai_post_request(request),
    }
}

fn execute_xai_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let response = builder.config().http_status_as_error(false).build().call();
    xai_provider_api_response(response)
}

fn execute_xai_post_request(
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
        Some(ProviderApiRequestBody::FormData { content }) => {
            let (content_type, body) = xai_multipart_body(&content);
            builder.header("content-type", content_type).send(body)
        }
        None => builder.send_empty(),
    };
    xai_provider_api_response(response)
}

fn xai_multipart_body(form_data: &FormData) -> (String, Vec<u8>) {
    let boundary = "----ai-sdk-rust-xai-boundary";
    let mut body = Vec::new();

    for entry in &form_data.entries {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match &entry.value {
            FormDataValue::Text { value } => {
                body.extend_from_slice(
                    format!(
                        "content-disposition: form-data; name=\"{}\"\r\n\r\n",
                        entry.name
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(value.as_bytes());
            }
            FormDataValue::Bytes { value } => {
                body.extend_from_slice(
                    format!(
                        "content-disposition: form-data; name=\"{}\"; filename=\"blob\"\r\ncontent-type: application/octet-stream\r\n\r\n",
                        entry.name
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(value);
            }
        }
        body.extend_from_slice(b"\r\n");
    }

    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn xai_provider_api_response(
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
fn xai_convert_chat_messages(
    prompt: &[crate::language_model::LanguageModelMessage],
) -> Vec<JsonValue> {
    prompt.iter().filter_map(xai_convert_chat_message).collect()
}

#[cfg(test)]
fn xai_convert_chat_message(
    message: &crate::language_model::LanguageModelMessage,
) -> Option<JsonValue> {
    use crate::file_data::FileData;
    use crate::language_model::{
        LanguageModelAssistantContentPart, LanguageModelMessage, LanguageModelToolContentPart,
        LanguageModelUserContentPart,
    };

    match message {
        LanguageModelMessage::System(message) => Some(json!({
            "role": "system",
            "content": message.content
        })),
        LanguageModelMessage::User(message) => {
            let content = message
                .content
                .iter()
                .filter_map(|part| match part {
                    LanguageModelUserContentPart::Text(text) => Some(json!({
                        "type": "text",
                        "text": text.text
                    })),
                    LanguageModelUserContentPart::File(file) => match &file.data {
                        FileData::Data { data } if file.media_type.starts_with("image/") => {
                            Some(json!({
                                "type": "image_url",
                                "image_url": {
                                    "url": format!("data:{};base64,{}", file.media_type, convert_to_base64(data))
                                }
                            }))
                        }
                        FileData::Url { url } if file.media_type.starts_with("image/") => {
                            Some(json!({
                                "type": "image_url",
                                "image_url": {
                                    "url": url.to_string()
                                }
                            }))
                        }
                        _ => None,
                    },
                })
                .collect::<Vec<_>>();
            Some(json!({
                "role": "user",
                "content": content
            }))
        }
        LanguageModelMessage::Assistant(message) => {
            let mut text = String::new();
            let mut tool_calls = Vec::new();
            for part in &message.content {
                match part {
                    LanguageModelAssistantContentPart::Text(part) => text.push_str(&part.text),
                    LanguageModelAssistantContentPart::ToolCall(part) => tool_calls.push(json!({
                        "id": part.tool_call_id,
                        "type": "function",
                        "function": {
                            "name": part.tool_name,
                            "arguments": part.input.to_string()
                        }
                    })),
                    _ => {}
                }
            }
            let mut object = JsonObject::new();
            object.insert(
                "role".to_string(),
                JsonValue::String("assistant".to_string()),
            );
            object.insert("content".to_string(), JsonValue::String(text));
            if !tool_calls.is_empty() {
                object.insert("tool_calls".to_string(), JsonValue::Array(tool_calls));
            }
            Some(JsonValue::Object(object))
        }
        LanguageModelMessage::Tool(message) => message.content.iter().find_map(|part| {
            let LanguageModelToolContentPart::ToolResult(part) = part else {
                return None;
            };
            Some(json!({
                "role": "tool",
                "tool_call_id": part.tool_call_id,
                "content": xai_tool_result_output_string(&part.output)
            }))
        }),
    }
}

#[cfg(test)]
fn xai_convert_responses_input(
    prompt: &[crate::language_model::LanguageModelMessage],
) -> Vec<JsonValue> {
    use crate::file_data::FileData;
    use crate::language_model::{
        LanguageModelAssistantContentPart, LanguageModelMessage, LanguageModelToolContentPart,
        LanguageModelUserContentPart,
    };

    prompt
        .iter()
        .flat_map(|message| match message {
            LanguageModelMessage::System(message) => vec![json!({
                "role": "system",
                "content": [
                    {
                        "type": "input_text",
                        "text": message.content
                    }
                ]
            })],
            LanguageModelMessage::User(message) => vec![json!({
                "role": "user",
                "content": message.content.iter().filter_map(|part| match part {
                    LanguageModelUserContentPart::Text(text) => Some(json!({
                        "type": "input_text",
                        "text": text.text
                    })),
                    LanguageModelUserContentPart::File(file) => match &file.data {
                        FileData::Reference { reference } => reference.provider_id("xai").ok().map(|file_id| json!({
                            "type": "input_file",
                            "file_id": file_id
                        })),
                        FileData::Url { url } if file.media_type.starts_with("image/") => Some(json!({
                            "type": "input_image",
                            "image_url": url.to_string()
                        })),
                        FileData::Data { data } if file.media_type.starts_with("image/") => Some(json!({
                            "type": "input_image",
                            "image_url": format!("data:{};base64,{}", file.media_type, convert_to_base64(data))
                        })),
                        FileData::Url { url } => Some(json!({
                            "type": "input_file",
                            "file_url": url.to_string()
                        })),
                        _ => None,
                    },
                }).collect::<Vec<_>>()
            })],
            LanguageModelMessage::Assistant(message) => message
                .content
                .iter()
                .filter_map(|part| match part {
                    LanguageModelAssistantContentPart::Text(part) => Some(json!({
                        "role": "assistant",
                        "content": [
                            {
                                "type": "output_text",
                                "text": part.text
                            }
                        ]
                    })),
                    LanguageModelAssistantContentPart::ToolCall(part)
                        if part.provider_executed != Some(true) =>
                    {
                        Some(json!({
                            "type": "function_call",
                            "call_id": part.tool_call_id,
                            "name": part.tool_name,
                            "arguments": part.input.to_string()
                        }))
                    }
                    LanguageModelAssistantContentPart::Reasoning(part) => Some(json!({
                        "type": "reasoning",
                        "summary": [
                            {
                                "type": "summary_text",
                                "text": part.text
                            }
                        ]
                    })),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            LanguageModelMessage::Tool(message) => message
                .content
                .iter()
                .filter_map(|part| {
                    let LanguageModelToolContentPart::ToolResult(part) = part else {
                        return None;
                    };
                    Some(json!({
                        "type": "function_call_output",
                        "call_id": part.tool_call_id,
                        "output": xai_tool_result_output_string(&part.output)
                    }))
                })
                .collect::<Vec<_>>(),
        })
        .collect()
}

#[cfg(test)]
fn xai_tool_result_output_string(
    output: &crate::language_model::LanguageModelToolResultOutput,
) -> String {
    use crate::language_model::LanguageModelToolResultOutput;

    match output {
        LanguageModelToolResultOutput::Text { value, .. }
        | LanguageModelToolResultOutput::ErrorText { value, .. } => value.clone(),
        LanguageModelToolResultOutput::Json { value, .. }
        | LanguageModelToolResultOutput::ErrorJson { value, .. } => value.to_string(),
        LanguageModelToolResultOutput::ExecutionDenied { reason, .. } => reason
            .clone()
            .unwrap_or_else(|| "Tool call execution denied.".to_string()),
        LanguageModelToolResultOutput::Content { value } => {
            serde_json::to_string(value).unwrap_or_else(|_| "[]".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_XAI_BASE_URL, XaiProvider, XaiProviderSettings, XaiVideoPollResult, create_xai,
        transform_xai_chat_request_body, xai, xai_chat_usage_from_raw, xai_convert_chat_messages,
        xai_convert_responses_input, xai_image_request_body, xai_image_warnings,
        xai_responses_request_with_tools, xai_responses_usage_from_raw, xai_video_options_error,
        xai_video_poll_result, xai_video_request, xai_video_result_from_response,
        xai_video_warnings,
    };
    use crate::file_data::{FileData, FileDataContent, ProviderReference};
    use crate::files::{Files, FilesUploadFileCallOptions, FilesUploadFileData};
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::image_model::{ImageModel, ImageModelCallOptions, ImageModelFile};
    use crate::json::{JsonObject, JsonValue};
    use crate::language_model::{
        FinishReason, InputTokenUsage, LanguageModel, LanguageModelAssistantContentPart,
        LanguageModelAssistantMessage, LanguageModelCallOptions, LanguageModelFilePart,
        LanguageModelFinishReason, LanguageModelMessage, LanguageModelProviderTool,
        LanguageModelReasoningEffort, LanguageModelReasoningPart, LanguageModelStreamFinish,
        LanguageModelStreamPart, LanguageModelStreamStart, LanguageModelTextPart,
        LanguageModelTool, LanguageModelToolCallPart, LanguageModelToolChoice,
        LanguageModelToolContentPart, LanguageModelToolMessage, LanguageModelToolResultOutput,
        LanguageModelToolResultPart, LanguageModelUsage, LanguageModelUserContentPart,
        LanguageModelUserMessage, OutputTokenUsage, ProviderAbortController,
    };
    use crate::open_responses::{OpenResponsesTransport, OpenResponsesTransportFuture};
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::prompt::Prompt;
    use crate::provider::{
        ModelType, Provider, ProviderOptions, ProviderWithFiles, ProviderWithVideoModel,
    };
    use crate::provider_utils::{
        FormDataValue, ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod,
        ProviderApiResponse,
    };
    use crate::video_model::{VideoModel, VideoModelCallOptions, VideoModelFile};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::env;
    use std::future::{Future, ready};
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

        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => break value,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    #[test]
    fn xai_provider_creates_responses_model_with_headers_base_url_and_body() {
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
                        "id": "resp_xai",
                        "created_at": 1711115037,
                        "model": "grok-4",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "Hello from xAI"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 5,
                            "output_tokens": 4
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = create_xai(
            XaiProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.xai.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_responses_transport(transport);
        let model = provider.language_model("grok-4");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16),
        ));

        assert_eq!(model.provider(), "xai.responses");
        assert_eq!(result.text, "Hello from xAI");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.xai.test/v1/responses");
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
                .is_some_and(|value| value.contains("ai-sdk/xai/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "grok-4",
                "input": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": "Say hello"
                            }
                        ]
                    }
                ],
                "max_output_tokens": 16
            }))
        );
    }

    #[test]
    fn xai_responses_model_prepares_server_tools_custom_tool_and_usage() {
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
                        "id": "resp_xai_tools",
                        "created_at": 1711115037,
                        "model": "grok-4",
                        "output": [
                            {
                                "type": "message",
                                "role": "assistant",
                                "content": [
                                    {
                                        "type": "output_text",
                                        "text": "xAI hosted tools prepared"
                                    }
                                ]
                            }
                        ],
                        "usage": {
                            "input_tokens": 10,
                            "input_tokens_details": {
                                "cached_tokens": 3
                            },
                            "output_tokens": 8,
                            "output_tokens_details": {
                                "reasoning_tokens": 2
                            }
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = XaiProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.xai.test/v1/")
            .with_responses_transport(transport);
        let model = provider.responses("grok-4");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Use hosted tools"))
                .expect("prompt is valid")
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.web_search",
                    "liveSearch",
                    JsonObject::new(),
                )))
                .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "openai.custom",
                    "write_sql",
                    JsonObject::from_iter([
                        (
                            "description".to_string(),
                            JsonValue::String("Write SQL statements.".to_string()),
                        ),
                        (
                            "format".to_string(),
                            json!({
                                "type": "grammar",
                                "syntax": "lark",
                                "definition": "start: SELECT"
                            }),
                        ),
                    ]),
                )))
                .with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "liveSearch".to_string(),
                }),
        ));

        assert_eq!(result.text, "xAI hosted tools prepared");
        assert_eq!(result.usage.input_tokens.total, Some(10));
        assert_eq!(result.usage.input_tokens.no_cache, Some(7));
        assert_eq!(result.usage.input_tokens.cache_read, Some(3));
        assert_eq!(result.usage.output_tokens.total, Some(8));
        assert_eq!(result.usage.output_tokens.text, Some(6));
        assert_eq!(result.usage.output_tokens.reasoning, Some(2));

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

        assert_eq!(request_body["model"], "grok-4");
        assert_eq!(
            request_body["tools"],
            json!([
                {
                    "type": "web_search"
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
        assert_eq!(request_body["tool_choice"], json!({ "type": "web_search" }));
    }

    #[test]
    fn xai_chat_request_maps_upstream_options_tools_warnings_and_usage() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "xai": {
                "reasoningEffort": "xhigh",
                "parallel_function_calling": false,
                "searchParameters": {
                    "mode": "on",
                    "returnCitations": true,
                    "fromDate": "2025-01-01",
                    "toDate": "2025-01-02",
                    "maxSearchResults": 3,
                    "sources": [
                        {
                            "type": "web",
                            "allowedWebsites": ["example.com"],
                            "safeSearch": true
                        },
                        {
                            "type": "x",
                            "xHandles": ["xai"]
                        }
                    ]
                },
                "topLogprobs": 5
            }
        }))
        .expect("provider options deserialize");
        let request_body = transform_xai_chat_request_body(json!({
            "model": "grok-3",
            "max_tokens": 128,
            "frequency_penalty": 0.2,
            "presence_penalty": 0.1,
            "stop": ["END"],
            "reasoning_effort": "xhigh",
            "topLogprobs": 5,
            "parallel_function_calling": false,
            "searchParameters": provider_options
                .get("xai")
                .and_then(|xai| xai.get("searchParameters"))
                .cloned()
                .expect("search parameters exist"),
            "messages": [
                {
                    "role": "user",
                    "content": "hello"
                }
            ],
            "tools": [
                {
                    "type": "function",
                    "function": {
                        "name": "lookup",
                        "parameters": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": {
                                "city": {
                                    "type": "string"
                                }
                            }
                        },
                        "strict": true
                    }
                }
            ]
        }));

        assert_eq!(request_body["max_completion_tokens"], json!(128));
        assert_eq!(request_body.get("max_tokens"), None);
        assert_eq!(request_body.get("frequency_penalty"), None);
        assert_eq!(request_body.get("presence_penalty"), None);
        assert_eq!(request_body.get("stop"), None);
        assert_eq!(request_body["reasoning_effort"], "high");
        assert_eq!(request_body["logprobs"], true);
        assert_eq!(request_body["top_logprobs"], 5);
        assert_eq!(
            request_body["tools"],
            json!([
                {
                    "type": "function",
                    "function": {
                        "name": "lookup",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "city": {
                                    "type": "string"
                                }
                            }
                        },
                        "strict": true
                    }
                }
            ])
        );
        assert_eq!(
            request_body["search_parameters"],
            json!({
                "mode": "on",
                "return_citations": true,
                "from_date": "2025-01-01",
                "to_date": "2025-01-02",
                "max_search_results": 3,
                "sources": [
                    {
                        "type": "web",
                        "safe_search": true,
                        "allowed_websites": ["example.com"]
                    },
                    {
                        "type": "x",
                        "x_handles": ["xai"]
                    }
                ]
            })
        );

        let mut options = LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
            LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                LanguageModelTextPart::new("hello"),
            )]),
        )])
        .with_top_k(40)
        .with_frequency_penalty(0.2)
        .with_presence_penalty(0.1)
        .with_stop_sequence("END")
        .with_reasoning(LanguageModelReasoningEffort::Xhigh)
        .with_provider_options(provider_options);
        let mut warnings = Vec::new();
        super::xai_append_chat_warnings(&mut warnings, &options);
        assert_eq!(
            warnings,
            vec![
                crate::warning::Warning::Unsupported {
                    feature: "topK".to_string(),
                    details: None,
                },
                crate::warning::Warning::Unsupported {
                    feature: "frequencyPenalty".to_string(),
                    details: None,
                },
                crate::warning::Warning::Unsupported {
                    feature: "presencePenalty".to_string(),
                    details: None,
                },
                crate::warning::Warning::Unsupported {
                    feature: "stopSequences".to_string(),
                    details: None,
                },
            ]
        );

        let mut stream = vec![LanguageModelStreamPart::StreamStart(
            LanguageModelStreamStart::new(Vec::new()),
        )];
        super::xai_append_stream_start_warnings(&mut stream, &options);
        assert!(matches!(
            &stream[0],
            LanguageModelStreamPart::StreamStart(start)
                if start.warnings.iter().any(|warning| {
                    matches!(
                        warning,
                        crate::warning::Warning::Unsupported { feature, .. }
                            if feature == "stopSequences"
                    )
                })
        ));

        options.reasoning = Some(LanguageModelReasoningEffort::None);
        let body = transform_xai_chat_request_body(json!({
            "reasoning_effort": "none"
        }));
        assert_eq!(body.get("reasoning_effort"), None);

        let usage = xai_chat_usage_from_raw(Some(
            json!({
                "prompt_tokens": 10,
                "prompt_tokens_details": {
                    "cached_tokens": 12
                },
                "completion_tokens": 7,
                "completion_tokens_details": {
                    "reasoning_tokens": 4
                }
            })
            .as_object()
            .expect("usage object"),
        ));
        assert_eq!(
            usage,
            LanguageModelUsage {
                input_tokens: InputTokenUsage {
                    total: Some(22),
                    no_cache: Some(0),
                    cache_read: Some(12),
                    cache_write: None,
                },
                output_tokens: OutputTokenUsage {
                    total: Some(11),
                    text: Some(7),
                    reasoning: Some(4),
                },
                raw: Some(
                    json!({
                        "prompt_tokens": 10,
                        "prompt_tokens_details": {
                            "cached_tokens": 12
                        },
                        "completion_tokens": 7,
                        "completion_tokens_details": {
                            "reasoning_tokens": 4
                        }
                    })
                    .as_object()
                    .expect("usage object")
                    .clone()
                ),
            }
        );
    }

    #[test]
    fn xai_chat_stream_maps_warnings_text_and_usage() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());
                let stream_body = [
                    json!({
                        "id": "chatcmpl-xai-stream",
                        "created": 1711115037,
                        "model": "grok-3",
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
                        "choices": [
                            {
                                "index": 0,
                                "delta": {
                                    "content": "Hel"
                                },
                                "finish_reason": null
                            }
                        ]
                    }),
                    json!({
                        "choices": [
                            {
                                "index": 0,
                                "delta": {
                                    "content": "lo"
                                },
                                "finish_reason": null
                            }
                        ]
                    }),
                    json!({
                        "choices": [
                            {
                                "index": 0,
                                "delta": {},
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 5,
                            "prompt_tokens_details": {
                                "cached_tokens": 2
                            },
                            "completion_tokens": 4,
                            "completion_tokens_details": {
                                "reasoning_tokens": 1
                            }
                        }
                    }),
                ]
                .into_iter()
                .map(|event| format!("data: {event}\n\n"))
                .chain(["data: [DONE]\n\n".to_string()])
                .collect::<String>();
                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", stream_body))))
            });
        let model = XaiProvider::new().with_transport(transport).chat("grok-3");
        let result =
            poll_ready(
                model.do_stream(
                    LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                        LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                            LanguageModelTextPart::new("hello"),
                        )]),
                    )])
                    .with_stop_sequence("END")
                    .with_tool(LanguageModelTool::Provider(LanguageModelProviderTool::new(
                        "xai.web_search",
                        "web_search",
                        JsonObject::new(),
                    ))),
                ),
            );

        assert!(matches!(
            result.stream.first(),
            Some(LanguageModelStreamPart::StreamStart(start))
                if start.warnings.iter().any(|warning| {
                    matches!(warning, crate::warning::Warning::Unsupported { feature, .. } if feature == "stopSequences")
                }) && start.warnings.iter().any(|warning| {
                    matches!(warning, crate::warning::Warning::Unsupported { feature, .. } if feature == "provider-defined tool xai.web_search")
                })
        ));
        assert_eq!(
            result
                .stream
                .iter()
                .filter_map(|part| match part {
                    LanguageModelStreamPart::TextDelta(delta) => Some(delta.delta.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec!["Hel", "lo"]
        );
        let finish = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => Some(finish),
                _ => None,
            })
            .expect("stream finish");
        assert_eq!(finish.usage.input_tokens.total, Some(5));
        assert_eq!(finish.usage.input_tokens.cache_read, Some(2));
        assert_eq!(finish.usage.output_tokens.total, Some(5));
        assert_eq!(finish.usage.output_tokens.reasoning, Some(1));
    }

    #[test]
    fn xai_chat_message_conversion_covers_text_images_tool_calls_and_results() {
        let image_url = Url::parse("https://example.com/cat.png").expect("url parses");
        let messages = vec![
            LanguageModelMessage::System(crate::language_model::LanguageModelSystemMessage::new(
                "You are concise.",
            )),
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Look")),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Url { url: image_url },
                    "image/png",
                )),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64("aW1hZ2U=".to_string()),
                    },
                    "image/jpeg",
                )),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64("SGVsbG8=".to_string()),
                    },
                    "text/plain",
                )),
            ])),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("Sure")),
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-1",
                    "lookup",
                    json!({ "city": "Brisbane" }),
                )),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-1",
                    "lookup",
                    LanguageModelToolResultOutput::Json {
                        value: json!({ "temp": 28 }),
                        provider_options: None,
                    },
                )),
            ])),
        ];

        assert_eq!(
            JsonValue::Array(xai_convert_chat_messages(&messages)),
            json!([
                {
                    "role": "system",
                    "content": "You are concise."
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "Look"
                        },
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": "https://example.com/cat.png"
                            }
                        },
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": "data:image/jpeg;base64,aW1hZ2U="
                            }
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": "Sure",
                    "tool_calls": [
                        {
                            "id": "call-1",
                            "type": "function",
                            "function": {
                                "name": "lookup",
                                "arguments": "{\"city\":\"Brisbane\"}"
                            }
                        }
                    ]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call-1",
                    "content": "{\"temp\":28}"
                }
            ])
        );
    }

    #[test]
    fn xai_responses_tool_ids_input_usage_stream_metadata_are_xai_specific() {
        let context = super::XaiResponsesRequestContext {
            tools: Some(vec![
                LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "xai.web_search",
                    "web",
                    JsonObject::from_iter([
                        ("allowedDomains".to_string(), json!(["example.com"])),
                        ("enableImageUnderstanding".to_string(), json!(true)),
                    ]),
                )),
                LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "xai.x_search",
                    "x_search",
                    JsonObject::from_iter([
                        ("allowedXHandles".to_string(), json!(["xai"])),
                        ("enableVideoUnderstanding".to_string(), json!(true)),
                    ]),
                )),
                LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "xai.code_execution",
                    "code_execution",
                    JsonObject::new(),
                )),
                LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "xai.view_image",
                    "view_image",
                    JsonObject::new(),
                )),
                LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "xai.view_x_video",
                    "view_x_video",
                    JsonObject::new(),
                )),
                LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "xai.file_search",
                    "file_search",
                    JsonObject::from_iter([("vectorStoreIds".to_string(), json!(["vs_1"]))]),
                )),
                LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "xai.mcp",
                    "mcp",
                    JsonObject::from_iter([
                        ("serverUrl".to_string(), json!("https://mcp.example.com")),
                        ("serverLabel".to_string(), json!("docs")),
                    ]),
                )),
                LanguageModelTool::Function(
                    crate::language_model::LanguageModelFunctionTool::new(
                        "local_lookup",
                        JsonObject::from_iter([(
                            "additionalProperties".to_string(),
                            JsonValue::Bool(false),
                        )]),
                    )
                    .with_strict(true),
                ),
            ]),
            tool_choice: Some(LanguageModelToolChoice::Tool {
                tool_name: "web".to_string(),
            }),
        };
        let request = ProviderApiRequest::post(
            "https://api.x.ai/v1/responses",
            crate::headers::Headers::new(),
            ProviderApiRequestBody::text(json!({ "model": "grok-4" }).to_string()),
            json!({ "model": "grok-4" }),
        );
        let request = xai_responses_request_with_tools(request, Some(context));
        let body: JsonValue = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str(body).ok())
            .expect("request body parses");

        assert_eq!(
            body["tools"],
            json!([
                {
                    "type": "web_search",
                    "allowed_domains": ["example.com"],
                    "enable_image_understanding": true
                },
                {
                    "type": "x_search",
                    "allowed_x_handles": ["xai"],
                    "enable_video_understanding": true
                },
                {
                    "type": "code_interpreter"
                },
                {
                    "type": "view_image"
                },
                {
                    "type": "view_x_video"
                },
                {
                    "type": "file_search",
                    "vector_store_ids": ["vs_1"]
                },
                {
                    "type": "mcp",
                    "server_url": "https://mcp.example.com",
                    "server_label": "docs"
                },
                {
                    "type": "function",
                    "name": "local_lookup",
                    "parameters": {},
                    "strict": true
                }
            ])
        );
        assert_eq!(body["tool_choice"], JsonValue::Null);

        let function_choice_context = super::XaiResponsesRequestContext {
            tools: Some(vec![LanguageModelTool::Function(
                crate::language_model::LanguageModelFunctionTool::new(
                    "local_lookup",
                    JsonObject::new(),
                ),
            )]),
            tool_choice: Some(LanguageModelToolChoice::Tool {
                tool_name: "local_lookup".to_string(),
            }),
        };
        let request = ProviderApiRequest::post(
            "https://api.x.ai/v1/responses",
            crate::headers::Headers::new(),
            ProviderApiRequestBody::text(json!({ "model": "grok-4" }).to_string()),
            json!({ "model": "grok-4" }),
        );
        let function_choice_request =
            xai_responses_request_with_tools(request, Some(function_choice_context));
        let function_choice_body: JsonValue = function_choice_request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str(body).ok())
            .expect("request body parses");
        assert_eq!(
            function_choice_body["tool_choice"],
            json!({
                "type": "function",
                "name": "local_lookup"
            })
        );

        let usage = xai_responses_usage_from_raw(Some(
            json!({
                "input_tokens": 10,
                "input_tokens_details": {
                    "cached_tokens": 12
                },
                "output_tokens": 8,
                "output_tokens_details": {
                    "reasoning_tokens": 3
                },
                "cost_in_usd_ticks": 113500
            })
            .as_object()
            .expect("usage object"),
        ));
        assert_eq!(usage.input_tokens.total, Some(22));
        assert_eq!(usage.input_tokens.no_cache, Some(0));
        assert_eq!(usage.output_tokens.text, Some(5));
        assert_eq!(usage.raw.as_ref().unwrap()["cost_in_usd_ticks"], 113500);

        let raw_stream_usage = json!({
            "input_tokens": 3,
            "input_tokens_details": {
                "cached_tokens": 1
            },
            "output_tokens": 9,
            "output_tokens_details": {
                "reasoning_tokens": 4
            },
            "cost_in_usd_ticks": 55
        });
        let mut stream = vec![LanguageModelStreamPart::Finish(
            LanguageModelStreamFinish::new(
                LanguageModelUsage {
                    raw: Some(raw_stream_usage.as_object().expect("usage object").clone()),
                    ..Default::default()
                },
                LanguageModelFinishReason {
                    unified: FinishReason::Stop,
                    raw: Some("stop".to_string()),
                },
            ),
        )];
        super::xai_adjust_stream_usage(&mut stream, xai_responses_usage_from_raw);
        let LanguageModelStreamPart::Finish(finish) = &stream[0] else {
            panic!("stream finish part is preserved");
        };
        assert_eq!(finish.usage.input_tokens.total, Some(3));
        assert_eq!(finish.usage.input_tokens.cache_read, Some(1));
        assert_eq!(finish.usage.output_tokens.text, Some(5));
        assert_eq!(
            finish
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("xai"))
                .and_then(|metadata| metadata.get("costInUsdTicks")),
            Some(&json!(55))
        );

        let reference = ProviderReference::try_from(BTreeMap::from([(
            "xai".to_string(),
            "file-123".to_string(),
        )]))
        .expect("reference valid");
        let input_messages = vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("hello")),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Reference { reference },
                    "application/pdf",
                )),
            ])),
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("hi")),
                LanguageModelAssistantContentPart::ToolCall(LanguageModelToolCallPart::new(
                    "call-1",
                    "lookup",
                    json!({ "q": "xai" }),
                )),
                LanguageModelAssistantContentPart::ToolCall(
                    LanguageModelToolCallPart::new("server-1", "web", json!({}))
                        .with_provider_executed(true),
                ),
                LanguageModelAssistantContentPart::Reasoning(LanguageModelReasoningPart::new(
                    "thinking",
                )),
            ])),
            LanguageModelMessage::Tool(LanguageModelToolMessage::new(vec![
                LanguageModelToolContentPart::ToolResult(LanguageModelToolResultPart::new(
                    "call-1",
                    "lookup",
                    LanguageModelToolResultOutput::Text {
                        value: "ok".to_string(),
                        provider_options: None,
                    },
                )),
            ])),
        ];
        assert_eq!(
            JsonValue::Array(xai_convert_responses_input(&input_messages)),
            json!([
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": "hello"
                        },
                        {
                            "type": "input_file",
                            "file_id": "file-123"
                        }
                    ]
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "hi"
                        }
                    ]
                },
                {
                    "type": "function_call",
                    "call_id": "call-1",
                    "name": "lookup",
                    "arguments": "{\"q\":\"xai\"}"
                },
                {
                    "type": "reasoning",
                    "summary": [
                        {
                            "type": "summary_text",
                            "text": "thinking"
                        }
                    ]
                },
                {
                    "type": "function_call_output",
                    "call_id": "call-1",
                    "output": "ok"
                }
            ])
        );
    }

    #[test]
    fn xai_responses_web_search_tool_maps_enable_image_search() {
        let context = super::XaiResponsesRequestContext {
            tools: Some(vec![LanguageModelTool::Provider(
                LanguageModelProviderTool::new(
                    "xai.web_search",
                    "web_search",
                    JsonObject::from_iter([("enableImageSearch".to_string(), json!(true))]),
                ),
            )]),
            tool_choice: None,
        };
        let request = ProviderApiRequest::post(
            "https://api.x.ai/v1/responses",
            crate::headers::Headers::new(),
            ProviderApiRequestBody::text(json!({ "model": "grok-4" }).to_string()),
            json!({ "model": "grok-4" }),
        );
        let request = xai_responses_request_with_tools(request, Some(context));
        let body: JsonValue = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str(body).ok())
            .expect("request body parses");

        assert_eq!(
            body["tools"],
            json!([
                {
                    "type": "web_search",
                    "enable_image_search": true
                }
            ])
        );
    }

    #[test]
    fn xai_files_image_and_video_models_cover_requests_metadata_and_errors() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_for_transport = Arc::clone(&captured_requests);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                captured_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned")
                    .push(request.clone());

                let response = if request.url.ends_with("/files") {
                    ProviderApiResponse::text(
                        200,
                        "OK",
                        json!({
                            "id": "file-123",
                            "filename": "spec.pdf",
                            "bytes": 2048,
                            "created_at": 1711115037
                        })
                        .to_string(),
                    )
                } else if request.url.ends_with("/images/edits") {
                    ProviderApiResponse::text(
                        200,
                        "OK",
                        json!({
                            "data": [
                                {
                                    "b64_json": "ZmFrZS1pbWFnZQ==",
                                    "revised_prompt": "A sharper cat"
                                }
                            ],
                            "usage": {
                                "cost_in_usd_ticks": 125
                            }
                        })
                        .to_string(),
                    )
                } else if request.url.ends_with("/videos/edits") {
                    ProviderApiResponse::text(
                        200,
                        "OK",
                        json!({
                            "request_id": "vid-123"
                        })
                        .to_string(),
                    )
                } else if request.url.ends_with("/videos/vid-123") {
                    ProviderApiResponse::text(
                        200,
                        "OK",
                        json!({
                            "status": "completed",
                            "video": {
                                "url": "https://cdn.x.ai/video.mp4"
                            },
                            "duration": 6,
                            "progress": 1,
                            "cost_in_usd_ticks": 42
                        })
                        .to_string(),
                    )
                } else {
                    ProviderApiResponse::text(
                        404,
                        "Not Found",
                        json!({ "error": { "message": "not found" } }).to_string(),
                    )
                };

                Box::pin(ready(Ok(response)))
            });
        let provider = XaiProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.xai.test/v1")
            .with_transport(transport);

        let mut file_options: ProviderOptions = ProviderOptions::new();
        file_options.insert(
            "xai".to_string(),
            JsonObject::from_iter([("teamId".to_string(), json!("team-1"))]),
        );
        let files = ProviderWithFiles::files(&provider);
        let file_result = poll_ready(
            files.upload_file(
                FilesUploadFileCallOptions::new(
                    FilesUploadFileData::data(FileDataContent::Base64("cGRm".to_string())),
                    "application/pdf",
                )
                .with_filename("spec.pdf")
                .with_provider_options(file_options),
            ),
        );
        assert_eq!(
            file_result
                .provider_reference
                .provider_id("xai")
                .expect("xai id"),
            "file-123"
        );
        assert_eq!(file_result.filename.as_deref(), Some("spec.pdf"));
        assert_eq!(
            file_result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("xai"))
                .and_then(|metadata| metadata.get("bytes")),
            Some(&json!(2048))
        );

        let mut image_options: ProviderOptions = ProviderOptions::new();
        image_options.insert(
            "xai".to_string(),
            JsonObject::from_iter([
                ("outputFormat".to_string(), json!("png")),
                ("resolution".to_string(), json!("2k")),
            ]),
        );
        let image = provider.image("grok-2-image");
        assert_eq!(poll_ready(image.max_images_per_call()), Some(3));
        let image_result = poll_ready(
            image.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A cat")
                    .with_size("1024x1024")
                    .with_aspect_ratio("16:9")
                    .with_seed(7)
                    .with_mask(ImageModelFile::file(
                        "image/png",
                        FileDataContent::Bytes(vec![9]),
                    ))
                    .with_files(vec![ImageModelFile::file(
                        "image/png",
                        FileDataContent::Bytes(vec![1, 2, 3]),
                    )])
                    .with_provider_options(image_options),
            ),
        );
        assert_eq!(
            image_result.images,
            vec![FileDataContent::Base64("ZmFrZS1pbWFnZQ==".to_string())]
        );
        assert_eq!(image_result.warnings.len(), 3);
        assert_eq!(
            image_result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("xai"))
                .map(|entry| (&entry.images, &entry.extra)),
            Some((
                &vec![json!({ "revisedPrompt": "A sharper cat" })],
                &JsonObject::from_iter([("costInUsdTicks".to_string(), json!(125))])
            ))
        );

        let mut video_options: ProviderOptions = ProviderOptions::new();
        video_options.insert(
            "xai".to_string(),
            JsonObject::from_iter([
                ("mode".to_string(), json!("edit-video")),
                ("videoUrl".to_string(), json!("https://cdn.x.ai/input.mp4")),
                ("resolution".to_string(), json!("720p")),
                ("pollIntervalMs".to_string(), json!(0)),
            ]),
        );
        let video = ProviderWithVideoModel::video_model(&provider, "grok-video")
            .expect("video model resolves");
        assert_eq!(poll_ready(video.max_videos_per_call()), Some(1));
        let video_result = poll_ready(
            video.do_generate(
                VideoModelCallOptions::new(2)
                    .with_prompt("Extend")
                    .with_duration(8.0)
                    .with_fps(24.0)
                    .with_seed(2)
                    .with_resolution("1280x720")
                    .with_image(VideoModelFile::file(
                        "image/png",
                        FileDataContent::Base64("aW1n".to_string()),
                    ))
                    .with_provider_options(video_options),
            ),
        );
        assert_eq!(video_result.warnings.len(), 5);
        for feature in ["fps", "seed", "n", "duration", "resolution"] {
            assert!(video_result.warnings.iter().any(|warning| {
                matches!(warning, crate::warning::Warning::Unsupported { feature: warned, .. } if warned == feature)
            }));
        }
        assert_eq!(
            serde_json::to_value(&video_result.videos).expect("videos serialize"),
            json!([
                {
                    "type": "url",
                    "url": "https://cdn.x.ai/video.mp4",
                    "mediaType": "video/mp4"
                }
            ])
        );
        assert_eq!(
            video_result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("xai"))
                .and_then(|metadata| metadata.get("costInUsdTicks")),
            Some(&json!(42))
        );

        let requests = captured_requests
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone();
        let file_form = requests[0]
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_form_data)
            .expect("file upload uses form data");
        assert_eq!(
            file_form.get("file"),
            Some(&FormDataValue::bytes(b"pdf".to_vec()))
        );
        assert_eq!(
            file_form.get("team_id"),
            Some(&FormDataValue::text("team-1"))
        );
        let image_body: JsonValue = requests[1]
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str(body).ok())
            .expect("image body parses");
        assert_eq!(requests[1].url, "https://api.xai.test/v1/images/edits");
        assert_eq!(image_body["image"]["url"], "data:image/png;base64,AQID");
        assert_eq!(image_body["aspect_ratio"], "16:9");
        assert_eq!(image_body["output_format"], "png");
        let video_body: JsonValue = requests[2]
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str(body).ok())
            .expect("video body parses");
        assert_eq!(requests[2].url, "https://api.xai.test/v1/videos/edits");
        assert_eq!(video_body["video"]["url"], "https://cdn.x.ai/input.mp4");
        assert_eq!(video_body.get("duration"), None);
        assert_eq!(requests[3].url, "https://api.xai.test/v1/videos/vid-123");

        let request_body = xai_image_request_body(
            "grok-2-image",
            &ImageModelCallOptions::new(2)
                .with_prompt("two refs")
                .with_files(vec![
                    ImageModelFile::url(Url::parse("https://example.com/1.png").expect("url")),
                    ImageModelFile::url(Url::parse("https://example.com/2.png").expect("url")),
                ]),
        );
        assert_eq!(
            request_body["images"],
            json!([
                {
                    "type": "image_url",
                    "url": "https://example.com/1.png"
                },
                {
                    "type": "image_url",
                    "url": "https://example.com/2.png"
                }
            ])
        );

        let (endpoint, text_video_body) = xai_video_request(
            "grok-video",
            &VideoModelCallOptions::new(1)
                .with_prompt("Generate")
                .with_aspect_ratio("16:9")
                .with_duration(5.0)
                .with_resolution("854x480"),
        );
        assert_eq!(endpoint, "videos/generations");
        assert_eq!(text_video_body["duration"], json!(5.0));
        assert_eq!(text_video_body["aspect_ratio"], "16:9");
        assert_eq!(text_video_body["resolution"], "480p");

        let unknown_resolution_warnings = xai_video_warnings(
            &VideoModelCallOptions::new(1)
                .with_prompt("Generate")
                .with_resolution("3840x2160"),
        );
        assert!(unknown_resolution_warnings.iter().any(|warning| {
            matches!(warning, crate::warning::Warning::Unsupported { feature, .. } if feature == "resolution")
        }));

        let mut extension_options: ProviderOptions = ProviderOptions::new();
        extension_options.insert(
            "xai".to_string(),
            JsonObject::from_iter([
                ("mode".to_string(), json!("extend-video")),
                ("videoUrl".to_string(), json!("https://cdn.x.ai/input.mp4")),
                ("resolution".to_string(), json!("720p")),
            ]),
        );
        let (extension_endpoint, extension_body) = xai_video_request(
            "grok-video",
            &VideoModelCallOptions::new(1)
                .with_prompt("Extend")
                .with_duration(6.0)
                .with_aspect_ratio("16:9")
                .with_resolution("1280x720")
                .with_provider_options(extension_options.clone()),
        );
        assert_eq!(extension_endpoint, "videos/extensions");
        assert_eq!(extension_body["video"]["url"], "https://cdn.x.ai/input.mp4");
        assert_eq!(extension_body["duration"], json!(6.0));
        assert_eq!(extension_body.get("aspect_ratio"), None);
        assert_eq!(extension_body.get("resolution"), None);
        let extension_warnings = xai_video_warnings(
            &VideoModelCallOptions::new(1)
                .with_aspect_ratio("16:9")
                .with_provider_options(extension_options),
        );
        assert!(extension_warnings.iter().any(|warning| {
            matches!(warning, crate::warning::Warning::Unsupported { feature, .. } if feature == "aspectRatio")
        }));
        assert!(extension_warnings.iter().any(|warning| {
            matches!(warning, crate::warning::Warning::Unsupported { feature, .. } if feature == "resolution")
        }));

        let mut reference_options: ProviderOptions = ProviderOptions::new();
        reference_options.insert(
            "xai".to_string(),
            JsonObject::from_iter([(
                "referenceImageUrls".to_string(),
                json!([
                    "https://example.com/ref.jpg",
                    "data:image/png;base64,iVBORw=="
                ]),
            )]),
        );
        let (reference_endpoint, reference_body) = xai_video_request(
            "grok-video",
            &VideoModelCallOptions::new(1)
                .with_prompt("Reference")
                .with_duration(8.0)
                .with_aspect_ratio("16:9")
                .with_provider_options(reference_options),
        );
        assert_eq!(reference_endpoint, "videos/generations");
        assert_eq!(
            reference_body["reference_images"],
            json!([
                { "url": "https://example.com/ref.jpg" },
                { "url": "data:image/png;base64,iVBORw==" }
            ])
        );
        assert_eq!(reference_body["duration"], json!(8.0));
        assert_eq!(reference_body["aspect_ratio"], "16:9");

        let mut empty_reference_options: ProviderOptions = ProviderOptions::new();
        empty_reference_options.insert(
            "xai".to_string(),
            JsonObject::from_iter([("referenceImageUrls".to_string(), json!([]))]),
        );
        assert!(
            xai_video_options_error(
                &VideoModelCallOptions::new(1).with_provider_options(empty_reference_options)
            )
            .expect("empty reference images are rejected")
            .contains("at least one")
        );
        let mut too_many_reference_options: ProviderOptions = ProviderOptions::new();
        too_many_reference_options.insert(
            "xai".to_string(),
            JsonObject::from_iter([(
                "referenceImageUrls".to_string(),
                JsonValue::Array(
                    (0..8)
                        .map(|index| json!(format!("https://example.com/{index}.jpg")))
                        .collect(),
                ),
            )]),
        );
        assert!(
            xai_video_options_error(
                &VideoModelCallOptions::new(1).with_provider_options(too_many_reference_options)
            )
            .expect("too many reference images are rejected")
            .contains("at most 7")
        );
        let mut blank_reference_options: ProviderOptions = ProviderOptions::new();
        blank_reference_options.insert(
            "xai".to_string(),
            JsonObject::from_iter([(
                "referenceImageUrls".to_string(),
                json!(["https://example.com/ref.jpg", ""]),
            )]),
        );
        assert!(
            xai_video_options_error(
                &VideoModelCallOptions::new(1).with_provider_options(blank_reference_options)
            )
            .expect("blank reference images are rejected")
            .contains("empty URLs")
        );

        assert!(matches!(
            xai_video_poll_result(
                "grok-video",
                "vid-123",
                json!({ "status": "pending" }),
                None,
                Vec::new()
            ),
            XaiVideoPollResult::Pending
        ));
        let moderation_result = xai_video_result_from_response(
            "grok-video",
            "vid-123",
            json!({
                "status": "done",
                "video": {
                    "url": "",
                    "respect_moderation": false
                }
            }),
            None,
            Vec::new(),
        );
        assert!(
            moderation_result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("xai"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str)
                .is_some_and(|message| message.contains("content policy"))
        );
        assert_eq!(
            xai_image_warnings(&ImageModelCallOptions::new(1)),
            Vec::new()
        );
        assert_eq!(
            xai_video_warnings(&VideoModelCallOptions::new(1)),
            Vec::new()
        );
    }

    #[test]
    fn xai_media_models_abort_before_request() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_for_transport = Arc::clone(&captured_requests);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                captured_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned")
                    .push(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({}).to_string(),
                ))))
            });
        let provider = XaiProvider::new().with_transport(transport);
        let abort_controller = ProviderAbortController::new();
        abort_controller.abort_with_reason("client disconnected");

        let image = poll_ready(
            provider.image("grok-2-image").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("aborted")
                    .with_abort_signal(abort_controller.signal()),
            ),
        );
        let video = poll_ready(
            provider.video("grok-video").do_generate(
                VideoModelCallOptions::new(1)
                    .with_prompt("aborted")
                    .with_abort_signal(abort_controller.signal()),
            ),
        );

        assert!(image.images.is_empty());
        assert!(video.videos.is_empty());
        assert!(
            captured_requests
                .lock()
                .expect("captured request mutex is not poisoned")
                .is_empty()
        );
    }

    #[test]
    fn xai_provider_creates_chat_model_with_openai_compatible_transport() {
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
                        "id": "chatcmpl-xai",
                        "created": 1711115037,
                        "model": "grok-3",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from xAI chat"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 4,
                            "total_tokens": 8
                        },
                        "citations": [
                            "https://example.com/source"
                        ]
                    })
                    .to_string(),
                ))))
            });
        let provider = XaiProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.xai.test/v1/")
            .with_transport(transport);
        let model = provider.chat("grok-3");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid"),
        ));

        assert_eq!(model.provider(), "xai.chat");
        assert_eq!(model.model_id(), "grok-3");
        assert_eq!(result.text, "Hello from xAI chat");
        assert_eq!(result.sources.len(), 1);

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.url, "https://api.xai.test/v1/chat/completions");
    }

    #[test]
    fn xai_provider_creates_image_model_and_reports_unsupported_embeddings() {
        let provider = XaiProvider::new();
        let default_model = xai("grok-4");
        let image = provider.image("grok-2-image");
        let embedding_error = Provider::embedding_model(&provider, "embed")
            .err()
            .expect("embedding models are unsupported");

        assert_eq!(default_model.provider(), "xai.responses");
        assert_eq!(image.provider(), "xai.image");
        assert_eq!(image.model_id(), "grok-2-image");
        assert_eq!(embedding_error.model_type(), ModelType::EmbeddingModel);
        assert_eq!(DEFAULT_XAI_BASE_URL, "https://api.x.ai/v1");
    }

    #[test]
    fn xai_provider_settings_serde_accepts_upstream_base_url() {
        let settings: XaiProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.xai.test/v1/",
            "apiKey": "key",
            "headers": {
                "x-provider": "xai"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            XaiProviderSettings::new()
                .with_base_url("https://api.xai.test/v1/")
                .with_api_key("key")
                .with_header("x-provider", "xai")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://api.xai.test/v1/",
                "apiKey": "key",
                "headers": {
                    "x-provider": "xai"
                }
            })
        );
    }

    #[test]
    fn xai_provider_implements_provider_trait() {
        let provider = XaiProvider::new();
        let model = Provider::language_model(&provider, "grok-4").expect("language model resolves");
        let image = Provider::image_model(&provider, "grok-2-image").expect("image resolves");
        let video =
            ProviderWithVideoModel::video_model(&provider, "grok-2-video").expect("video resolves");
        let files = ProviderWithFiles::files(&provider);

        assert_eq!(model.provider(), "xai.responses");
        assert_eq!(image.provider(), "xai.image");
        assert_eq!(video.provider(), "xai.video");
        assert_eq!(files.provider(), "xai.files");
    }

    #[test]
    #[ignore = "requires XAI_API_KEY and performs a live xAI Responses text-generation request"]
    fn live_xai_responses_generation_validates_provider_contract() {
        let Some(api_key) = live_xai_api_key() else {
            eprintln!("skipping live xAI Responses test: XAI_API_KEY is not set");
            return;
        };
        let provider = XaiProvider::new().with_api_key(api_key);
        let model_id = env::var("AI_SDK_RUST_XAI_RESPONSES_MODEL")
            .or_else(|_| env::var("XAI_RESPONSES_MODEL"))
            .unwrap_or_else(|_| "grok-4-fast-non-reasoning".to_string());
        let model = provider.responses(model_id);
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Reply with one word."))
                .expect("prompt is valid")
                .with_max_output_tokens(8),
        ));

        assert_eq!(model.provider(), "xai.responses");
        assert!(!result.text.trim().is_empty());
    }

    #[test]
    #[ignore = "requires XAI_API_KEY and performs live xAI Files, image, and video requests"]
    fn live_xai_files_image_video_validate_provider_contract() {
        let Some(api_key) = live_xai_api_key() else {
            eprintln!("skipping live xAI media test: XAI_API_KEY is not set");
            return;
        };
        let provider = XaiProvider::new().with_api_key(api_key);

        let files = provider.files();
        let file = poll_ready(files.upload_file(FilesUploadFileCallOptions::new(
            FilesUploadFileData::data(FileDataContent::Bytes(b"xai-live".to_vec())),
            "text/plain",
        )));
        assert!(file.provider_reference.provider_id("xai").is_ok());

        let image_model_id = env::var("AI_SDK_RUST_XAI_IMAGE_MODEL")
            .or_else(|_| env::var("XAI_IMAGE_MODEL"))
            .unwrap_or_else(|_| "grok-imagine-image".to_string());
        let image = poll_ready(
            provider
                .image(image_model_id)
                .do_generate(ImageModelCallOptions::new(1).with_prompt("A small blue cube")),
        );
        assert!(!image.images.is_empty());

        let video_model_id = env::var("AI_SDK_RUST_XAI_VIDEO_MODEL")
            .or_else(|_| env::var("XAI_VIDEO_MODEL"))
            .unwrap_or_else(|_| "grok-imagine-video".to_string());
        let mut video_options = ProviderOptions::new();
        video_options.insert(
            "xai".to_string(),
            JsonObject::from_iter([
                ("pollIntervalMs".to_string(), json!(5000)),
                ("pollTimeoutMs".to_string(), json!(600000)),
                ("resolution".to_string(), json!("480p")),
            ]),
        );
        let video = poll_ready(
            provider.video(video_model_id).do_generate(
                VideoModelCallOptions::new(1)
                    .with_prompt("A small blue cube rotating on a table")
                    .with_provider_options(video_options),
            ),
        );
        assert!(!video.videos.is_empty());
    }

    fn live_xai_api_key() -> Option<String> {
        env::var("XAI_API_KEY")
            .ok()
            .filter(|api_key| !api_key.is_empty())
    }
}
