use std::collections::BTreeMap;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use ai_sdk_rust::{
    FetchErrorInfo, FileData, FinishReason, GetFromApiOptions, HandledFetchError, Headers,
    InputTokenUsage, JsonObject, JsonValue, LanguageModel, LanguageModelAssistantContentPart,
    LanguageModelCallOptions, LanguageModelContent, LanguageModelErrorStreamPart,
    LanguageModelFilePart, LanguageModelFinishReason, LanguageModelGenerateResult,
    LanguageModelMessage, LanguageModelRawStreamPart, LanguageModelReasoning,
    LanguageModelReasoningDelta, LanguageModelReasoningEffort, LanguageModelReasoningEnd,
    LanguageModelReasoningStart, LanguageModelRequest, LanguageModelResponse,
    LanguageModelResponseFormat, LanguageModelStreamFinish, LanguageModelStreamPart,
    LanguageModelStreamResponseMetadata, LanguageModelStreamResult,
    LanguageModelStreamResultResponse, LanguageModelStreamStart, LanguageModelSupportedUrls,
    LanguageModelText, LanguageModelTextDelta, LanguageModelTextEnd, LanguageModelTextStart,
    LanguageModelTool, LanguageModelToolCall, LanguageModelToolChoice,
    LanguageModelToolContentPart, LanguageModelToolResultOutput, LanguageModelUsage,
    LanguageModelUserContentPart, LoadApiKeyError, LoadApiKeyOptions, ModelType, NoSuchModelError,
    OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel, ParseJsonResult,
    PostJsonToApiOptions, Provider, ProviderApiRequest, ProviderApiRequestBody,
    ProviderApiRequestMethod, ProviderApiResponse, ProviderApiResponseHandlerError,
    ProviderMetadata, ProviderOptions, ProviderWithVideoModel, ReasoningLevel, RuntimeEnvironment,
    StreamingToolCallDelta, StreamingToolCallTracker, UnsupportedFunctionalityError, VideoModel,
    VideoModelCallOptions, VideoModelFile, VideoModelResponse, VideoModelResult,
    VideoModelVideoData, Warning, combine_headers, convert_to_base64,
    create_event_source_response_handler, create_json_error_response_handler,
    create_json_response_handler, delay, generate_id, get_from_api, get_top_level_media_type,
    is_custom_reasoning, load_api_key, map_reasoning_to_provider_budget, parse_provider_options,
    post_json_to_api, resolve_full_media_type, with_user_agent_suffix, without_trailing_slash,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use url::Url;

/// Default OpenAI-compatible base URL for upstream `@ai-sdk/alibaba` chat calls.
pub const DEFAULT_ALIBABA_BASE_URL: &str = "https://dashscope-intl.aliyuncs.com/compatible-mode/v1";

/// Default DashScope native base URL for upstream `@ai-sdk/alibaba` video calls.
pub const DEFAULT_ALIBABA_VIDEO_BASE_URL: &str = "https://dashscope-intl.aliyuncs.com";

/// Default polling interval used by upstream Alibaba video generation.
pub const DEFAULT_ALIBABA_VIDEO_POLL_INTERVAL_MILLIS: u64 = 5_000;

/// Default polling timeout used by upstream Alibaba video generation.
pub const DEFAULT_ALIBABA_VIDEO_POLL_TIMEOUT_MILLIS: u64 = 600_000;

/// Settings for the upstream Alibaba Cloud DashScope provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlibabaProviderSettings {
    /// Base URL for OpenAI-compatible chat API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Base URL for DashScope native video generation API calls.
    #[serde(
        default,
        rename = "videoBaseURL",
        alias = "videoBaseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub video_base_url: Option<String>,

    /// Alibaba API key. When omitted, `ALIBABA_API_KEY` is read at request time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,

    /// Whether streamed chat responses should request final usage chunks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_usage: Option<bool>,
}

impl AlibabaProviderSettings {
    /// Creates empty Alibaba provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Alibaba API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets the OpenAI-compatible chat base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the DashScope native video base URL.
    pub fn with_video_base_url(mut self, video_base_url: impl Into<String>) -> Self {
        self.video_base_url = Some(video_base_url.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Sets whether stream requests should include usage.
    pub fn with_include_usage(mut self, include_usage: bool) -> Self {
        self.include_usage = Some(include_usage);
        self
    }
}

/// Upstream Alibaba provider foundation.
#[derive(Clone)]
pub struct AlibabaProvider {
    settings: AlibabaProviderSettings,
    transport: AlibabaTransport,
    current_date: AlibabaDateProvider,
}

/// Alibaba chat language model.
#[derive(Clone)]
pub struct AlibabaChatLanguageModel {
    model_id: String,
    base_url: String,
    settings: AlibabaProviderSettings,
    transport: AlibabaTransport,
}

/// Alibaba video model.
#[derive(Clone)]
pub struct AlibabaVideoModel {
    model_id: String,
    base_url: String,
    settings: AlibabaProviderSettings,
    transport: AlibabaTransport,
    current_date: AlibabaDateProvider,
}

/// Future returned by an injected Alibaba HTTP transport.
pub type AlibabaTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by Alibaba provider models.
pub type AlibabaTransport = Arc<dyn Fn(ProviderApiRequest) -> AlibabaTransportFuture + Send + Sync>;

type AlibabaDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type AlibabaChatSupportedUrlsFuture<'a> = Ready<LanguageModelSupportedUrls>;
type AlibabaChatGenerateFuture<'a> =
    Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>;
type AlibabaChatStreamFuture<'a> = Pin<
    Box<dyn Future<Output = LanguageModelStreamResult<Vec<LanguageModelStreamPart>>> + Send + 'a>,
>;
type AlibabaVideoMaxVideosFuture<'a> = Ready<Option<usize>>;
type AlibabaVideoGenerateFuture<'a> = Pin<Box<dyn Future<Output = VideoModelResult> + Send + 'a>>;

impl AlibabaProvider {
    /// Creates an Alibaba provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(AlibabaProviderSettings::new())
    }

    /// Creates a provider from explicit Alibaba settings.
    pub fn from_settings(settings: AlibabaProviderSettings) -> Self {
        Self {
            settings,
            transport: default_alibaba_transport(),
            current_date: default_alibaba_date_provider(),
        }
    }

    /// Sets the Alibaba API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the chat base URL for this provider.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.settings.base_url = Some(base_url.into());
        self
    }

    /// Sets the video base URL for this provider.
    pub fn with_video_base_url(mut self, video_base_url: impl Into<String>) -> Self {
        self.settings.video_base_url = Some(video_base_url.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: AlibabaTransport) -> Self {
        self.transport = transport;
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

    /// Creates an Alibaba chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> AlibabaChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Creates an Alibaba chat language model.
    pub fn chat(&self, model_id: impl Into<String>) -> AlibabaChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Creates an Alibaba chat language model.
    pub fn chat_model(&self, model_id: impl Into<String>) -> AlibabaChatLanguageModel {
        AlibabaChatLanguageModel::new(
            model_id,
            alibaba_chat_base_url(&self.settings),
            self.settings.clone(),
            Arc::clone(&self.transport),
        )
    }

    /// Creates an Alibaba video model.
    pub fn video(&self, model_id: impl Into<String>) -> AlibabaVideoModel {
        self.video_model(model_id)
            .expect("Alibaba video models are supported")
    }

    /// Creates an Alibaba video model.
    pub fn video_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<AlibabaVideoModel, NoSuchModelError> {
        Ok(AlibabaVideoModel::new(
            model_id,
            alibaba_video_base_url(&self.settings),
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Reports that Alibaba does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Deprecated upstream alias for embedding model lookup.
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        self.embedding_model(model_id)
    }

    /// Reports that Alibaba does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }
}

impl Default for AlibabaProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for AlibabaProvider {
    type LanguageModel = AlibabaChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(AlibabaProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        AlibabaProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        AlibabaProvider::image_model(self, model_id)
    }
}

impl ProviderWithVideoModel for AlibabaProvider {
    type VideoModel = AlibabaVideoModel;

    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
        AlibabaProvider::video_model(self, model_id)
    }
}

impl AlibabaChatLanguageModel {
    fn new(
        model_id: impl Into<String>,
        base_url: String,
        settings: AlibabaProviderSettings,
        transport: AlibabaTransport,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            base_url,
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
        "alibaba.chat"
    }

    async fn do_generate_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelGenerateResult {
        let (request_body, warnings) = match alibaba_chat_request_body(&self.model_id, &options) {
            Ok(result) => result,
            Err(message) => {
                return alibaba_chat_error_generate_result(
                    message,
                    json!({ "model": self.model_id }),
                    None,
                    Vec::new(),
                );
            }
        };
        let request_headers =
            match alibaba_request_headers(&self.settings, options.headers.as_ref()) {
                Ok(headers) => headers,
                Err(error) => {
                    return alibaba_chat_error_generate_result(
                        error.to_string(),
                        request_body,
                        None,
                        warnings,
                    );
                }
            };
        let request_body_for_response = request_body.clone();
        let request_body_for_error = request_body.clone();
        let url = format!("{}/chat/completions", self.base_url);
        let post_options = PostJsonToApiOptions::new(url, request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.transport);

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
                    alibaba_chat_error_data,
                    alibaba_chat_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => alibaba_chat_generate_result_from_response(
                response.value,
                response.raw_value,
                response.response_headers,
                request_body_for_response,
                warnings,
            ),
            Err(error) => alibaba_chat_result_from_error(error, request_body_for_error, warnings),
        }
    }

    async fn do_stream_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let include_raw_chunks = options.include_raw_chunks.unwrap_or(false);
        let (mut request_body, warnings) = match alibaba_chat_request_body(&self.model_id, &options)
        {
            Ok(result) => result,
            Err(message) => {
                return alibaba_chat_error_stream_result(
                    message,
                    json!({ "model": self.model_id }),
                    None,
                    None,
                    Vec::new(),
                );
            }
        };

        if let Some(body) = request_body.as_object_mut() {
            body.insert("stream".to_string(), JsonValue::Bool(true));
            if self.settings.include_usage.unwrap_or(true) {
                body.insert(
                    "stream_options".to_string(),
                    json!({ "include_usage": true }),
                );
            }
        }

        let request_headers =
            match alibaba_request_headers(&self.settings, options.headers.as_ref()) {
                Ok(headers) => headers,
                Err(error) => {
                    return alibaba_chat_error_stream_result(
                        error.to_string(),
                        request_body,
                        None,
                        None,
                        warnings,
                    );
                }
            };
        let request_body_for_response = request_body.clone();
        let request_body_for_error = request_body.clone();
        let url = format!("{}/chat/completions", self.base_url);
        let post_options = PostJsonToApiOptions::new(url, request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.transport);

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
                    alibaba_chat_error_data,
                    alibaba_chat_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => alibaba_chat_stream_result_from_response(
                response.value,
                response.response_headers,
                request_body_for_response,
                warnings,
                include_raw_chunks,
            ),
            Err(error) => {
                let (message, headers, body) = alibaba_handled_error_parts(error);
                alibaba_chat_error_stream_result(
                    message,
                    request_body_for_error,
                    headers,
                    body.as_deref(),
                    warnings,
                )
            }
        }
    }
}

