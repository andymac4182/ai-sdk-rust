use std::collections::{BTreeMap, BTreeSet};
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use ai_sdk_rust::{
    FetchErrorInfo, GetFromApiOptions, HandledFetchError, Headers, JsonObject, JsonValue,
    LoadApiKeyError, LoadApiKeyOptions, ModelType, NoSuchModelError,
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel,
    PostJsonToApiOptions, Provider, ProviderApiRequest, ProviderApiRequestBody,
    ProviderApiRequestMethod, ProviderApiResponse, ProviderApiResponseHandlerError,
    ProviderMetadata, ProviderWithVideoModel, RuntimeEnvironment, VideoModel,
    VideoModelCallOptions, VideoModelFile, VideoModelResponse, VideoModelResult,
    VideoModelVideoData, Warning, combine_headers, convert_to_base64,
    create_json_error_response_handler, create_json_response_handler, delay, get_from_api,
    load_api_key, parse_provider_options, post_json_to_api, without_trailing_slash,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use url::Url;

/// Default base URL for upstream `@ai-sdk/bytedance` API calls.
pub const DEFAULT_BYTEDANCE_BASE_URL: &str = "https://ark.ap-southeast.bytepluses.com/api/v3";

/// Default polling interval used by upstream ByteDance video generation.
pub const DEFAULT_BYTEDANCE_POLL_INTERVAL_MILLIS: u64 = 3_000;

/// Default polling timeout used by upstream ByteDance video generation.
pub const DEFAULT_BYTEDANCE_POLL_TIMEOUT_MILLIS: u64 = 300_000;

/// Settings for the upstream ByteDance provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ByteDanceProviderSettings {
    /// ByteDance Ark API key. When omitted, `ARK_API_KEY` is read at request time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Base URL for API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl ByteDanceProviderSettings {
    /// Creates empty ByteDance provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the ByteDance Ark API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets the base URL used for API calls.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

/// Upstream ByteDance provider foundation.
#[derive(Clone)]
pub struct ByteDanceProvider {
    base_url: String,
    settings: ByteDanceProviderSettings,
    transport: ByteDanceTransport,
    current_date: ByteDanceDateProvider,
}

/// ByteDance video model.
#[derive(Clone)]
pub struct ByteDanceVideoModel {
    model_id: String,
    base_url: String,
    settings: ByteDanceProviderSettings,
    transport: ByteDanceTransport,
    current_date: ByteDanceDateProvider,
}

/// Future returned by an injected ByteDance HTTP transport.
pub type ByteDanceTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by ByteDance provider models.
pub type ByteDanceTransport =
    Arc<dyn Fn(ProviderApiRequest) -> ByteDanceTransportFuture + Send + Sync>;

type ByteDanceDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type ByteDanceVideoMaxVideosFuture<'a> = Ready<Option<usize>>;
type ByteDanceVideoGenerateFuture<'a> = Pin<Box<dyn Future<Output = VideoModelResult> + Send + 'a>>;

impl ByteDanceProvider {
    /// Creates a ByteDance provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(ByteDanceProviderSettings::new())
    }

    /// Creates a provider from explicit ByteDance settings.
    pub fn from_settings(settings: ByteDanceProviderSettings) -> Self {
        let base_url = without_trailing_slash(
            settings
                .base_url
                .as_deref()
                .or(Some(DEFAULT_BYTEDANCE_BASE_URL)),
        )
        .expect("default ByteDance base URL is present")
        .to_string();

        Self {
            base_url,
            settings,
            transport: default_bytedance_transport(),
            current_date: default_bytedance_date_provider(),
        }
    }

    /// Sets the ByteDance Ark API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: ByteDanceTransport) -> Self {
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

    /// Creates a video model.
    pub fn video(&self, model_id: impl Into<String>) -> ByteDanceVideoModel {
        self.video_model(model_id)
            .expect("ByteDance video models are supported")
    }

    /// Creates a video model.
    pub fn video_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<ByteDanceVideoModel, NoSuchModelError> {
        Ok(ByteDanceVideoModel::new(
            model_id,
            self.base_url.clone(),
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Reports that ByteDance does not expose language models through this provider.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::LanguageModel))
    }

    /// Reports that ByteDance does not expose embedding models through this provider.
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

    /// Reports that ByteDance does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }
}