impl LanguageModel for AlibabaChatLanguageModel {
    type SupportedUrlsFuture<'a>
        = AlibabaChatSupportedUrlsFuture<'a>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = AlibabaChatGenerateFuture<'a>
    where
        Self: 'a;

    type Stream = Vec<LanguageModelStreamPart>;

    type StreamFuture<'a>
        = AlibabaChatStreamFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        AlibabaChatLanguageModel::provider(self)
    }

    fn model_id(&self) -> &str {
        AlibabaChatLanguageModel::model_id(self)
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(LanguageModelSupportedUrls::from([(
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

impl AlibabaVideoModel {
    fn new(
        model_id: impl Into<String>,
        base_url: String,
        settings: AlibabaProviderSettings,
        transport: AlibabaTransport,
        current_date: AlibabaDateProvider,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            base_url,
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
        "alibaba.video"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: AlibabaTransport) -> Self {
        self.transport = transport;
        self
    }

    /// Returns a copy of this model that uses the supplied timestamp provider.
    pub fn with_current_date<F>(mut self, current_date: F) -> Self
    where
        F: Fn() -> OffsetDateTime + Send + Sync + 'static,
    {
        self.current_date = Arc::new(current_date);
        self
    }

    async fn do_generate_result(&self, options: VideoModelCallOptions) -> VideoModelResult {
        let timestamp = (self.current_date)();
        let (request_body, warnings, provider_options) =
            match alibaba_video_request_body(&self.model_id, &options) {
                Ok(result) => result,
                Err(message) => {
                    return alibaba_video_result_from_error(
                        &self.model_id,
                        message,
                        None,
                        timestamp,
                        Vec::new(),
                    );
                }
            };
        let request_headers =
            match alibaba_request_headers(&self.settings, options.headers.as_ref()) {
                Ok(headers) => headers,
                Err(error) => {
                    return alibaba_video_result_from_error(
                        &self.model_id,
                        error.to_string(),
                        None,
                        timestamp,
                        warnings,
                    );
                }
            };
        let create_headers = combine_headers([
            Some(
                request_headers
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect::<Vec<_>>(),
            ),
            Some(vec![(
                "X-DashScope-Async".to_string(),
                Some("enable".to_string()),
            )]),
        ]);
        let create_url = format!(
            "{}/api/v1/services/aigc/video-generation/video-synthesis",
            self.base_url
        );
        let create_options = PostJsonToApiOptions::new(create_url, request_body)
            .with_headers(create_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.transport);
        let create = match post_json_to_api(
            create_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    alibaba_video_create_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    alibaba_video_error_data,
                    alibaba_video_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => response.value,
            Err(error) => {
                let (message, _, _) = alibaba_handled_error_parts(error);
                return alibaba_video_result_from_error(
                    &self.model_id,
                    message,
                    None,
                    timestamp,
                    warnings,
                );
            }
        };
        let Some(task_id) = create
            .output
            .and_then(|output| output.task_id)
            .filter(|task_id| !task_id.is_empty())
        else {
            return alibaba_video_result_from_error(
                &self.model_id,
                format!(
                    "No task_id returned from Alibaba API. Response: {}",
                    serde_json::to_string(&create.raw).unwrap_or_else(|_| "{}".to_string())
                ),
                None,
                timestamp,
                warnings,
            );
        };

        match self
            .wait_for_video_completion(&task_id, &request_headers, &provider_options)
            .await
        {
            Ok((status, headers)) => alibaba_video_result_from_response(
                &self.model_id,
                &task_id,
                status,
                headers,
                timestamp,
                warnings,
            ),
            Err(message) => alibaba_video_result_from_error(
                &self.model_id,
                message,
                Some(task_id),
                timestamp,
                warnings,
            ),
        }
    }

    async fn wait_for_video_completion(
        &self,
        task_id: &str,
        headers: &BTreeMap<String, Option<String>>,
        provider_options: &AlibabaVideoProviderOptions,
    ) -> Result<(AlibabaVideoTaskStatusResponse, Option<Headers>), String> {
        let poll_interval = provider_options
            .poll_interval_millis
            .unwrap_or(DEFAULT_ALIBABA_VIDEO_POLL_INTERVAL_MILLIS);
        let poll_timeout = provider_options
            .poll_timeout_millis
            .unwrap_or(DEFAULT_ALIBABA_VIDEO_POLL_TIMEOUT_MILLIS);
        let started = Instant::now();
        let status_url = format!("{}/api/v1/tasks/{task_id}", self.base_url);

        loop {
            let transport = Arc::clone(&self.transport);
            let get_options = GetFromApiOptions::new(status_url.clone())
                .with_headers(headers.clone())
                .with_environment(RuntimeEnvironment::unknown());
            let response = get_from_api(
                get_options,
                move |request| (transport)(request),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        alibaba_video_task_status_response,
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| {
                    Ok(create_json_error_response_handler(
                        response.json_error_response_handler_options(request),
                        alibaba_video_error_data,
                        alibaba_video_error_message,
                        |_, _| None,
                    ))
                },
            )
            .await
            .map_err(|error| alibaba_handled_error_parts(error).0)?;

            let task_status = response
                .value
                .output
                .as_ref()
                .map(|output| output.task_status.as_str());

            match task_status {
                Some("SUCCEEDED") => return Ok((response.value, response.response_headers)),
                Some("FAILED") | Some("CANCELED") => {
                    let status = task_status.expect("status is present");
                    let message = response
                        .value
                        .output
                        .as_ref()
                        .and_then(|output| output.message.clone())
                        .unwrap_or_default();
                    return Err(format!(
                        "Video generation {}. Task ID: {task_id}. {message}",
                        status.to_lowercase()
                    ));
                }
                _ => {
                    if started.elapsed().as_millis() > u128::from(poll_timeout) {
                        return Err(format!("Video generation timed out after {poll_timeout}ms"));
                    }

                    if poll_interval > 0 {
                        delay(Some(poll_interval as i64)).await;
                    } else {
                        delay(None).await;
                    }
                }
            }
        }
    }
}

impl VideoModel for AlibabaVideoModel {
    type MaxVideosPerCallFuture<'a>
        = AlibabaVideoMaxVideosFuture<'a>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = AlibabaVideoGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        AlibabaVideoModel::provider(self)
    }

    fn model_id(&self) -> &str {
        AlibabaVideoModel::model_id(self)
    }

    fn max_videos_per_call(&self) -> Self::MaxVideosPerCallFuture<'_> {
        ready(Some(1))
    }

    fn do_generate(&self, options: VideoModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates an Alibaba provider with explicit settings.
pub fn create_alibaba(settings: AlibabaProviderSettings) -> AlibabaProvider {
    AlibabaProvider::from_settings(settings)
}

/// Creates an Alibaba chat language model using default provider settings.
pub fn alibaba(model_id: impl Into<String>) -> AlibabaChatLanguageModel {
    AlibabaProvider::new().language_model(model_id)
}

/// Provider-specific chat options accepted by upstream Alibaba.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlibabaChatProviderOptions {
    /// Enable thinking/reasoning mode for supported models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enable_thinking: Option<bool>,

    /// Maximum number of reasoning tokens to generate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_budget: Option<u64>,

    /// Whether Alibaba should allow parallel function calling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
}

/// Provider-specific video options accepted by upstream Alibaba.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlibabaVideoProviderOptions {
    /// Negative prompt to specify what to avoid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,

    /// URL to an audio file for audio-video sync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_url: Option<String>,

    /// Whether Alibaba should extend or rewrite the prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_extend: Option<bool>,

    /// Shot type, usually `single` or `multi`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shot_type: Option<String>,

    /// Whether to add a watermark to the generated video.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watermark: Option<bool>,

    /// Whether to generate audio for I2V/R2V models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<bool>,

    /// Reference URLs for reference-to-video mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_urls: Option<Vec<String>>,

    /// Poll interval in milliseconds.
    #[serde(
        default,
        rename = "pollIntervalMs",
        skip_serializing_if = "Option::is_none"
    )]
    pub poll_interval_millis: Option<u64>,

    /// Poll timeout in milliseconds.
    #[serde(
        default,
        rename = "pollTimeoutMs",
        skip_serializing_if = "Option::is_none"
    )]
    pub poll_timeout_millis: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AlibabaChatErrorData {
    error: AlibabaChatErrorBody,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AlibabaChatErrorBody {
    message: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    error_type: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AlibabaVideoErrorData {
    #[serde(default)]
    code: Option<String>,
    message: String,
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AlibabaVideoCreateTaskResponse {
    #[serde(default)]
    output: Option<AlibabaVideoCreateTaskOutput>,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(skip)]
    raw: JsonValue,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AlibabaVideoCreateTaskOutput {
    #[serde(default)]
    task_status: Option<String>,
    #[serde(default)]
    task_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AlibabaVideoTaskStatusResponse {
    #[serde(default)]
    output: Option<AlibabaVideoTaskStatusOutput>,
    #[serde(default)]
    usage: Option<AlibabaVideoUsage>,
    #[serde(default)]
    request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AlibabaVideoTaskStatusOutput {
    task_id: String,
    task_status: String,
    #[serde(default)]
    video_url: Option<String>,
    #[serde(default)]
    submit_time: Option<String>,
    #[serde(default)]
    scheduled_time: Option<String>,
    #[serde(default)]
    end_time: Option<String>,
    #[serde(default)]
    orig_prompt: Option<String>,
    #[serde(default)]
    actual_prompt: Option<String>,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AlibabaVideoUsage {
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    output_video_duration: Option<f64>,
    #[serde(default, rename = "SR")]
    sr: Option<f64>,
    #[serde(default)]
    size: Option<String>,
}

fn alibaba_chat_request_body(
    model_id: &str,
    options: &LanguageModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>), String> {
    let mut warnings = Vec::new();
    let alibaba_options = parse_provider_options(
        "alibaba",
        options.provider_options.as_ref(),
        alibaba_chat_provider_options,
    )
    .map_err(|error| error.to_string())?
    .unwrap_or_default();
    let mut cache_control_validator = CacheControlValidator::new();
    let mut body = JsonObject::new();

    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    insert_option_u64(&mut body, "max_tokens", options.max_output_tokens);
    insert_option_f64(&mut body, "temperature", options.temperature);
    insert_option_f64(&mut body, "top_p", options.top_p);
    insert_option_u64(&mut body, "top_k", options.top_k);
    insert_option_f64(&mut body, "presence_penalty", options.presence_penalty);
    insert_option_string_array(&mut body, "stop", options.stop_sequences.as_ref());
    insert_option_u64(&mut body, "seed", options.seed);

    if options.frequency_penalty.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "frequencyPenalty".to_string(),
            details: None,
        });
    }

    if let Some(response_format) = options.response_format.as_ref()
        && let Some(value) = alibaba_response_format(response_format)
    {
        body.insert("response_format".to_string(), value);
    }

    for (name, value) in
        alibaba_thinking_options(options.reasoning.as_ref(), &alibaba_options, &mut warnings)
    {
        body.insert(name, value);
    }

    body.insert(
        "messages".to_string(),
        JsonValue::Array(alibaba_chat_messages(
            &options.prompt,
            &mut cache_control_validator,
        )?),
    );

    let (tools, tool_choice) =
        alibaba_prepare_tools(&options.tools, &options.tool_choice, &mut warnings);
    if let Some(tools) = tools {
        body.insert("tools".to_string(), JsonValue::Array(tools));
        if let Some(parallel_tool_calls) = alibaba_options.parallel_tool_calls {
            body.insert(
                "parallel_tool_calls".to_string(),
                JsonValue::Bool(parallel_tool_calls),
            );
        }
    }
    if let Some(tool_choice) = tool_choice {
        body.insert("tool_choice".to_string(), tool_choice);
    }

    warnings.extend(cache_control_validator.into_warnings());

    Ok((JsonValue::Object(body), warnings))
}

fn alibaba_response_format(response_format: &LanguageModelResponseFormat) -> Option<JsonValue> {
    match response_format {
        LanguageModelResponseFormat::Text => None,
        LanguageModelResponseFormat::Json {
            schema,
            name,
            description,
        } => {
            if let Some(schema) = schema {
                let mut json_schema = JsonObject::new();
                json_schema.insert("schema".to_string(), JsonValue::Object(schema.clone()));
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
                Some(json!({
                    "type": "json_schema",
                    "json_schema": json_schema
                }))
            } else {
                Some(json!({ "type": "json_object" }))
            }
        }
    }
}

fn alibaba_thinking_options(
    reasoning: Option<&LanguageModelReasoningEffort>,
    alibaba_options: &AlibabaChatProviderOptions,
    warnings: &mut Vec<Warning>,
) -> Vec<(String, JsonValue)> {
    let mut options = Vec::new();
    if let Some(enable_thinking) = alibaba_options.enable_thinking {
        options.push((
            "enable_thinking".to_string(),
            JsonValue::Bool(enable_thinking),
        ));
    }
    if let Some(thinking_budget) = alibaba_options.thinking_budget {
        options.push(("thinking_budget".to_string(), json!(thinking_budget)));
    }
    if !options.is_empty() {
        return options;
    }

    if !is_custom_reasoning(reasoning) {
        return options;
    }

    if matches!(reasoning, Some(LanguageModelReasoningEffort::None)) {
        options.push(("enable_thinking".to_string(), JsonValue::Bool(false)));
        return options;
    }

    let Some(reasoning_level) =
        reasoning.and_then(|reasoning| ReasoningLevel::try_from(reasoning.clone()).ok())
    else {
        return options;
    };
    let thinking_budget =
        map_reasoning_to_provider_budget(reasoning_level, 16_384, 16_384, None, None, warnings);

    options.push(("enable_thinking".to_string(), JsonValue::Bool(true)));
    if let Some(thinking_budget) = thinking_budget {
        options.push(("thinking_budget".to_string(), json!(thinking_budget)));
    }

    options
}

fn alibaba_chat_messages(
    prompt: &[LanguageModelMessage],
    cache_control_validator: &mut CacheControlValidator,
) -> Result<Vec<JsonValue>, String> {
    let mut messages = Vec::new();

    for message in prompt {
        match message {
            LanguageModelMessage::System(message) => {
                let message_cache_control =
                    cache_control_validator.get_cache_control(message.provider_options.as_ref());
                let mut object = JsonObject::new();
                object.insert("role".to_string(), JsonValue::String("system".to_string()));
                object.insert(
                    "content".to_string(),
                    match message_cache_control {
                        Some(cache_control) => JsonValue::Array(vec![alibaba_text_content(
                            &message.content,
                            Some(cache_control),
                        )]),
                        None => JsonValue::String(message.content.clone()),
                    },
                );
                messages.push(JsonValue::Object(object));
            }
            LanguageModelMessage::User(message) => {
                let message_cache_control =
                    cache_control_validator.get_cache_control(message.provider_options.as_ref());
                let last_index = message.content.len().saturating_sub(1);
                let mut content = Vec::new();
                for (index, part) in message.content.iter().enumerate() {
                    let is_last = index == last_index;
                    let part_cache_control = alibaba_user_part_cache_control(
                        part,
                        message_cache_control.clone(),
                        is_last,
                        cache_control_validator,
                    );
                    content.push(alibaba_user_content_part(part, part_cache_control)?);
                }

                let mut object = JsonObject::new();
                object.insert("role".to_string(), JsonValue::String("user".to_string()));
                object.insert("content".to_string(), JsonValue::Array(content));
                messages.push(JsonValue::Object(object));
            }
            LanguageModelMessage::Assistant(message) => {
                let message_cache_control =
                    cache_control_validator.get_cache_control(message.provider_options.as_ref());
                let mut text = String::new();
                let mut tool_calls = Vec::new();

                for part in &message.content {
                    match part {
                        LanguageModelAssistantContentPart::Text(text_part) => {
                            text.push_str(&text_part.text);
                        }
                        LanguageModelAssistantContentPart::Reasoning(reasoning_part) => {
                            text.push_str(&reasoning_part.text);
                        }
                        LanguageModelAssistantContentPart::ToolCall(tool_call) => {
                            tool_calls.push(json!({
                                "id": tool_call.tool_call_id,
                                "type": "function",
                                "function": {
                                    "name": tool_call.tool_name,
                                    "arguments": tool_call.input.to_string()
                                }
                            }));
                        }
                        LanguageModelAssistantContentPart::File(_)
                        | LanguageModelAssistantContentPart::Custom(_)
                        | LanguageModelAssistantContentPart::ReasoningFile(_)
                        | LanguageModelAssistantContentPart::ToolResult(_)
                        | LanguageModelAssistantContentPart::ToolApprovalRequest(_) => {}
                    }
                }

                let mut object = JsonObject::new();
                object.insert(
                    "role".to_string(),
                    JsonValue::String("assistant".to_string()),
                );
                object.insert(
                    "content".to_string(),
                    match message_cache_control {
                        Some(cache_control) => {
                            JsonValue::Array(vec![alibaba_text_content(&text, Some(cache_control))])
                        }
                        None if text.is_empty() => JsonValue::Null,
                        None => JsonValue::String(text),
                    },
                );
                if !tool_calls.is_empty() {
                    object.insert("tool_calls".to_string(), JsonValue::Array(tool_calls));
                }
                messages.push(JsonValue::Object(object));
            }
            LanguageModelMessage::Tool(message) => {
                let message_cache_control =
                    cache_control_validator.get_cache_control(message.provider_options.as_ref());
                let tool_results = message
                    .content
                    .iter()
                    .filter_map(|part| match part {
                        LanguageModelToolContentPart::ToolResult(tool_result) => Some(tool_result),
                        LanguageModelToolContentPart::ToolApprovalResponse(_) => None,
                    })
                    .collect::<Vec<_>>();
                let last_index = tool_results.len().saturating_sub(1);

                for (index, tool_result) in tool_results.iter().enumerate() {
                    let part_cache_control = cache_control_validator
                        .get_cache_control(tool_result.provider_options.as_ref())
                        .or_else(|| {
                            if index == last_index {
                                message_cache_control.clone()
                            } else {
                                None
                            }
                        });
                    let content = alibaba_tool_result_content(&tool_result.output);
                    let mut object = JsonObject::new();
                    object.insert("role".to_string(), JsonValue::String("tool".to_string()));
                    object.insert(
                        "tool_call_id".to_string(),
                        JsonValue::String(tool_result.tool_call_id.clone()),
                    );
                    object.insert(
                        "content".to_string(),
                        match part_cache_control {
                            Some(cache_control) => JsonValue::Array(vec![alibaba_text_content(
                                &content,
                                Some(cache_control),
                            )]),
                            None => JsonValue::String(content),
                        },
                    );
                    messages.push(JsonValue::Object(object));
                }
            }
        }
    }

    Ok(messages)
}

fn alibaba_user_part_cache_control(
    part: &LanguageModelUserContentPart,
    message_cache_control: Option<JsonValue>,
    is_last: bool,
    cache_control_validator: &mut CacheControlValidator,
) -> Option<JsonValue> {
    let part_cache_control = match part {
        LanguageModelUserContentPart::Text(text) => {
            cache_control_validator.get_cache_control(text.provider_options.as_ref())
        }
        LanguageModelUserContentPart::File(file) => {
            cache_control_validator.get_cache_control(file.provider_options.as_ref())
        }
    };

    part_cache_control.or_else(|| if is_last { message_cache_control } else { None })
}

fn alibaba_user_content_part(
    part: &LanguageModelUserContentPart,
    cache_control: Option<JsonValue>,
) -> Result<JsonValue, String> {
    match part {
        LanguageModelUserContentPart::Text(text) => {
            Ok(alibaba_text_content(&text.text, cache_control))
        }
        LanguageModelUserContentPart::File(file) => {
            let url = alibaba_image_url(file)?;
            let mut object = JsonObject::new();
            object.insert(
                "type".to_string(),
                JsonValue::String("image_url".to_string()),
            );
            object.insert("image_url".to_string(), json!({ "url": url }));
            if let Some(cache_control) = cache_control {
                object.insert("cache_control".to_string(), cache_control);
            }
            Ok(JsonValue::Object(object))
        }
    }
}

fn alibaba_text_content(text: &str, cache_control: Option<JsonValue>) -> JsonValue {
    let mut object = JsonObject::new();
    object.insert("type".to_string(), JsonValue::String("text".to_string()));
    object.insert("text".to_string(), JsonValue::String(text.to_string()));
    if let Some(cache_control) = cache_control {
        object.insert("cache_control".to_string(), cache_control);
    }
    JsonValue::Object(object)
}

fn alibaba_image_url(part: &LanguageModelFilePart) -> Result<String, String> {
    match &part.data {
        FileData::Reference { .. } => {
            return Err(
                UnsupportedFunctionalityError::new("file parts with provider references")
                    .to_string(),
            );
        }
        FileData::Text { .. } => {
            return Err(UnsupportedFunctionalityError::new("text file parts").to_string());
        }
        FileData::Url { .. } | FileData::Data { .. } => {}
    }

    if get_top_level_media_type(&part.media_type) != "image" {
        return Err(
            UnsupportedFunctionalityError::new("Only image file parts are supported").to_string(),
        );
    }

    match &part.data {
        FileData::Url { url } => Ok(url.to_string()),
        FileData::Data { data } => Ok(format!(
            "data:{};base64,{}",
            resolve_full_media_type(part).map_err(|error| error.to_string())?,
            convert_to_base64(data)
        )),
        FileData::Reference { .. } | FileData::Text { .. } => unreachable!(),
    }
}

fn alibaba_tool_result_content(output: &LanguageModelToolResultOutput) -> String {
    match output {
        LanguageModelToolResultOutput::Text { value, .. }
        | LanguageModelToolResultOutput::ErrorText { value, .. } => value.clone(),
        LanguageModelToolResultOutput::ExecutionDenied { reason, .. } => reason
            .clone()
            .unwrap_or_else(|| "Tool call execution denied.".to_string()),
        LanguageModelToolResultOutput::Json { value, .. }
        | LanguageModelToolResultOutput::ErrorJson { value, .. } => value.to_string(),
        LanguageModelToolResultOutput::Content { value } => {
            serde_json::to_string(value).unwrap_or_else(|_| "[]".to_string())
        }
    }
}

fn alibaba_prepare_tools(
    tools: &Option<Vec<LanguageModelTool>>,
    tool_choice: &Option<LanguageModelToolChoice>,
    warnings: &mut Vec<Warning>,
) -> (Option<Vec<JsonValue>>, Option<JsonValue>) {
    let Some(tools) = tools.as_ref().filter(|tools| !tools.is_empty()) else {
        return (None, None);
    };

    let prepared_tools = tools
        .iter()
        .filter_map(|tool| match tool {
            LanguageModelTool::Function(tool) => {
                let mut function = JsonObject::new();
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
                Some(json!({ "type": "function", "function": function }))
            }
            LanguageModelTool::Provider(tool) => {
                warnings.push(Warning::Unsupported {
                    feature: format!("provider-defined tool {}", tool.id),
                    details: None,
                });
                None
            }
        })
        .collect::<Vec<_>>();

    let prepared_tool_choice = tool_choice.as_ref().map(|choice| match choice {
        LanguageModelToolChoice::Auto => JsonValue::String("auto".to_string()),
        LanguageModelToolChoice::None => JsonValue::String("none".to_string()),
        LanguageModelToolChoice::Required => JsonValue::String("required".to_string()),
        LanguageModelToolChoice::Tool { tool_name } => json!({
            "type": "function",
            "function": { "name": tool_name }
        }),
    });

    (Some(prepared_tools), prepared_tool_choice)
}

fn alibaba_video_request_body(
    model_id: &str,
    options: &VideoModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>, AlibabaVideoProviderOptions), String> {
    let provider_options = parse_provider_options(
        "alibaba",
        Some(&options.provider_options),
        alibaba_video_provider_options,
    )
    .map_err(|error| error.to_string())?
    .unwrap_or_default();
    let warnings = alibaba_video_warnings(options);
    let mode = alibaba_video_mode(model_id);
    let mut input = JsonObject::new();
    let mut parameters = JsonObject::new();

    insert_option_string(&mut input, "prompt", options.prompt.as_ref());
    insert_option_string(
        &mut input,
        "negative_prompt",
        provider_options.negative_prompt.as_ref(),
    );
    insert_option_string(&mut input, "audio_url", provider_options.audio_url.as_ref());

    if mode == AlibabaVideoMode::ImageToVideo
        && let Some(image) = options.image.as_ref()
    {
        input.insert(
            "img_url".to_string(),
            JsonValue::String(alibaba_video_image_url(image)),
        );
    }
    if mode == AlibabaVideoMode::ReferenceToVideo
        && let Some(reference_urls) = provider_options.reference_urls.as_ref()
    {
        input.insert("reference_urls".to_string(), json!(reference_urls));
    }

    insert_option_f64(&mut parameters, "duration", options.duration);
    insert_option_u64(&mut parameters, "seed", options.seed);
    if let Some(resolution) = options.resolution.as_ref() {
        match mode {
            AlibabaVideoMode::ImageToVideo => {
                parameters.insert(
                    "resolution".to_string(),
                    JsonValue::String(alibaba_i2v_resolution(resolution).to_string()),
                );
            }
            AlibabaVideoMode::TextToVideo | AlibabaVideoMode::ReferenceToVideo => {
                parameters.insert(
                    "size".to_string(),
                    JsonValue::String(resolution.replace('x', "*")),
                );
            }
        }
    }
    insert_option_bool(
        &mut parameters,
        "prompt_extend",
        provider_options.prompt_extend,
    );
    insert_option_string(
        &mut parameters,
        "shot_type",
        provider_options.shot_type.as_ref(),
    );
    insert_option_bool(&mut parameters, "watermark", provider_options.watermark);
    insert_option_bool(&mut parameters, "audio", provider_options.audio);

    Ok((
        json!({
            "model": model_id,
            "input": input,
            "parameters": parameters
        }),
        warnings,
        provider_options,
    ))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AlibabaVideoMode {
    TextToVideo,
    ImageToVideo,
    ReferenceToVideo,
}

fn alibaba_video_mode(model_id: &str) -> AlibabaVideoMode {
    if model_id.contains("-i2v") {
        AlibabaVideoMode::ImageToVideo
    } else if model_id.contains("-r2v") {
        AlibabaVideoMode::ReferenceToVideo
    } else {
        AlibabaVideoMode::TextToVideo
    }
}

fn alibaba_video_image_url(file: &VideoModelFile) -> String {
    match file {
        VideoModelFile::Url { url, .. } => url.as_str().to_string(),
        VideoModelFile::File { data, .. } => convert_to_base64(data),
    }
}

fn alibaba_i2v_resolution(resolution: &str) -> &str {
    match resolution {
        "1280x720" | "720x1280" | "960x960" | "1088x832" | "832x1088" => "720P",
        "1920x1080" | "1080x1920" | "1440x1440" | "1632x1248" | "1248x1632" => "1080P",
        "832x480" | "480x832" | "624x624" => "480P",
        _ => resolution,
    }
}

fn alibaba_video_warnings(options: &VideoModelCallOptions) -> Vec<Warning> {
    let mut warnings = Vec::new();
    if options.aspect_ratio.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "aspectRatio".to_string(),
            details: Some(
                "Alibaba video models use explicit size/resolution dimensions. Use the resolution option or providerOptions.alibaba for size control."
                    .to_string(),
            ),
        });
    }
    if options.fps.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "fps".to_string(),
            details: Some("Alibaba video models do not support custom FPS.".to_string()),
        });
    }
    if options.n > 1 {
        warnings.push(Warning::Unsupported {
            feature: "n".to_string(),
            details: Some(
                "Alibaba video models only support generating 1 video per call.".to_string(),
            ),
        });
    }
    warnings
}

fn alibaba_chat_generate_result_from_response(
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
    let mut content = Vec::new();

    if let Some(text) = message
        .and_then(|message| message.get("content"))
        .and_then(JsonValue::as_str)
        .filter(|text| !text.is_empty())
    {
        content.push(LanguageModelContent::Text(LanguageModelText::new(text)));
    }
    if let Some(reasoning) = message
        .and_then(|message| message.get("reasoning_content"))
        .and_then(JsonValue::as_str)
        .filter(|reasoning| !reasoning.is_empty())
    {
        content.push(LanguageModelContent::Reasoning(
            LanguageModelReasoning::new(reasoning),
        ));
    }
    if let Some(tool_calls) = message
        .and_then(|message| message.get("tool_calls"))
        .and_then(JsonValue::as_array)
    {
        for tool_call in tool_calls {
            let Some(function) = tool_call.get("function") else {
                continue;
            };
            let Some(tool_name) = function.get("name").and_then(JsonValue::as_str) else {
                continue;
            };
            let input = function
                .get("arguments")
                .and_then(JsonValue::as_str)
                .unwrap_or_default();
            let tool_call_id = tool_call
                .get("id")
                .and_then(JsonValue::as_str)
                .map(str::to_string)
                .unwrap_or_else(generate_id);

            content.push(LanguageModelContent::ToolCall(LanguageModelToolCall::new(
                tool_call_id,
                tool_name.to_string(),
                input.to_string(),
            )));
        }
    }

    let finish_reason =
        alibaba_finish_reason(choice.and_then(|choice| choice.get("finish_reason")));
    let usage = alibaba_chat_usage(response.get("usage"));
    let raw_body = raw_response.unwrap_or_else(|| response.clone());
    let mut result = LanguageModelGenerateResult::new(content, finish_reason, usage)
        .with_request(LanguageModelRequest::new().with_body(request_body));
    let mut response_metadata = LanguageModelResponse::new().with_body(raw_body);

    if let Some(id) = response.get("id").and_then(JsonValue::as_str) {
        response_metadata = response_metadata.with_id(id);
    }
    if let Some(timestamp) = alibaba_response_timestamp(response.get("created")) {
        response_metadata = response_metadata.with_timestamp(timestamp);
    }
    if let Some(model_id) = response.get("model").and_then(JsonValue::as_str) {
        response_metadata = response_metadata.with_model_id(model_id);
    }
    if let Some(headers) = response_headers {
        response_metadata = with_language_response_headers(response_metadata, headers);
    }
    for warning in warnings {
        result = result.with_warning(warning);
    }

    result.with_response(response_metadata)
}