impl Default for ByteDanceProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for ByteDanceProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        ByteDanceProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        ByteDanceProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        ByteDanceProvider::image_model(self, model_id)
    }
}

impl ProviderWithVideoModel for ByteDanceProvider {
    type VideoModel = ByteDanceVideoModel;

    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
        ByteDanceProvider::video_model(self, model_id)
    }
}

impl ByteDanceVideoModel {
    fn new(
        model_id: impl Into<String>,
        base_url: String,
        settings: ByteDanceProviderSettings,
        transport: ByteDanceTransport,
        current_date: ByteDanceDateProvider,
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
        "bytedance.video"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: ByteDanceTransport) -> Self {
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
            match bytedance_video_request_body(&self.model_id, &options) {
                Ok(args) => args,
                Err(message) => {
                    return bytedance_video_result_from_error(
                        &self.model_id,
                        message,
                        None,
                        timestamp,
                        Vec::new(),
                    );
                }
            };
        let request_headers = match self.request_headers(options.headers.as_ref()) {
            Ok(headers) => headers,
            Err(error) => {
                return bytedance_video_result_from_error(
                    &self.model_id,
                    error.to_string(),
                    None,
                    timestamp,
                    warnings,
                );
            }
        };
        let create_url = format!("{}/contents/generations/tasks", self.base_url);
        let create_options = PostJsonToApiOptions::new(create_url, request_body)
            .with_headers(request_headers.clone())
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);
        let create = match post_json_to_api(
            create_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    bytedance_task_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    bytedance_error_data,
                    bytedance_error_to_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                let message = bytedance_handled_error_message(error);
                return bytedance_video_result_from_error(
                    &self.model_id,
                    message,
                    None,
                    timestamp,
                    warnings,
                );
            }
        };
        let Some(task_id) = create.value.id.filter(|id| !id.is_empty()) else {
            return bytedance_video_result_from_error(
                &self.model_id,
                "No task ID returned from API".to_string(),
                None,
                timestamp,
                warnings,
            );
        };

        match self
            .wait_for_completion(&task_id, &request_headers, &provider_options)
            .await
        {
            Ok((status, headers)) => bytedance_video_result_from_response(
                &self.model_id,
                &task_id,
                status,
                headers,
                timestamp,
                warnings,
            ),
            Err(message) => bytedance_video_result_from_error(
                &self.model_id,
                message,
                Some(task_id),
                timestamp,
                warnings,
            ),
        }
    }

    async fn wait_for_completion(
        &self,
        task_id: &str,
        headers: &BTreeMap<String, Option<String>>,
        provider_options: &ByteDanceVideoProviderOptions,
    ) -> Result<(ByteDanceStatusResponse, Option<Headers>), String> {
        let poll_interval = provider_options
            .poll_interval_millis
            .unwrap_or(DEFAULT_BYTEDANCE_POLL_INTERVAL_MILLIS);
        let poll_timeout = provider_options
            .poll_timeout_millis
            .unwrap_or(DEFAULT_BYTEDANCE_POLL_TIMEOUT_MILLIS);
        let started = Instant::now();
        let status_url = format!("{}/contents/generations/tasks/{task_id}", self.base_url);

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
                        bytedance_status_response,
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| {
                    Ok(create_json_error_response_handler(
                        response.json_error_response_handler_options(request),
                        bytedance_error_data,
                        bytedance_error_to_message,
                        |_, _| None,
                    ))
                },
            )
            .await
            .map_err(bytedance_handled_error_message)?;

            match response.value.status.as_str() {
                "succeeded" => return Ok((response.value, response.response_headers)),
                "failed" => {
                    let status = serde_json::to_string(&response.value)
                        .unwrap_or_else(|_| response.value.status.clone());
                    return Err(format!("Video generation failed: {status}"));
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

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(bytedance_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
        ]))
    }
}