fn alibaba_chat_stream_result_from_response(
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
    let mut active_text = false;
    let mut active_reasoning = false;
    let mut tool_call_tracker = StreamingToolCallTracker::new().with_generate_id(generate_id);

    for event in events {
        match event {
            ParseJsonResult::Success { value, raw_value } => {
                if include_raw_chunks {
                    stream.push(LanguageModelStreamPart::Raw(
                        LanguageModelRawStreamPart::new(raw_value.clone()),
                    ));
                }
                if is_first_chunk {
                    is_first_chunk = false;
                    stream.push(LanguageModelStreamPart::ResponseMetadata(
                        alibaba_stream_response_metadata(&value),
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
                    finish_reason = alibaba_finish_reason(Some(raw_finish_reason));
                }
                let Some(delta) = choice.get("delta") else {
                    continue;
                };

                if let Some(reasoning) = delta
                    .get("reasoning_content")
                    .and_then(JsonValue::as_str)
                    .filter(|reasoning| !reasoning.is_empty())
                {
                    if active_text {
                        stream.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
                            "0",
                        )));
                        active_text = false;
                    }
                    if !active_reasoning {
                        stream.push(LanguageModelStreamPart::ReasoningStart(
                            LanguageModelReasoningStart::new("reasoning-0"),
                        ));
                        active_reasoning = true;
                    }
                    stream.push(LanguageModelStreamPart::ReasoningDelta(
                        LanguageModelReasoningDelta::new("reasoning-0", reasoning),
                    ));
                }

                if let Some(text) = delta
                    .get("content")
                    .and_then(JsonValue::as_str)
                    .filter(|text| !text.is_empty())
                {
                    if active_reasoning {
                        stream.push(LanguageModelStreamPart::ReasoningEnd(
                            LanguageModelReasoningEnd::new("reasoning-0"),
                        ));
                        active_reasoning = false;
                    }
                    if !active_text {
                        stream.push(LanguageModelStreamPart::TextStart(
                            LanguageModelTextStart::new("0"),
                        ));
                        active_text = true;
                    }
                    stream.push(LanguageModelStreamPart::TextDelta(
                        LanguageModelTextDelta::new("0", text),
                    ));
                }

                if let Some(tool_calls) = delta.get("tool_calls").and_then(JsonValue::as_array) {
                    if active_reasoning {
                        stream.push(LanguageModelStreamPart::ReasoningEnd(
                            LanguageModelReasoningEnd::new("reasoning-0"),
                        ));
                        active_reasoning = false;
                    }
                    if active_text {
                        stream.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
                            "0",
                        )));
                        active_text = false;
                    }

                    for tool_call in tool_calls {
                        match serde_json::from_value::<StreamingToolCallDelta>(tool_call.clone())
                            .map_err(|error| error.to_string())
                            .and_then(|delta| {
                                tool_call_tracker
                                    .process_delta(delta)
                                    .map_err(|error| error.to_string())
                            }) {
                            Ok(parts) => stream.extend(parts),
                            Err(error) => {
                                finish_reason = LanguageModelFinishReason {
                                    unified: FinishReason::Error,
                                    raw: Some("alibaba-tool-call-error".to_string()),
                                };
                                stream.push(LanguageModelStreamPart::Error(
                                    LanguageModelErrorStreamPart::new(JsonValue::String(error)),
                                ));
                            }
                        }
                    }
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
                        "rawValue": raw_value
                    })),
                ));
            }
        }
    }

    if active_reasoning {
        stream.push(LanguageModelStreamPart::ReasoningEnd(
            LanguageModelReasoningEnd::new("reasoning-0"),
        ));
    }
    if active_text {
        stream.push(LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new(
            "0",
        )));
    }
    stream.extend(tool_call_tracker.flush());
    stream.push(LanguageModelStreamPart::Finish(
        LanguageModelStreamFinish::new(alibaba_chat_usage(usage.as_ref()), finish_reason),
    ));

    let mut result = LanguageModelStreamResult::new(stream)
        .with_request(LanguageModelRequest::new().with_body(request_body));
    if let Some(headers) = response_headers {
        let mut response = LanguageModelStreamResultResponse::new();
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
        result = result.with_response(response);
    }

    result
}

fn alibaba_video_result_from_response(
    model_id: &str,
    task_id: &str,
    response: AlibabaVideoTaskStatusResponse,
    headers: Option<Headers>,
    timestamp: OffsetDateTime,
    warnings: Vec<Warning>,
) -> VideoModelResult {
    let Some(output) = response.output else {
        return alibaba_video_result_from_error(
            model_id,
            format!("No video URL in response. Task ID: {task_id}"),
            Some(task_id.to_string()),
            timestamp,
            warnings,
        );
    };
    let Some(video_url) = output.video_url.clone() else {
        return alibaba_video_result_from_error(
            model_id,
            format!("No video URL in response. Task ID: {task_id}"),
            Some(task_id.to_string()),
            timestamp,
            warnings,
        );
    };
    let Ok(url) = Url::parse(&video_url) else {
        return alibaba_video_result_from_error(
            model_id,
            format!("No video URL in response. Task ID: {task_id}"),
            Some(task_id.to_string()),
            timestamp,
            warnings,
        );
    };

    let mut result = VideoModelResult::new(
        vec![VideoModelVideoData::url(url, "video/mp4")],
        alibaba_video_response(model_id, headers, timestamp),
    )
    .with_provider_metadata(alibaba_video_success_metadata(
        task_id,
        &video_url,
        output,
        response.usage,
    ));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn alibaba_video_result_from_error(
    model_id: &str,
    message: String,
    task_id: Option<String>,
    timestamp: OffsetDateTime,
    warnings: Vec<Warning>,
) -> VideoModelResult {
    let mut result = VideoModelResult::new(
        Vec::new(),
        alibaba_video_response(model_id, None, timestamp),
    )
    .with_provider_metadata(alibaba_video_error_metadata(message, task_id));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn alibaba_chat_error_generate_result(
    message: String,
    request_body: JsonValue,
    response_headers: Option<Headers>,
    warnings: Vec<Warning>,
) -> LanguageModelGenerateResult {
    let mut result = LanguageModelGenerateResult::new(
        Vec::new(),
        LanguageModelFinishReason {
            unified: FinishReason::Error,
            raw: None,
        },
        LanguageModelUsage::default(),
    )
    .with_request(LanguageModelRequest::new().with_body(request_body))
    .with_provider_metadata(alibaba_error_metadata(message));
    let mut response = LanguageModelResponse::new();
    if let Some(headers) = response_headers {
        response = with_language_response_headers(response, headers);
    }
    result = result.with_response(response);
    for warning in warnings {
        result = result.with_warning(warning);
    }
    result
}

fn alibaba_chat_result_from_error(
    error: HandledFetchError,
    request_body: JsonValue,
    warnings: Vec<Warning>,
) -> LanguageModelGenerateResult {
    let (message, headers, _) = alibaba_handled_error_parts(error);
    alibaba_chat_error_generate_result(message, request_body, headers, warnings)
}

fn alibaba_chat_error_stream_result(
    message: String,
    request_body: JsonValue,
    response_headers: Option<Headers>,
    response_body: Option<&str>,
    warnings: Vec<Warning>,
) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
    let mut stream = vec![
        LanguageModelStreamPart::StreamStart(LanguageModelStreamStart::new(warnings)),
        LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(json!({
            "message": message,
            "responseBody": response_body
        }))),
        LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(
            LanguageModelUsage::default(),
            LanguageModelFinishReason {
                unified: FinishReason::Error,
                raw: None,
            },
        )),
    ];
    let mut result = LanguageModelStreamResult::new(std::mem::take(&mut stream))
        .with_request(LanguageModelRequest::new().with_body(request_body));
    if let Some(headers) = response_headers {
        let mut response = LanguageModelStreamResultResponse::new();
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
        result = result.with_response(response);
    }
    result
}

fn alibaba_chat_usage(value: Option<&JsonValue>) -> LanguageModelUsage {
    let Some(value) = value else {
        return LanguageModelUsage::default();
    };
    let input_total = json_u64(value.get("prompt_tokens")).unwrap_or_default();
    let output_total = json_u64(value.get("completion_tokens")).unwrap_or_default();
    let cache_read = json_u64(
        value
            .get("prompt_tokens_details")
            .and_then(|details| details.get("cached_tokens")),
    )
    .unwrap_or_default();
    let cache_write = json_u64(
        value
            .get("prompt_tokens_details")
            .and_then(|details| details.get("cache_creation_input_tokens")),
    )
    .unwrap_or_default();
    let reasoning_tokens = json_u64(
        value
            .get("completion_tokens_details")
            .and_then(|details| details.get("reasoning_tokens")),
    )
    .unwrap_or_default();

    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: Some(input_total),
            no_cache: Some(input_total.saturating_sub(cache_read + cache_write)),
            cache_read: Some(cache_read),
            cache_write: Some(cache_write),
        },
        output_tokens: ai_sdk_rust::OutputTokenUsage {
            total: Some(output_total),
            text: Some(output_total.saturating_sub(reasoning_tokens)),
            reasoning: Some(reasoning_tokens),
        },
        raw: value.as_object().cloned(),
    }
}

fn alibaba_finish_reason(value: Option<&JsonValue>) -> LanguageModelFinishReason {
    let raw = value.and_then(JsonValue::as_str).map(str::to_string);
    let unified = match raw.as_deref() {
        Some("stop") => FinishReason::Stop,
        Some("length") => FinishReason::Length,
        Some("content_filter") => FinishReason::ContentFilter,
        Some("tool_calls") => FinishReason::ToolCalls,
        Some("error") => FinishReason::Error,
        _ => FinishReason::Other,
    };

    LanguageModelFinishReason { unified, raw }
}

fn alibaba_stream_response_metadata(value: &JsonValue) -> LanguageModelStreamResponseMetadata {
    let mut metadata = LanguageModelStreamResponseMetadata::new();
    if let Some(id) = value.get("id").and_then(JsonValue::as_str) {
        metadata = metadata.with_id(id);
    }
    if let Some(timestamp) = alibaba_response_timestamp(value.get("created")) {
        metadata = metadata.with_timestamp(timestamp);
    }
    if let Some(model_id) = value.get("model").and_then(JsonValue::as_str) {
        metadata = metadata.with_model_id(model_id);
    }
    metadata
}

fn alibaba_response_timestamp(value: Option<&JsonValue>) -> Option<OffsetDateTime> {
    value
        .and_then(JsonValue::as_i64)
        .and_then(|timestamp| OffsetDateTime::from_unix_timestamp(timestamp).ok())
}

fn alibaba_video_response(
    model_id: &str,
    headers: Option<Headers>,
    timestamp: OffsetDateTime,
) -> VideoModelResponse {
    let mut response = VideoModelResponse::new(timestamp, model_id);
    if let Some(headers) = headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }
    response
}

fn alibaba_video_success_metadata(
    task_id: &str,
    video_url: &str,
    output: AlibabaVideoTaskStatusOutput,
    usage: Option<AlibabaVideoUsage>,
) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("taskId".to_string(), JsonValue::String(task_id.to_string()));
    provider.insert(
        "videoUrl".to_string(),
        JsonValue::String(video_url.to_string()),
    );
    if let Some(actual_prompt) = output.actual_prompt {
        provider.insert("actualPrompt".to_string(), JsonValue::String(actual_prompt));
    }
    if let Some(usage) = usage {
        let mut usage_object = JsonObject::new();
        insert_option_f64(&mut usage_object, "duration", usage.duration);
        insert_option_f64(
            &mut usage_object,
            "outputVideoDuration",
            usage.output_video_duration,
        );
        insert_option_f64(&mut usage_object, "resolution", usage.sr);
        insert_option_string(&mut usage_object, "size", usage.size.as_ref());
        provider.insert("usage".to_string(), JsonValue::Object(usage_object));
    }

    metadata.insert("alibaba".to_string(), provider);
    metadata
}

fn alibaba_video_error_metadata(message: String, task_id: Option<String>) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("errorMessage".to_string(), JsonValue::String(message));
    if let Some(task_id) = task_id {
        provider.insert("taskId".to_string(), JsonValue::String(task_id));
    }
    metadata.insert("alibaba".to_string(), provider);
    metadata
}

fn alibaba_error_metadata(message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert("alibaba".to_string(), provider);
    metadata
}

#[derive(Clone, Debug, Default)]
struct CacheControlValidator {
    breakpoint_count: usize,
    warnings: Vec<Warning>,
}

impl CacheControlValidator {
    fn new() -> Self {
        Self::default()
    }