impl VideoModel for ByteDanceVideoModel {
    type MaxVideosPerCallFuture<'a>
        = ByteDanceVideoMaxVideosFuture<'a>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = ByteDanceVideoGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        ByteDanceVideoModel::provider(self)
    }

    fn model_id(&self) -> &str {
        ByteDanceVideoModel::model_id(self)
    }

    fn max_videos_per_call(&self) -> Self::MaxVideosPerCallFuture<'_> {
        ready(Some(1))
    }

    fn do_generate(&self, options: VideoModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates a ByteDance provider with explicit settings.
pub fn create_byte_dance(settings: ByteDanceProviderSettings) -> ByteDanceProvider {
    ByteDanceProvider::from_settings(settings)
}

/// Creates a ByteDance video model using the default provider settings.
pub fn byte_dance(model_id: impl Into<String>) -> ByteDanceVideoModel {
    ByteDanceProvider::new().video(model_id)
}

/// Provider-specific video options accepted by upstream ByteDance.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ByteDanceVideoProviderOptions {
    /// Whether to include a watermark in generated videos.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watermark: Option<bool>,

    /// Whether ByteDance should generate audio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generate_audio: Option<bool>,

    /// Whether the camera should remain fixed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_fixed: Option<bool>,

    /// Whether to return the last frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub return_last_frame: Option<bool>,

    /// Service tier, usually `default` or `flex`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<String>,

    /// Whether to request draft generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<bool>,

    /// URL or data URI for a last-frame image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_frame_image: Option<String>,

    /// Reference image URLs or data URIs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_images: Option<Vec<String>>,

    /// Reference video URLs or data URIs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_videos: Option<Vec<String>>,

    /// Reference audio URLs or data URIs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_audio: Option<Vec<String>>,

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

    /// Additional provider-specific options passed through unchanged.
    #[serde(flatten)]
    pub extra: BTreeMap<String, JsonValue>,
}

impl ByteDanceVideoProviderOptions {
    fn validate(&self) -> Result<(), &'static str> {
        if self
            .service_tier
            .as_deref()
            .is_some_and(|tier| tier != "default" && tier != "flex")
        {
            return Err("serviceTier must be 'default' or 'flex'");
        }

        if self.poll_interval_millis == Some(0) {
            return Err("pollIntervalMs must be positive");
        }