    fn get_cache_control(
        &mut self,
        provider_options: Option<&ProviderOptions>,
    ) -> Option<JsonValue> {
        let cache_control = provider_options
            .and_then(|provider_options| provider_options.get("alibaba"))
            .and_then(|alibaba| {
                alibaba
                    .get("cacheControl")
                    .or_else(|| alibaba.get("cache_control"))
            })
            .cloned();

        if cache_control.is_some() {
            self.breakpoint_count += 1;
            if self.breakpoint_count > 4 {
                self.warnings.push(Warning::Other {
                    message:
                        "Max breakpoint limit exceeded. Only the last 4 cache markers will take effect."
                            .to_string(),
                });
            }
        }

        cache_control
    }

    fn into_warnings(self) -> Vec<Warning> {
        self.warnings
    }
}

fn alibaba_chat_provider_options(value: &JsonValue) -> Result<AlibabaChatProviderOptions, String> {
    let options = serde_json::from_value::<AlibabaChatProviderOptions>(value.clone())
        .map_err(|error| error.to_string())?;
    if options.thinking_budget == Some(0) {
        return Err("thinkingBudget must be positive".to_string());
    }
    Ok(options)
}

fn alibaba_video_provider_options(
    value: &JsonValue,
) -> Result<AlibabaVideoProviderOptions, String> {
    let options = serde_json::from_value::<AlibabaVideoProviderOptions>(value.clone())
        .map_err(|error| error.to_string())?;
    if options
        .shot_type
        .as_deref()
        .is_some_and(|shot_type| shot_type != "single" && shot_type != "multi")
    {
        return Err("shotType must be 'single' or 'multi'".to_string());
    }
    if options.poll_interval_millis == Some(0) {
        return Err("pollIntervalMs must be positive".to_string());
    }
    if options.poll_timeout_millis == Some(0) {
        return Err("pollTimeoutMs must be positive".to_string());
    }
    Ok(options)
}

fn alibaba_request_headers(
    settings: &AlibabaProviderSettings,
    call_headers: Option<&Headers>,
) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
    let headers = alibaba_provider_headers(settings)?;

    Ok(combine_headers([
        Some(
            headers
                .into_iter()
                .map(|(name, value)| (name, Some(value)))
                .collect::<Vec<_>>(),
        ),
        optional_headers(call_headers),
    ]))
}

fn alibaba_provider_headers(
    settings: &AlibabaProviderSettings,
) -> Result<Headers, LoadApiKeyError> {
    let mut headers = vec![
        (
            "Authorization".to_string(),
            Some(format!(
                "Bearer {}",
                alibaba_api_key(settings.api_key.as_ref())?
            )),
        ),
        (
            "Content-Type".to_string(),
            Some("application/json".to_string()),
        ),
    ];

    headers.extend(
        settings
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    );

    Ok(with_user_agent_suffix(
        Some(headers),
        [format!("ai-sdk/alibaba/{}", ai_sdk_rust::VERSION)],
    ))
}

fn optional_headers(headers: Option<&Headers>) -> Option<Vec<(String, Option<String>)>> {
    headers.map(|headers| {
        headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone())))
            .collect()
    })
}

fn alibaba_api_key(explicit_api_key: Option<&String>) -> Result<String, LoadApiKeyError> {
    let mut options = LoadApiKeyOptions::new("ALIBABA_API_KEY", "Alibaba Cloud (DashScope)");
    if let Some(api_key) = explicit_api_key {
        options = options.with_api_key(api_key.clone());
    }
    load_api_key(options)
}

fn alibaba_chat_base_url(settings: &AlibabaProviderSettings) -> String {
    alibaba_base_url(settings.base_url.as_deref(), DEFAULT_ALIBABA_BASE_URL)
}

fn alibaba_video_base_url(settings: &AlibabaProviderSettings) -> String {
    alibaba_base_url(
        settings.video_base_url.as_deref(),
        DEFAULT_ALIBABA_VIDEO_BASE_URL,
    )
}

fn alibaba_base_url(value: Option<&str>, default: &str) -> String {
    let base_url = value.filter(|value| !value.is_empty()).unwrap_or(default);
    without_trailing_slash(Some(base_url))
        .unwrap_or(base_url)
        .to_string()
}

fn with_language_response_headers(
    mut response: LanguageModelResponse,
    headers: Headers,
) -> LanguageModelResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }
    response
}

fn clone_json_value(value: &JsonValue) -> Result<JsonValue, serde_json::Error> {
    Ok(value.clone())
}

fn alibaba_chat_error_data(value: &JsonValue) -> Result<AlibabaChatErrorData, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn alibaba_chat_error_message(data: &AlibabaChatErrorData) -> String {
    data.error.message.clone()
}

fn alibaba_video_error_data(value: &JsonValue) -> Result<AlibabaVideoErrorData, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn alibaba_video_error_message(data: &AlibabaVideoErrorData) -> String {
    data.message.clone()
}

fn alibaba_video_create_response(
    value: &JsonValue,
) -> Result<AlibabaVideoCreateTaskResponse, serde_json::Error> {
    let mut response = serde_json::from_value::<AlibabaVideoCreateTaskResponse>(value.clone())?;
    response.raw = value.clone();
    Ok(response)
}

fn alibaba_video_task_status_response(
    value: &JsonValue,
) -> Result<AlibabaVideoTaskStatusResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn alibaba_handled_error_parts(
    error: HandledFetchError,
) -> (String, Option<Headers>, Option<String>) {
    match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None, None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
            error.response_body().map(String::from),
        ),
    }
}

fn json_u64(value: Option<&JsonValue>) -> Option<u64> {
    match value? {
        JsonValue::Number(number) => number.as_u64(),
        JsonValue::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn insert_option_string(object: &mut JsonObject, name: &str, value: Option<&String>) {
    if let Some(value) = value {
        object.insert(name.to_string(), JsonValue::String(value.clone()));
    }
}

fn insert_option_string_array(object: &mut JsonObject, name: &str, value: Option<&Vec<String>>) {
    if let Some(value) = value {
        object.insert(name.to_string(), json!(value));
    }
}

fn insert_option_bool(object: &mut JsonObject, name: &str, value: Option<bool>) {
    if let Some(value) = value {
        object.insert(name.to_string(), JsonValue::Bool(value));
    }
}

fn insert_option_u64(object: &mut JsonObject, name: &str, value: Option<u64>) {
    if let Some(value) = value {
        object.insert(name.to_string(), json!(value));
    }
}

fn insert_option_f64(object: &mut JsonObject, name: &str, value: Option<f64>) {
    if let Some(value) = value {
        object.insert(name.to_string(), json!(value));
    }
}

fn default_alibaba_date_provider() -> AlibabaDateProvider {
    Arc::new(OffsetDateTime::now_utc)
}

fn default_alibaba_transport() -> AlibabaTransport {
    Arc::new(|request| Box::pin(ready(execute_alibaba_request(request))))
}

fn execute_alibaba_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_alibaba_get_request(request),
        ProviderApiRequestMethod::Post => execute_alibaba_post_request(request),
    }
}

fn execute_alibaba_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    alibaba_provider_api_response(response)
}

fn execute_alibaba_post_request(
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
                "multipart form data is not supported by the Alibaba transport",
            ));
        }
        None => builder.send_empty(),
    };

    alibaba_provider_api_response(response)
}

fn alibaba_provider_api_response(
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
    let body = response.body_mut().read_to_vec().map_err(|error| {
        FetchErrorInfo::new("failed to read response body")
            .with_name("Error")
            .with_cause_message(error.to_string())
    })?;

    Ok(ProviderApiResponse::bytes(status.as_u16(), status_text, body).with_headers(headers))
}

#[cfg(test)]
mod tests {
    use super::{
        AlibabaProvider, AlibabaProviderSettings, AlibabaTransport, AlibabaTransportFuture,
        DEFAULT_ALIBABA_BASE_URL, DEFAULT_ALIBABA_VIDEO_BASE_URL, alibaba, create_alibaba,
    };
    use ai_sdk_rust::{
        FileData, FileDataContent, FinishReason, JsonObject, LanguageModel,
        LanguageModelAssistantContentPart, LanguageModelAssistantMessage, LanguageModelCallOptions,
        LanguageModelContent, LanguageModelFilePart, LanguageModelFunctionTool,
        LanguageModelMessage, LanguageModelReasoningEffort, LanguageModelReasoningPart,
        LanguageModelStreamPart, LanguageModelTextPart, LanguageModelTool,
        LanguageModelToolCallPart, LanguageModelToolChoice, LanguageModelToolContentPart,
        LanguageModelToolMessage, LanguageModelToolResultOutput, LanguageModelToolResultPart,
        LanguageModelUserContentPart, LanguageModelUserMessage, ModelType, Provider,
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
        ProviderOptions, ProviderWithVideoModel, VideoModel, VideoModelCallOptions, VideoModelFile,
        VideoModelVideoData,
    };
    use serde_json::json;
    use std::future::{Future, ready};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use time::OffsetDateTime;
    use url::Url;

    struct NoopWake;

    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    fn poll_ready<F>(future: F) -> F::Output
    where
        F: Future,
    {
        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures use ready transports"),
        }
    }

    fn fixed_timestamp() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(0).expect("unix epoch is valid")
    }

    fn json_response(value: serde_json::Value) -> ProviderApiResponse {
        ProviderApiResponse::text(200, "OK", value.to_string())
    }

    fn provider_options(provider: &str, value: serde_json::Value) -> ProviderOptions {
        let mut options = ProviderOptions::new();
        options.insert(
            provider.to_string(),
            serde_json::from_value(value).expect("provider options are an object"),
        );
        options
    }

    fn request_body_json(request: &ProviderApiRequest) -> serde_json::Value {
        let Some(ProviderApiRequestBody::Text { content }) = request.body.as_ref() else {
            panic!("expected text request body");
        };

        serde_json::from_str(content).expect("request body is valid JSON")
    }

    fn chat_success_transport() -> (Arc<Mutex<Vec<ProviderApiRequest>>>, AlibabaTransport) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: AlibabaTransport = Arc::new(move |request| -> AlibabaTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());

            Box::pin(ready(Ok(json_response(json!({
                "id": "chatcmpl-alibaba",
                "created": 0,
                "model": "qwen-plus",
                "choices": [
                    {
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": "Final answer",
                            "reasoning_content": "Reasoned first",
                            "tool_calls": [
                                {
                                    "id": "call-1",
                                    "type": "function",
                                    "function": {
                                        "name": "get_weather",
                                        "arguments": "{\"city\":\"Brisbane\"}"
                                    }
                                }
                            ]
                        },
                        "finish_reason": "tool_calls"
                    }
                ],
                "usage": {
                    "prompt_tokens": 100,
                    "completion_tokens": 50,
                    "total_tokens": 150,
                    "prompt_tokens_details": {
                        "cached_tokens": 70,
                        "cache_creation_input_tokens": 20
                    },
                    "completion_tokens_details": {
                        "reasoning_tokens": 10
                    }
                }
            }))
            .with_headers(
                [("x-request-id".to_string(), "req-chat".to_string())]
                    .into_iter()
                    .collect(),
            ))))
        });

        (requests, transport)
    }

    #[test]
    fn alibaba_chat_model_builds_request_body_with_options_reasoning_tools_cache_and_usage() {
        let (requests, transport) = chat_success_transport();
        let provider = create_alibaba(
            AlibabaProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://example.com/compatible/v1/")
                .with_header("X-Provider", "provider"),
        )
        .with_transport(transport);
        let mut schema = JsonObject::new();
        schema.insert("type".to_string(), json!("object"));
        let result = poll_ready(
            provider.chat_model("qwen-plus").do_generate(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Hello").with_provider_options(
                            provider_options(
                                "alibaba",
                                json!({ "cacheControl": { "type": "ephemeral" } }),
                            ),
                        ),
                    )]),
                )])
                .with_max_output_tokens(128)
                .with_temperature(0.2)
                .with_top_p(0.9)
                .with_top_k(40)
                .with_presence_penalty(0.1)
                .with_frequency_penalty(0.3)
                .with_stop_sequence("stop")
                .with_seed(7)
                .with_reasoning(LanguageModelReasoningEffort::High)
                .with_provider_options(provider_options(
                    "alibaba",
                    json!({
                        "enableThinking": true,
                        "thinkingBudget": 2048,
                        "parallelToolCalls": false
                    }),
                ))
                .with_tool(LanguageModelTool::Function(LanguageModelFunctionTool::new(
                    "get_weather",
                    schema,
                )))
                .with_tool_choice(LanguageModelToolChoice::Tool {
                    tool_name: "get_weather".to_string(),
                })
                .with_header("X-Request", "request"),
            ),
        );

        assert_eq!(result.finish_reason.unified, FinishReason::ToolCalls);
        assert_eq!(result.usage.input_tokens.total, Some(100));
        assert_eq!(result.usage.input_tokens.cache_read, Some(70));
        assert_eq!(result.usage.input_tokens.cache_write, Some(20));
        assert_eq!(result.usage.input_tokens.no_cache, Some(10));
        assert_eq!(result.usage.output_tokens.total, Some(50));
        assert_eq!(result.usage.output_tokens.reasoning, Some(10));
        assert_eq!(result.usage.output_tokens.text, Some(40));
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| matches!(warning, ai_sdk_rust::Warning::Unsupported { feature, .. } if feature == "frequencyPenalty"))
        );
        assert!(matches!(
            &result.content[0],
            LanguageModelContent::Text(text) if text.text == "Final answer"
        ));
        assert!(matches!(
            &result.content[1],
            LanguageModelContent::Reasoning(reasoning) if reasoning.text == "Reasoned first"
        ));
        assert!(matches!(
            &result.content[2],
            LanguageModelContent::ToolCall(tool_call)
                if tool_call.tool_name == "get_weather"
                    && tool_call.input == "{\"city\":\"Brisbane\"}"
        ));

        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].url,
            "https://example.com/compatible/v1/chat/completions"
        );
        assert_eq!(
            requests[0].headers.get("authorization"),
            Some(&"Bearer test-api-key".to_string())
        );
        assert_eq!(
            requests[0].headers.get("x-provider"),
            Some(&"provider".to_string())
        );
        assert_eq!(
            requests[0].headers.get("x-request"),
            Some(&"request".to_string())
        );

        let body = request_body_json(&requests[0]);
        assert_eq!(body["model"], "qwen-plus");
        assert_eq!(body["max_tokens"], 128);
        assert_eq!(body["temperature"], 0.2);
        assert_eq!(body["top_p"], 0.9);
        assert_eq!(body["top_k"], 40);
        assert_eq!(body["presence_penalty"], 0.1);
        assert!(body.get("frequency_penalty").is_none());
        assert_eq!(body["stop"], json!(["stop"]));
        assert_eq!(body["seed"], 7);
        assert_eq!(body["enable_thinking"], true);
        assert_eq!(body["thinking_budget"], 2048);
        assert_eq!(body["parallel_tool_calls"], false);
        assert_eq!(body["tool_choice"]["function"]["name"], "get_weather");
        assert_eq!(
            body["messages"][0]["content"][0]["cache_control"],
            json!({ "type": "ephemeral" })
        );
    }

    #[test]
    fn alibaba_chat_model_converts_multimodal_assistant_and_tool_messages() {
        let (requests, transport) = chat_success_transport();
        let provider = create_alibaba(AlibabaProviderSettings::new().with_api_key("test-api-key"))
            .with_transport(transport);
        let tool_message_options = provider_options(
            "alibaba",
            json!({ "cache_control": { "type": "ephemeral" } }),
        );
        let result = poll_ready(provider.chat("qwen-plus").do_generate(
            LanguageModelCallOptions::new(vec![
                    LanguageModelMessage::System(
                        ai_sdk_rust::LanguageModelSystemMessage::new("System prompt")
                            .with_provider_options(provider_options(
                                "alibaba",
                                json!({ "cacheControl": { "type": "ephemeral" } }),
                            )),
                    ),
                    LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                        LanguageModelUserContentPart::Text(LanguageModelTextPart::new(
                            "What is in this image?",
                        )),
                        LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                            FileData::Data {
                                data: FileDataContent::Bytes(vec![0, 1, 2, 3]),
                            },
                            "image/png",
                        )),
                    ])),
                    LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                        LanguageModelAssistantContentPart::Reasoning(
                            LanguageModelReasoningPart::new("Prior reasoning. "),
                        ),
                        LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new(
                            "Prior text.",
                        )),
                        LanguageModelAssistantContentPart::ToolCall(
                            LanguageModelToolCallPart::new(
                                "call-1",
                                "get_weather",
                                json!({ "city": "Brisbane" }),
                            ),
                        ),
                    ])),
                    LanguageModelMessage::Tool(
                        ToolMessageBuilder::new(vec![
                            LanguageModelToolContentPart::ToolResult(
                                LanguageModelToolResultPart::new(
                                    "call-1",
                                    "get_weather",
                                    LanguageModelToolResultOutput::json(json!({ "temp": 24 })),
                                ),
                            ),
                            LanguageModelToolContentPart::ToolResult(
                                LanguageModelToolResultPart::new(
                                    "call-2",
                                    "get_time",
                                    LanguageModelToolResultOutput::text("2:30 PM"),
                                ),
                            ),
                        ])
                        .with_provider_options(tool_message_options),
                    ),
                ]),
        ));

        assert_eq!(result.finish_reason.unified, FinishReason::ToolCalls);
        let requests = requests.lock().expect("request list mutex is not poisoned");
        let body = request_body_json(&requests[0]);
        assert_eq!(
            body["messages"][0]["content"][0]["cache_control"],
            json!({ "type": "ephemeral" })
        );
        assert_eq!(
            body["messages"][1]["content"][1]["image_url"]["url"],
            "data:image/png;base64,AAECAw=="
        );
        assert_eq!(
            body["messages"][2],
            json!({
                "role": "assistant",
                "content": "Prior reasoning. Prior text.",
                "tool_calls": [
                    {
                        "id": "call-1",
                        "type": "function",
                        "function": {
                            "name": "get_weather",
                            "arguments": "{\"city\":\"Brisbane\"}"
                        }
                    }
                ]
            })
        );
        assert_eq!(body["messages"][3]["content"], "{\"temp\":24}");
        assert_eq!(
            body["messages"][4]["content"][0]["cache_control"],
            json!({ "type": "ephemeral" })
        );
    }

    struct ToolMessageBuilder;

    impl ToolMessageBuilder {
        fn new(content: Vec<LanguageModelToolContentPart>) -> LanguageModelToolMessage {
            LanguageModelToolMessage::new(content)
        }
    }

    #[test]
    fn alibaba_chat_model_streams_reasoning_text_tool_calls_usage_and_raw_chunks() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: AlibabaTransport = Arc::new(move |request| -> AlibabaTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request);
            let chunks = [
                json!({
                    "id": "stream-1",
                    "created": 0,
                    "model": "qwen-plus",
                    "choices": [
                        {
                            "index": 0,
                            "delta": { "reasoning_content": "Thinking" },
                            "finish_reason": null
                        }
                    ]
                })
                .to_string(),
                json!({
                    "id": "stream-1",
                    "created": 0,
                    "model": "qwen-plus",
                    "choices": [
                        {
                            "index": 0,
                            "delta": { "content": "Hello" },
                            "finish_reason": null
                        }
                    ]
                })
                .to_string(),
                json!({
                    "id": "stream-1",
                    "created": 0,
                    "model": "qwen-plus",
                    "choices": [
                        {
                            "index": 0,
                            "delta": {
                                "tool_calls": [
                                    {
                                        "index": 0,
                                        "id": "call-1",
                                        "type": "function",
                                        "function": {
                                            "name": "get_weather",
                                            "arguments": "{}"
                                        }
                                    }
                                ]
                            },
                            "finish_reason": "tool_calls"
                        }
                    ],
                    "usage": {
                        "prompt_tokens": 2,
                        "completion_tokens": 3,
                        "total_tokens": 5,
                        "prompt_tokens_details": {
                            "cached_tokens": 1,
                            "cache_creation_input_tokens": 0
                        },
                        "completion_tokens_details": {
                            "reasoning_tokens": 1
                        }
                    }
                })
                .to_string(),
            ];
            let body = format!(
                "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                chunks[0], chunks[1], chunks[2]
            );
            Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", body))))
        });
        let provider = create_alibaba(
            AlibabaProviderSettings::new()
                .with_api_key("test-api-key")
                .with_include_usage(false),
        )
        .with_transport(transport);

        let result = poll_ready(
            provider.chat("qwen-plus").do_stream(
                LanguageModelCallOptions::new(vec![LanguageModelMessage::User(
                    LanguageModelUserMessage::new(vec![LanguageModelUserContentPart::Text(
                        LanguageModelTextPart::new("Hello"),
                    )]),
                )])
                .with_include_raw_chunks(true),
            ),
        );

        assert!(matches!(result.stream[1], LanguageModelStreamPart::Raw(_)));
        assert!(result.stream.iter().any(|part| {
            matches!(part, LanguageModelStreamPart::ReasoningDelta(delta) if delta.delta == "Thinking")
        }));
        assert!(result.stream.iter().any(|part| {
            matches!(part, LanguageModelStreamPart::TextDelta(delta) if delta.delta == "Hello")
        }));
        assert!(result.stream.iter().any(|part| {
            matches!(part, LanguageModelStreamPart::ToolCall(tool_call) if tool_call.tool_name == "get_weather")
        }));
        let finish = result
            .stream
            .iter()
            .find_map(|part| match part {
                LanguageModelStreamPart::Finish(finish) => Some(finish),
                _ => None,
            })
            .expect("stream finish is emitted");
        assert_eq!(finish.finish_reason.unified, FinishReason::ToolCalls);
        assert_eq!(finish.usage.input_tokens.total, Some(2));
        assert_eq!(finish.usage.input_tokens.cache_read, Some(1));
        assert_eq!(finish.usage.output_tokens.reasoning, Some(1));

        let requests = requests.lock().expect("request list mutex is not poisoned");
        let body = request_body_json(&requests[0]);
        assert_eq!(body["stream"], true);
        assert!(body.get("stream_options").is_none());
    }

    fn video_success_transport() -> (Arc<Mutex<Vec<ProviderApiRequest>>>, AlibabaTransport) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: AlibabaTransport = Arc::new(move |request| -> AlibabaTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());

            let response = match (request.method, request.url.as_str()) {
                (
                    ProviderApiRequestMethod::Post,
                    "https://dashscope-intl.aliyuncs.com/api/v1/services/aigc/video-generation/video-synthesis",
                ) => json_response(json!({
                    "output": {
                        "task_status": "PENDING",
                        "task_id": "task-123"
                    },
                    "request_id": "req-create"
                })),
                (
                    ProviderApiRequestMethod::Get,
                    "https://dashscope-intl.aliyuncs.com/api/v1/tasks/task-123",
                ) => json_response(json!({
                    "output": {
                        "task_id": "task-123",
                        "task_status": "SUCCEEDED",
                        "video_url": "https://dashscope-result.oss.aliyuncs.com/output.mp4",
                        "actual_prompt": "Enhanced prompt"
                    },
                    "usage": {
                        "duration": 5.0,
                        "output_video_duration": 5,
                        "SR": 1080,
                        "size": "1920x1080"
                    },
                    "request_id": "req-status"
                }))
                .with_headers(
                    [("x-request-id".to_string(), "req-status".to_string())]
                        .into_iter()
                        .collect(),
                ),
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({ "message": "unexpected request" }).to_string(),
                ),
            };

            Box::pin(ready(Ok(response)))
        });

        (requests, transport)
    }

    #[test]
    fn alibaba_video_model_generates_video_with_headers_body_and_metadata() {
        let (requests, transport) = video_success_transport();
        let provider = create_alibaba(
            AlibabaProviderSettings::new()
                .with_api_key("test-api-key")
                .with_header("X-Provider", "provider"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);
        let result = poll_ready(
            provider.video("wan2.6-t2v").do_generate(
                VideoModelCallOptions::new(1)
                    .with_prompt("A serene mountain lake")
                    .with_resolution("1920x1080")
                    .with_duration(5.0)
                    .with_seed(42)
                    .with_provider_options(provider_options(
                        "alibaba",
                        json!({
                            "negativePrompt": "blurry",
                            "audioUrl": "https://example.com/audio.mp3",
                            "promptExtend": true,
                            "shotType": "multi",
                            "watermark": false
                        }),
                    ))
                    .with_header("X-Request", "request"),
            ),
        );

        assert_eq!(
            result.videos,
            vec![VideoModelVideoData::url(
                Url::parse("https://dashscope-result.oss.aliyuncs.com/output.mp4")
                    .expect("valid URL"),
                "video/mp4"
            )]
        );
        assert_eq!(result.response.timestamp, fixed_timestamp());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("alibaba"))
                .and_then(|metadata| metadata.get("taskId")),
            Some(&json!("task-123"))
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("alibaba"))
                .and_then(|metadata| metadata.get("actualPrompt")),
            Some(&json!("Enhanced prompt"))
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("alibaba"))
                .and_then(|metadata| metadata.get("usage"))
                .and_then(|usage| usage.get("resolution")),
            Some(&json!(1080.0))
        );

        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].headers.get("x-dashscope-async"),
            Some(&"enable".to_string())
        );
        assert_eq!(
            requests[0].headers.get("authorization"),
            Some(&"Bearer test-api-key".to_string())
        );
        assert_eq!(
            requests[0].headers.get("x-provider"),
            Some(&"provider".to_string())
        );
        assert_eq!(
            requests[0].headers.get("x-request"),
            Some(&"request".to_string())
        );
        assert!(requests[1].headers.get("x-dashscope-async").is_none());
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "model": "wan2.6-t2v",
                "input": {
                    "prompt": "A serene mountain lake",
                    "negative_prompt": "blurry",
                    "audio_url": "https://example.com/audio.mp3"
                },
                "parameters": {
                    "duration": 5.0,
                    "seed": 42,
                    "size": "1920*1080",
                    "prompt_extend": true,
                    "shot_type": "multi",
                    "watermark": false
                }
            })
        );
    }

    #[test]
    fn alibaba_video_model_maps_i2v_r2v_resolution_and_warnings() {
        let (requests, transport) = video_success_transport();
        let provider = create_alibaba(AlibabaProviderSettings::new().with_api_key("test-api-key"))
            .with_transport(transport);

        let i2v = poll_ready(
            provider.video("wan2.6-i2v-flash").do_generate(
                VideoModelCallOptions::new(3)
                    .with_prompt("Animate this")
                    .with_image(VideoModelFile::file(
                        "image/png",
                        FileDataContent::Bytes(vec![137, 80, 78, 71]),
                    ))
                    .with_resolution("1920x1080")
                    .with_aspect_ratio("16:9")
                    .with_fps(30.0)
                    .with_provider_options(provider_options("alibaba", json!({ "audio": false }))),
            ),
        );
        let r2v = poll_ready(
            provider.video("wan2.6-r2v").do_generate(
                VideoModelCallOptions::new(1)
                    .with_prompt("Use references")
                    .with_resolution("1280x720")
                    .with_provider_options(provider_options(
                        "alibaba",
                        json!({
                            "referenceUrls": [
                                "https://example.com/ref-image.jpg",
                                "https://example.com/ref-video.mp4"
                            ]
                        }),
                    )),
            ),
        );

        assert!(
            i2v.warnings
                .iter()
                .any(|warning| matches!(warning, ai_sdk_rust::Warning::Unsupported { feature, .. } if feature == "aspectRatio"))
        );
        assert!(
            i2v.warnings
                .iter()
                .any(|warning| matches!(warning, ai_sdk_rust::Warning::Unsupported { feature, .. } if feature == "fps"))
        );
        assert!(
            i2v.warnings
                .iter()
                .any(|warning| matches!(warning, ai_sdk_rust::Warning::Unsupported { feature, .. } if feature == "n"))
        );
        assert_eq!(r2v.warnings, Vec::new());

        let requests = requests.lock().expect("request list mutex is not poisoned");
        let i2v_body = request_body_json(&requests[0]);
        let r2v_body = request_body_json(&requests[2]);
        assert_eq!(i2v_body["model"], "wan2.6-i2v-flash");
        assert_eq!(i2v_body["input"]["img_url"], "iVBORw==");
        assert_eq!(i2v_body["parameters"]["resolution"], "1080P");
        assert_eq!(i2v_body["parameters"]["audio"], false);
        assert_eq!(r2v_body["model"], "wan2.6-r2v");
        assert_eq!(
            r2v_body["input"]["reference_urls"],
            json!([
                "https://example.com/ref-image.jpg",
                "https://example.com/ref-video.mp4"
            ])
        );
        assert_eq!(r2v_body["parameters"]["size"], "1280*720");
    }

    #[test]
    fn alibaba_video_model_maps_api_and_status_errors_to_metadata() {
        let transport: AlibabaTransport = Arc::new(move |request| -> AlibabaTransportFuture {
            let response = match (request.method, request.url.as_str()) {
                (
                    ProviderApiRequestMethod::Post,
                    "https://dashscope-intl.aliyuncs.com/api/v1/services/aigc/video-generation/video-synthesis",
                ) => json_response(json!({
                    "output": {
                        "task_status": "PENDING",
                        "task_id": "failed-task"
                    }
                })),
                (
                    ProviderApiRequestMethod::Get,
                    "https://dashscope-intl.aliyuncs.com/api/v1/tasks/failed-task",
                ) => json_response(json!({
                    "output": {
                        "task_id": "failed-task",
                        "task_status": "FAILED",
                        "message": "Content policy violation"
                    }
                })),
                _ => ProviderApiResponse::text(
                    400,
                    "Bad Request",
                    json!({ "message": "Invalid request" }).to_string(),
                ),
            };
            Box::pin(ready(Ok(response)))
        });
        let provider = create_alibaba(AlibabaProviderSettings::new().with_api_key("test-api-key"))
            .with_transport(transport)
            .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider
                .video("wan2.6-t2v")
                .do_generate(VideoModelCallOptions::new(1).with_prompt("bad")),
        );

        assert!(result.videos.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("alibaba"))
                .and_then(|metadata| metadata.get("taskId")),
            Some(&json!("failed-task"))
        );
        assert!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("alibaba"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(|message| message.as_str())
                .is_some_and(|message| message.contains("failed"))
        );
    }

    #[test]
    fn alibaba_provider_reports_unsupported_model_families_and_trait_video() {
        let provider = AlibabaProvider::new();
        let embedding_error = match provider.embedding_model("some-model") {
            Ok(_) => panic!("embedding models are unsupported"),
            Err(error) => error,
        };
        let image_error = match provider.image_model("some-model") {
            Ok(_) => panic!("image models are unsupported"),
            Err(error) => error,
        };

        assert_eq!(embedding_error.model_type(), ModelType::EmbeddingModel);
        assert_eq!(image_error.model_type(), ModelType::ImageModel);
        assert_eq!(provider.specification_version().as_str(), "v4");
        assert_eq!(alibaba("qwen-plus").provider(), "alibaba.chat");
        assert_eq!(provider.language_model("qwen-plus").model_id(), "qwen-plus");

        let trait_video = ProviderWithVideoModel::video_model(&provider, "wan2.6-t2v")
            .expect("ProviderWithVideoModel creates video model");
        assert_eq!(trait_video.provider(), "alibaba.video");
        assert_eq!(trait_video.model_id(), "wan2.6-t2v");
        assert_eq!(poll_ready(trait_video.max_videos_per_call()), Some(1));
    }

    #[test]
    fn alibaba_provider_settings_serde_accepts_upstream_shape() {
        let settings: AlibabaProviderSettings = serde_json::from_value(json!({
            "apiKey": "key",
            "baseURL": "https://example.com/chat/",
            "videoBaseURL": "https://example.com/video/",
            "headers": {
                "x-extra": "1"
            },
            "includeUsage": false
        }))
        .expect("settings deserialize");

        assert_eq!(settings.api_key.as_deref(), Some("key"));
        assert_eq!(
            settings.base_url.as_deref(),
            Some("https://example.com/chat/")
        );
        assert_eq!(
            settings.video_base_url.as_deref(),
            Some("https://example.com/video/")
        );
        assert_eq!(settings.headers.get("x-extra"), Some(&"1".to_string()));
        assert_eq!(settings.include_usage, Some(false));
        assert_eq!(
            DEFAULT_ALIBABA_BASE_URL,
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1"
        );
        assert_eq!(
            DEFAULT_ALIBABA_VIDEO_BASE_URL,
            "https://dashscope-intl.aliyuncs.com"
        );
    }
}