        if self.poll_timeout_millis == Some(0) {
            return Err("pollTimeoutMs must be positive");
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ByteDanceTaskResponse {
    #[serde(default)]
    id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ByteDanceStatusResponse {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    status: String,
    #[serde(default)]
    content: Option<ByteDanceStatusContent>,
    #[serde(default)]
    usage: Option<ByteDanceUsage>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ByteDanceStatusContent {
    #[serde(default)]
    video_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ByteDanceUsage {
    #[serde(default)]
    completion_tokens: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ByteDanceErrorData {
    #[serde(default)]
    error: Option<ByteDanceErrorBody>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ByteDanceErrorBody {
    message: String,
    #[serde(default)]
    code: Option<String>,
}

fn bytedance_video_request_body(
    model_id: &str,
    options: &VideoModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>, ByteDanceVideoProviderOptions), String> {
    let provider_options = parse_provider_options(
        "bytedance",
        Some(&options.provider_options),
        bytedance_video_provider_options,
    )
    .map_err(|error| error.to_string())?
    .unwrap_or_default();
    let warnings = bytedance_video_warnings(options);
    let mut body = JsonObject::new();
    let mut content = Vec::new();

    if let Some(prompt) = options.prompt.as_ref() {
        if !prompt.is_empty() {
            content.push(bytedance_content_part(
                "text",
                "text",
                JsonValue::String(prompt.clone()),
                None,
            ));
        }
    }

    if let Some(image) = options.image.as_ref() {
        content.push(bytedance_url_content_part(
            "image_url",
            "image_url",
            bytedance_video_file_url(image),
            None,
        ));
    }

    if let Some(last_frame_image) = provider_options.last_frame_image.as_ref() {
        content.push(bytedance_url_content_part(
            "image_url",
            "image_url",
            last_frame_image.clone(),
            Some("last_frame"),
        ));
    }

    bytedance_extend_reference_content(
        &mut content,
        provider_options.reference_images.as_ref(),
        "image_url",
        "image_url",
        "reference_image",
    );
    bytedance_extend_reference_content(
        &mut content,
        provider_options.reference_videos.as_ref(),
        "video_url",
        "video_url",
        "reference_video",
    );
    bytedance_extend_reference_content(
        &mut content,
        provider_options.reference_audio.as_ref(),
        "audio_url",
        "audio_url",
        "reference_audio",
    );

    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    body.insert("content".to_string(), JsonValue::Array(content));
    insert_nonempty_string(&mut body, "ratio", options.aspect_ratio.as_ref());
    insert_positive_f64(&mut body, "duration", options.duration);
    insert_positive_u64(&mut body, "seed", options.seed);
    insert_resolution(&mut body, options.resolution.as_ref());
    insert_option_bool(&mut body, "watermark", provider_options.watermark);
    insert_option_bool(&mut body, "generate_audio", provider_options.generate_audio);
    insert_option_bool(&mut body, "camera_fixed", provider_options.camera_fixed);
    insert_option_bool(
        &mut body,
        "return_last_frame",
        provider_options.return_last_frame,
    );
    insert_option_string(
        &mut body,
        "service_tier",
        provider_options.service_tier.clone(),
    );
    insert_option_bool(&mut body, "draft", provider_options.draft);

    let handled = bytedance_handled_provider_option_keys();
    for (name, value) in &provider_options.extra {
        if !handled.contains(name.as_str()) {
            body.insert(name.clone(), value.clone());
        }
    }

    Ok((JsonValue::Object(body), warnings, provider_options))
}

fn bytedance_video_provider_options(
    value: &JsonValue,
) -> Result<ByteDanceVideoProviderOptions, String> {
    let options = serde_json::from_value::<ByteDanceVideoProviderOptions>(value.clone())
        .map_err(|error| error.to_string())?;
    options.validate().map_err(str::to_string)?;
    Ok(options)
}

fn bytedance_video_warnings(options: &VideoModelCallOptions) -> Vec<Warning> {
    let mut warnings = Vec::new();

    if options.fps.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "fps".to_string(),
            details: Some(
                "ByteDance video models do not support custom FPS. Frame rate is fixed at 24 fps."
                    .to_string(),
            ),
        });
    }

    if options.n > 1 {
        warnings.push(Warning::Unsupported {
            feature: "n".to_string(),
            details: Some(
                "ByteDance video models do not support generating multiple videos per call. Only 1 video will be generated."
                    .to_string(),
            ),
        });
    }

    warnings
}

fn bytedance_content_part(
    part_type: &str,
    value_key: &str,
    value: JsonValue,
    role: Option<&str>,
) -> JsonValue {
    let mut part = JsonObject::new();
    part.insert("type".to_string(), JsonValue::String(part_type.to_string()));
    part.insert(value_key.to_string(), value);

    if let Some(role) = role {
        part.insert("role".to_string(), JsonValue::String(role.to_string()));
    }

    JsonValue::Object(part)
}

fn bytedance_url_content_part(
    part_type: &str,
    value_key: &str,
    url: String,
    role: Option<&str>,
) -> JsonValue {
    let mut value = JsonObject::new();
    value.insert("url".to_string(), JsonValue::String(url));
    bytedance_content_part(part_type, value_key, JsonValue::Object(value), role)
}

fn bytedance_extend_reference_content(
    content: &mut Vec<JsonValue>,
    values: Option<&Vec<String>>,
    part_type: &str,
    value_key: &str,
    role: &str,
) {
    if let Some(values) = values {
        for value in values {
            content.push(bytedance_url_content_part(
                part_type,
                value_key,
                value.clone(),
                Some(role),
            ));
        }
    }
}

fn bytedance_video_file_url(file: &VideoModelFile) -> String {
    match file {
        VideoModelFile::Url { url, .. } => url.as_str().to_string(),
        VideoModelFile::File {
            media_type, data, ..
        } => format!("data:{media_type};base64,{}", convert_to_base64(data)),
    }
}

fn bytedance_handled_provider_option_keys() -> BTreeSet<&'static str> {
    [
        "watermark",
        "generateAudio",
        "cameraFixed",
        "returnLastFrame",
        "serviceTier",
        "draft",
        "lastFrameImage",
        "referenceImages",
        "referenceVideos",
        "referenceAudio",
        "pollIntervalMs",
        "pollTimeoutMs",
    ]
    .into_iter()
    .collect()
}

fn bytedance_provider_header_entries(
    settings: &ByteDanceProviderSettings,
) -> Result<Vec<(String, Option<String>)>, LoadApiKeyError> {
    let mut headers = vec![
        (
            "Authorization".to_string(),
            Some(format!(
                "Bearer {}",
                bytedance_api_key(settings.api_key.as_ref())?
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

    Ok(headers)
}

fn optional_headers(headers: Option<&Headers>) -> Option<Vec<(String, Option<String>)>> {
    headers.map(|headers| {
        headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone())))
            .collect()
    })
}

fn bytedance_api_key(explicit_api_key: Option<&String>) -> Result<String, LoadApiKeyError> {
    let mut options = LoadApiKeyOptions::new("ARK_API_KEY", "ByteDance ModelArk");

    if let Some(api_key) = explicit_api_key {
        options = options.with_api_key(api_key.clone());
    }

    load_api_key(options)
}

fn bytedance_task_response(value: &JsonValue) -> Result<ByteDanceTaskResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn bytedance_status_response(
    value: &JsonValue,
) -> Result<ByteDanceStatusResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn bytedance_error_data(value: &JsonValue) -> Result<ByteDanceErrorData, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn bytedance_error_to_message(data: &ByteDanceErrorData) -> String {
    data.error
        .as_ref()
        .map(|error| error.message.clone())
        .or_else(|| data.message.clone())
        .unwrap_or_else(|| "Unknown error".to_string())
}

fn bytedance_handled_error_message(error: HandledFetchError) -> String {
    match error {
        HandledFetchError::Original { error } => error.message().to_string(),
        HandledFetchError::ApiCall { error } => error.message().to_string(),
    }
}

fn bytedance_video_result_from_response(
    model_id: &str,
    task_id: &str,
    response: ByteDanceStatusResponse,
    headers: Option<Headers>,
    timestamp: OffsetDateTime,
    warnings: Vec<Warning>,
) -> VideoModelResult {
    let Some(content) = response.content else {
        return bytedance_video_result_from_error(
            model_id,
            "No video URL in response".to_string(),
            Some(task_id.to_string()),
            timestamp,
            warnings,
        );
    };
    let Some(video_url) = content.video_url else {
        return bytedance_video_result_from_error(
            model_id,
            "No video URL in response".to_string(),
            Some(task_id.to_string()),
            timestamp,
            warnings,
        );
    };
    let Ok(video_url) = Url::parse(&video_url) else {
        return bytedance_video_result_from_error(
            model_id,
            "No video URL in response".to_string(),
            Some(task_id.to_string()),
            timestamp,
            warnings,
        );
    };
    let mut result = VideoModelResult::new(
        vec![VideoModelVideoData::url(video_url, "video/mp4")],
        bytedance_video_response(model_id, headers, timestamp),
    )
    .with_provider_metadata(bytedance_success_metadata(task_id, response.usage));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn bytedance_video_result_from_error(
    model_id: &str,
    message: String,
    task_id: Option<String>,
    timestamp: OffsetDateTime,
    warnings: Vec<Warning>,
) -> VideoModelResult {
    let mut result = VideoModelResult::new(
        Vec::new(),
        bytedance_video_response(model_id, None, timestamp),
    )
    .with_provider_metadata(bytedance_error_metadata(message, task_id));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn bytedance_video_response(
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

fn bytedance_success_metadata(task_id: &str, usage: Option<ByteDanceUsage>) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("taskId".to_string(), JsonValue::String(task_id.to_string()));

    if let Some(usage) = usage {
        provider.insert(
            "usage".to_string(),
            serde_json::to_value(usage).expect("ByteDance usage serializes"),
        );
    }

    metadata.insert("bytedance".to_string(), provider);
    metadata
}

fn bytedance_error_metadata(message: String, task_id: Option<String>) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("errorMessage".to_string(), JsonValue::String(message));

    if let Some(task_id) = task_id {
        provider.insert("taskId".to_string(), JsonValue::String(task_id));
    }

    metadata.insert("bytedance".to_string(), provider);
    metadata
}

fn insert_nonempty_string(body: &mut JsonObject, name: &str, value: Option<&String>) {
    if let Some(value) = value {
        if !value.is_empty() {
            body.insert(name.to_string(), JsonValue::String(value.clone()));
        }
    }
}

fn insert_option_bool(body: &mut JsonObject, name: &str, value: Option<bool>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::Bool(value));
    }
}

fn insert_option_string(body: &mut JsonObject, name: &str, value: Option<String>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::String(value));
    }
}

fn insert_positive_f64(body: &mut JsonObject, name: &str, value: Option<f64>) {
    if let Some(value) = value {
        if value > 0.0 {
            body.insert(name.to_string(), JsonValue::from(value));
        }
    }
}

fn insert_positive_u64(body: &mut JsonObject, name: &str, value: Option<u64>) {
    if let Some(value) = value {
        if value > 0 {
            body.insert(name.to_string(), JsonValue::from(value));
        }
    }
}

fn insert_resolution(body: &mut JsonObject, value: Option<&String>) {
    if let Some(value) = value {
        if value.is_empty() {
            return;
        }

        body.insert(
            "resolution".to_string(),
            JsonValue::String(bytedance_resolution(value).to_string()),
        );
    }
}

fn bytedance_resolution(value: &str) -> &str {
    match value {
        "864x496" | "496x864" | "752x560" | "560x752" | "640x640" | "992x432" | "432x992"
        | "864x480" | "480x864" | "736x544" | "544x736" | "960x416" | "416x960" | "832x480"
        | "480x832" | "624x624" => "480p",
        "1280x720" | "720x1280" | "1112x834" | "834x1112" | "960x960" | "1470x630" | "630x1470"
        | "1248x704" | "704x1248" | "1120x832" | "832x1120" | "1504x640" | "640x1504" => "720p",
        "1920x1080" | "1080x1920" | "1664x1248" | "1248x1664" | "1440x1440" | "2206x946"
        | "946x2206" | "1920x1088" | "1088x1920" | "2176x928" | "928x2176" => "1080p",
        _ => value,
    }
}

fn default_bytedance_date_provider() -> ByteDanceDateProvider {
    Arc::new(OffsetDateTime::now_utc)
}

fn default_bytedance_transport() -> ByteDanceTransport {
    Arc::new(|request| Box::pin(ready(execute_bytedance_request(request))))
}

fn execute_bytedance_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_bytedance_get_request(request),
        ProviderApiRequestMethod::Post => execute_bytedance_post_request(request),
    }
}

fn execute_bytedance_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    bytedance_provider_api_response(response)
}

fn execute_bytedance_post_request(
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
                "multipart form data is not supported by the ByteDance transport",
            ));
        }
        None => builder.send_empty(),
    };

    bytedance_provider_api_response(response)
}

fn bytedance_provider_api_response(
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
        ByteDanceProvider, ByteDanceProviderSettings, ByteDanceTransport, ByteDanceTransportFuture,
        DEFAULT_BYTEDANCE_BASE_URL, byte_dance, create_byte_dance,
    };
    use ai_sdk_rust::{
        FileDataContent, ModelType, Provider, ProviderApiRequest, ProviderApiRequestBody,
        ProviderApiRequestMethod, ProviderApiResponse, ProviderOptions, ProviderWithVideoModel,
        VideoModel, VideoModelCallOptions, VideoModelFile, VideoModelVideoData,
    };
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
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

    fn fixed_timestamp() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(0).expect("unix epoch is valid")
    }

    fn json_response(value: serde_json::Value) -> ProviderApiResponse {
        ProviderApiResponse::text(200, "OK", value.to_string())
    }

    fn bytedance_success_transport() -> (Arc<Mutex<Vec<ProviderApiRequest>>>, ByteDanceTransport) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: ByteDanceTransport = Arc::new(move |request| -> ByteDanceTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());

            let response = match (request.method, request.url.as_str()) {
                (
                    ProviderApiRequestMethod::Post,
                    "https://ark.ap-southeast.bytepluses.com/api/v3/contents/generations/tasks",
                ) => json_response(json!({
                    "id": "task-123"
                })),
                (
                    ProviderApiRequestMethod::Get,
                    "https://ark.ap-southeast.bytepluses.com/api/v3/contents/generations/tasks/task-123",
                ) => json_response(json!({
                    "id": "task-123",
                    "model": "seedance-1-0-pro-250528",
                    "status": "succeeded",
                    "content": {
                        "video_url": "https://bytedance.cdn/files/video-output.mp4"
                    },
                    "usage": {
                        "completion_tokens": 100
                    }
                }))
                .with_headers(
                    [("x-request-id".to_string(), "req-123".to_string())]
                        .into_iter()
                        .collect(),
                ),
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"error": {"message": "unexpected request", "code": "404"}}).to_string(),
                ),
            };

            Box::pin(ready(Ok(response)))
        });

        (requests, transport)
    }

    fn request_body_json(request: &ProviderApiRequest) -> serde_json::Value {
        let Some(ProviderApiRequestBody::Text { content }) = request.body.as_ref() else {
            panic!("expected text request body");
        };

        serde_json::from_str(content).expect("request body is valid JSON")
    }

    #[test]
    fn bytedance_video_model_generates_video_with_headers_body_and_metadata() {
        let (requests, transport) = bytedance_success_transport();
        let provider = create_byte_dance(
            ByteDanceProviderSettings::new()
                .with_api_key("test-api-key")
                .with_header("Custom-Provider-Header", "provider"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "bytedance".to_string(),
            serde_json::from_value(json!({
                "watermark": true,
                "generateAudio": true,
                "cameraFixed": true,
                "returnLastFrame": true,
                "serviceTier": "flex",
                "draft": true,
                "lastFrameImage": "https://example.com/last-frame.png",
                "referenceImages": ["https://example.com/ref.png"],
                "referenceVideos": ["https://example.com/ref.mp4"],
                "referenceAudio": ["data:audio/mp3;base64,SGVsbG8="],
                "custom_param": "custom_value"
            }))
            .expect("provider options deserialize"),
        );

        let result = poll_ready(
            provider.video("seedance-1-0-pro-250528").do_generate(
                VideoModelCallOptions::new(3)
                    .with_prompt("A futuristic city")
                    .with_aspect_ratio("16:9")
                    .with_resolution("1920x1080")
                    .with_duration(5.0)
                    .with_fps(30.0)
                    .with_seed(42)
                    .with_image(VideoModelFile::file(
                        "image/png",
                        FileDataContent::Bytes(vec![137, 80, 78, 71]),
                    ))
                    .with_header("Custom-Request-Header", "request")
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.videos.len(), 1);
        assert_eq!(
            result.videos[0],
            VideoModelVideoData::url(
                Url::parse("https://bytedance.cdn/files/video-output.mp4").expect("valid URL"),
                "video/mp4"
            )
        );
        assert_eq!(result.warnings.len(), 2);
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| matches!(warning, ai_sdk_rust::Warning::Unsupported { feature, .. } if feature == "fps"))
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| matches!(warning, ai_sdk_rust::Warning::Unsupported { feature, .. } if feature == "n"))
        );
        assert_eq!(result.response.timestamp, fixed_timestamp());
        assert_eq!(result.response.model_id, "seedance-1-0-pro-250528");
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id")),
            Some(&"req-123".to_string())
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("bytedance"))
                .and_then(|provider| provider.get("taskId")),
            Some(&json!("task-123"))
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("bytedance"))
                .and_then(|provider| provider.get("usage")),
            Some(&json!({"completion_tokens": 100}))
        );

        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, ProviderApiRequestMethod::Post);
        assert_eq!(
            requests[0].headers.get("authorization"),
            Some(&"Bearer test-api-key".to_string())
        );
        assert_eq!(
            requests[0].headers.get("content-type"),
            Some(&"application/json".to_string())
        );
        assert_eq!(
            requests[0].headers.get("custom-provider-header"),
            Some(&"provider".to_string())
        );
        assert_eq!(
            requests[0].headers.get("custom-request-header"),
            Some(&"request".to_string())
        );
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "model": "seedance-1-0-pro-250528",
                "content": [
                    {
                        "type": "text",
                        "text": "A futuristic city"
                    },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": "data:image/png;base64,iVBORw=="
                        }
                    },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": "https://example.com/last-frame.png"
                        },
                        "role": "last_frame"
                    },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": "https://example.com/ref.png"
                        },
                        "role": "reference_image"
                    },
                    {
                        "type": "video_url",
                        "video_url": {
                            "url": "https://example.com/ref.mp4"
                        },
                        "role": "reference_video"
                    },
                    {
                        "type": "audio_url",
                        "audio_url": {
                            "url": "data:audio/mp3;base64,SGVsbG8="
                        },
                        "role": "reference_audio"
                    }
                ],
                "ratio": "16:9",
                "duration": 5.0,
                "seed": 42,
                "resolution": "1080p",
                "watermark": true,
                "generate_audio": true,
                "camera_fixed": true,
                "return_last_frame": true,
                "service_tier": "flex",
                "draft": true,
                "custom_param": "custom_value"
            })
        );
    }

    #[test]
    fn bytedance_video_model_passes_unmapped_resolution_and_url_image() {
        let (requests, transport) = bytedance_success_transport();
        let provider = create_byte_dance(
            ByteDanceProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://ark.ap-southeast.bytepluses.com/api/v3/"),
        )
        .with_transport(transport);

        let result = poll_ready(
            provider.video("custom-model").do_generate(
                VideoModelCallOptions::new(1)
                    .with_prompt("A city")
                    .with_resolution("640x480")
                    .with_image(VideoModelFile::url(
                        Url::parse("https://example.com/input.png").expect("valid URL"),
                    )),
            ),
        );

        assert_eq!(result.videos.len(), 1);
        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "model": "custom-model",
                "content": [
                    {
                        "type": "text",
                        "text": "A city"
                    },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": "https://example.com/input.png"
                        }
                    }
                ],
                "resolution": "640x480"
            })
        );
    }

    #[test]
    fn bytedance_video_model_maps_api_and_status_errors_to_metadata() {
        let transport: ByteDanceTransport = Arc::new(move |request| -> ByteDanceTransportFuture {
            let response = match (request.method, request.url.as_str()) {
                (
                    ProviderApiRequestMethod::Post,
                    "https://ark.ap-southeast.bytepluses.com/api/v3/contents/generations/tasks",
                ) => json_response(json!({
                    "id": "failed-task"
                })),
                (
                    ProviderApiRequestMethod::Get,
                    "https://ark.ap-southeast.bytepluses.com/api/v3/contents/generations/tasks/failed-task",
                ) => json_response(json!({
                    "id": "failed-task",
                    "status": "failed"
                })),
                _ => ProviderApiResponse::text(
                    400,
                    "Bad Request",
                    json!({
                        "error": {
                            "message": "Invalid prompt",
                            "code": "400"
                        }
                    })
                    .to_string(),
                ),
            };

            Box::pin(ready(Ok(response)))
        });
        let provider =
            create_byte_dance(ByteDanceProviderSettings::new().with_api_key("test-api-key"))
                .with_transport(transport)
                .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider
                .video("seedance-1-0-pro-250528")
                .do_generate(VideoModelCallOptions::new(1).with_prompt("bad")),
        );

        assert!(result.videos.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("bytedance"))
                .and_then(|provider| provider.get("taskId")),
            Some(&json!("failed-task"))
        );
        assert!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("bytedance"))
                .and_then(|provider| provider.get("errorMessage"))
                .and_then(|message| message.as_str())
                .is_some_and(|message| message.contains("Video generation failed"))
        );
    }

    #[test]
    fn bytedance_provider_reports_unsupported_model_families_and_trait_video() {
        let provider = ByteDanceProvider::new();
        let language_error = match provider.language_model("some-model") {
            Ok(_) => panic!("language models are unsupported"),
            Err(error) => error,
        };
        let embedding_error = match provider.embedding_model("some-model") {
            Ok(_) => panic!("embedding models are unsupported"),
            Err(error) => error,
        };
        let image_error = match provider.image_model("some-model") {
            Ok(_) => panic!("image models are unsupported"),
            Err(error) => error,
        };

        assert_eq!(language_error.model_type(), ModelType::LanguageModel);
        assert_eq!(embedding_error.model_type(), ModelType::EmbeddingModel);
        assert_eq!(image_error.model_type(), ModelType::ImageModel);
        assert_eq!(provider.specification_version().as_str(), "v4");
        assert_eq!(
            byte_dance("seedance-1-0-pro-250528").provider(),
            "bytedance.video"
        );

        let trait_video = ProviderWithVideoModel::video_model(&provider, "seedance-1-0-pro-250528")
            .expect("ProviderWithVideoModel creates video model");
        assert_eq!(trait_video.model_id(), "seedance-1-0-pro-250528");
        assert_eq!(poll_ready(trait_video.max_videos_per_call()), Some(1));
    }

    #[test]
    fn bytedance_provider_settings_serde_accepts_upstream_shape() {
        let settings: ByteDanceProviderSettings = serde_json::from_value(json!({
            "apiKey": "key",
            "baseURL": "https://example.com/base/",
            "headers": {
                "x-extra": "1"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(settings.api_key.as_deref(), Some("key"));
        assert_eq!(
            settings.base_url.as_deref(),
            Some("https://example.com/base/")
        );
        assert_eq!(settings.headers.get("x-extra"), Some(&"1".to_string()));
        assert_eq!(
            DEFAULT_BYTEDANCE_BASE_URL,
            "https://ark.ap-southeast.bytepluses.com/api/v3"
        );
    }
}
