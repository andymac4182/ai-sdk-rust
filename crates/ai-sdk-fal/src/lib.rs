use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use ai_sdk_rust::{
    FetchErrorInfo, GetFromApiOptions, HandledFetchError, Headers, ImageModel,
    ImageModelCallOptions, ImageModelFile, ImageModelProviderMetadata,
    ImageModelProviderMetadataEntry, ImageModelResponse, ImageModelResult, JsonObject, JsonValue,
    LoadApiKeyError, ModelType, NoSuchModelError, OpenAICompatibleChatLanguageModel,
    OpenAICompatibleEmbeddingModel, PostJsonToApiOptions, Provider, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, ProviderMetadata, ProviderWithVideoModel, RuntimeEnvironment,
    VideoModel, VideoModelCallOptions, VideoModelResponse, VideoModelResult, VideoModelVideoData,
    Warning, combine_headers, convert_image_model_file_to_data_uri, create_binary_response_handler,
    create_json_error_response_handler, create_json_response_handler, delay, get_from_api,
    parse_provider_options, post_json_to_api, with_user_agent_suffix, without_trailing_slash,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use url::Url;

/// Default base URL for upstream `@ai-sdk/fal` image API calls.
pub const DEFAULT_FAL_BASE_URL: &str = "https://fal.run";

/// Default fal queue base URL for video generation.
pub const DEFAULT_FAL_QUEUE_BASE_URL: &str = "https://queue.fal.run/fal-ai";

const DEFAULT_FAL_POLL_INTERVAL_MILLIS: u64 = 2_000;
const DEFAULT_FAL_POLL_TIMEOUT_MILLIS: u64 = 300_000;

/// Settings for the upstream fal.ai provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FalProviderSettings {
    /// fal.ai API key. When omitted, `FAL_API_KEY` then `FAL_KEY` are read at request time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Base URL for image API calls.
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

impl FalProviderSettings {
    /// Creates empty fal provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the fal API key.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets the base URL used for image API calls.
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

/// Upstream fal.ai provider foundation.
#[derive(Clone)]
pub struct FalProvider {
    base_url: String,
    settings: FalProviderSettings,
    transport: FalTransport,
    current_date: FalDateProvider,
}

/// fal image model.
#[derive(Clone)]
pub struct FalImageModel {
    model_id: String,
    base_url: String,
    settings: FalProviderSettings,
    transport: FalTransport,
    current_date: FalDateProvider,
}

/// fal video model.
#[derive(Clone)]
pub struct FalVideoModel {
    model_id: String,
    settings: FalProviderSettings,
    transport: FalTransport,
    current_date: FalDateProvider,
}

/// Future returned by an injected fal HTTP transport.
pub type FalTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by fal provider models.
pub type FalTransport = Arc<dyn Fn(ProviderApiRequest) -> FalTransportFuture + Send + Sync>;

type FalDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type FalImageGenerateFuture<'a> = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>;
type FalVideoMaxVideosFuture<'a> = Ready<Option<usize>>;
type FalVideoGenerateFuture<'a> = Pin<Box<dyn Future<Output = VideoModelResult> + Send + 'a>>;

impl FalProvider {
    /// Creates a fal provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(FalProviderSettings::new())
    }

    /// Creates a provider from explicit fal settings.
    pub fn from_settings(settings: FalProviderSettings) -> Self {
        let base_url =
            without_trailing_slash(settings.base_url.as_deref().or(Some(DEFAULT_FAL_BASE_URL)))
                .expect("default fal base URL is present")
                .to_string();

        Self {
            base_url,
            settings,
            transport: default_fal_transport(),
            current_date: Arc::new(OffsetDateTime::now_utc),
        }
    }

    /// Sets the fal API key for this provider.
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
    pub fn with_transport(mut self, transport: FalTransport) -> Self {
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

    /// Creates an image model.
    pub fn image(&self, model_id: impl Into<String>) -> FalImageModel {
        self.image_model(model_id)
            .expect("fal image models are supported")
    }

    /// Creates an image model.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<FalImageModel, NoSuchModelError> {
        Ok(FalImageModel::new(
            model_id,
            self.base_url.clone(),
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Creates a video model.
    pub fn video(&self, model_id: impl Into<String>) -> FalVideoModel {
        self.video_model(model_id)
            .expect("fal video models are supported")
    }

    /// Creates a video model.
    pub fn video_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<FalVideoModel, NoSuchModelError> {
        Ok(FalVideoModel::new(
            model_id,
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Reports that fal does not expose language models through this provider.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::LanguageModel))
    }

    /// Reports that fal does not expose embedding models through this provider.
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
}

impl Default for FalProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for FalProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = FalImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        FalProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        FalProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        FalProvider::image_model(self, model_id)
    }
}

impl ProviderWithVideoModel for FalProvider {
    type VideoModel = FalVideoModel;

    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
        FalProvider::video_model(self, model_id)
    }
}

impl FalImageModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: FalProviderSettings,
        transport: FalTransport,
        current_date: FalDateProvider,
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
        "fal.image"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: FalTransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_generate_result(&self, options: ImageModelCallOptions) -> ImageModelResult {
        let timestamp = (self.current_date)();
        let (request_body, warnings) = match fal_image_request_body(&options) {
            Ok(args) => args,
            Err(error) => {
                return fal_image_result_from_error(
                    &self.model_id,
                    error,
                    None,
                    Vec::new(),
                    timestamp,
                );
            }
        };
        let request_headers = match self.request_headers(options.headers.as_ref()) {
            Ok(headers) => headers,
            Err(error) => {
                return fal_image_result_from_error(
                    &self.model_id,
                    error.to_string(),
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let transport = Arc::clone(&self.transport);
        let response = match post_json_to_api(
            PostJsonToApiOptions::new(self.image_model_url(), request_body)
                .with_headers(request_headers)
                .with_environment(RuntimeEnvironment::unknown()),
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    fal_image_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    fal_error_response,
                    fal_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                let (message, headers) = fal_handled_error_parts(error);
                return fal_image_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    warnings,
                    timestamp,
                );
            }
        };

        let FalImageResponse {
            images: target_images,
            prompt: _,
            has_nsfw_concepts,
            nsfw_content_detected,
            mut extra,
        } = response.value;
        let mut images = Vec::with_capacity(target_images.len());

        for image in &target_images {
            let transport = Arc::clone(&self.transport);
            match get_from_api(
                GetFromApiOptions::new(image.url.clone())
                    .with_environment(RuntimeEnvironment::unknown()),
                move |request| (transport)(request),
                |request, response| {
                    create_binary_response_handler(
                        response.binary_response_handler_options(request),
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| {
                    Ok(create_json_error_response_handler(
                        response.json_error_response_handler_options(request),
                        fal_error_response,
                        fal_error_message,
                        |_, _| None,
                    ))
                },
            )
            .await
            {
                Ok(image) => images.push(ai_sdk_rust::FileDataContent::Bytes(image.value)),
                Err(error) => {
                    let (message, headers) = fal_handled_error_parts(error);
                    return fal_image_result_from_error(
                        &self.model_id,
                        message,
                        headers,
                        warnings,
                        timestamp,
                    );
                }
            }
        }

        let mut provider_images = Vec::with_capacity(target_images.len());
        for (index, image) in target_images.into_iter().enumerate() {
            provider_images.push(fal_image_metadata(
                image,
                has_nsfw_concepts
                    .as_ref()
                    .and_then(|values| values.get(index).copied())
                    .or_else(|| {
                        nsfw_content_detected
                            .as_ref()
                            .and_then(|values| values.get(index).copied())
                    }),
            ));
        }
        extra.remove("images");
        extra.remove("image");
        extra.remove("prompt");
        extra.remove("has_nsfw_concepts");
        extra.remove("nsfw_content_detected");

        let mut result = ImageModelResult::new(
            images,
            fal_image_response_metadata(&self.model_id, response.response_headers, timestamp),
        )
        .with_provider_metadata(ImageModelProviderMetadata::from([(
            "fal".to_string(),
            ImageModelProviderMetadataEntry {
                images: provider_images,
                extra,
            },
        )]));

        for warning in warnings {
            result = result.with_warning(warning);
        }

        result
    }

    fn image_model_url(&self) -> String {
        format!("{}/{}", self.base_url, self.model_id)
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(fal_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
        ]))
    }
}

impl ImageModel for FalImageModel {
    type MaxImagesPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = FalImageGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        FalImageModel::provider(self)
    }

    fn model_id(&self) -> &str {
        FalImageModel::model_id(self)
    }

    fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
        ready(Some(1))
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

impl FalVideoModel {
    fn new(
        model_id: impl Into<String>,
        settings: FalProviderSettings,
        transport: FalTransport,
        current_date: FalDateProvider,
    ) -> Self {
        Self {
            model_id: model_id.into(),
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
        "fal.video"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: FalTransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_generate_result(&self, options: VideoModelCallOptions) -> VideoModelResult {
        let timestamp = (self.current_date)();
        let (request_body, warnings, poll_overrides) = match fal_video_request_body(&options) {
            Ok(args) => args,
            Err(error) => {
                return fal_video_result_from_error(
                    &self.model_id,
                    error,
                    None,
                    Vec::new(),
                    timestamp,
                );
            }
        };
        let request_headers = match self.request_headers(options.headers.as_ref()) {
            Ok(headers) => headers,
            Err(error) => {
                return fal_video_result_from_error(
                    &self.model_id,
                    error.to_string(),
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let transport = Arc::clone(&self.transport);
        let queue_response = match post_json_to_api(
            PostJsonToApiOptions::new(self.queue_url(), request_body)
                .with_headers(request_headers.clone())
                .with_environment(RuntimeEnvironment::unknown()),
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    fal_job_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    fal_error_response,
                    fal_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                let (message, headers) = fal_handled_error_parts(error);
                return fal_video_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    warnings,
                    timestamp,
                );
            }
        };
        let Some(response_url) = queue_response.value.response_url else {
            return fal_video_result_from_error(
                &self.model_id,
                "No response URL returned from queue endpoint".to_string(),
                queue_response.response_headers,
                warnings,
                timestamp,
            );
        };

        let final_response = match self
            .poll_video_response(response_url, request_headers, poll_overrides)
            .await
        {
            Ok(response) => response,
            Err(error) => {
                return fal_video_result_from_error(
                    &self.model_id,
                    error,
                    queue_response.response_headers,
                    warnings,
                    timestamp,
                );
            }
        };

        fal_video_result_from_response(
            &self.model_id,
            final_response.value,
            final_response.response_headers,
            warnings,
            timestamp,
        )
    }

    async fn poll_video_response(
        &self,
        response_url: String,
        headers: BTreeMap<String, Option<String>>,
        overrides: FalPollOverrides,
    ) -> Result<ai_sdk_rust::ResponseHandlerResult<FalVideoResponse>, String> {
        let poll_interval_millis = overrides
            .poll_interval_millis
            .unwrap_or(DEFAULT_FAL_POLL_INTERVAL_MILLIS);
        let poll_timeout_millis = overrides
            .poll_timeout_millis
            .unwrap_or(DEFAULT_FAL_POLL_TIMEOUT_MILLIS);
        let start = Instant::now();

        loop {
            let transport = Arc::clone(&self.transport);
            match get_from_api(
                GetFromApiOptions::new(response_url.clone())
                    .with_headers(headers.clone())
                    .with_environment(RuntimeEnvironment::unknown()),
                move |request| (transport)(request),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        fal_video_response,
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| {
                    Ok(create_json_error_response_handler(
                        response.json_error_response_handler_options(request),
                        fal_error_response,
                        fal_error_message,
                        |_, _| None,
                    ))
                },
            )
            .await
            {
                Ok(response) => return Ok(response),
                Err(error) => {
                    let message = fal_handled_error_parts(error).0;
                    if message != "Request is still in progress" {
                        return Err(message);
                    }
                }
            }

            if start.elapsed().as_millis() > u128::from(poll_timeout_millis) {
                return Err(format!(
                    "Video generation request timed out after {poll_timeout_millis}ms"
                ));
            }

            delay(Some(poll_interval_millis as i64)).await;
        }
    }

    fn queue_url(&self) -> String {
        format!(
            "{}/{}",
            DEFAULT_FAL_QUEUE_BASE_URL,
            self.normalized_model_id()
        )
    }

    fn normalized_model_id(&self) -> String {
        self.model_id
            .strip_prefix("fal-ai/")
            .or_else(|| self.model_id.strip_prefix("fal/"))
            .unwrap_or(&self.model_id)
            .to_string()
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(fal_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
        ]))
    }
}

impl VideoModel for FalVideoModel {
    type MaxVideosPerCallFuture<'a>
        = FalVideoMaxVideosFuture<'a>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = FalVideoGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        FalVideoModel::provider(self)
    }

    fn model_id(&self) -> &str {
        FalVideoModel::model_id(self)
    }

    fn max_videos_per_call(&self) -> Self::MaxVideosPerCallFuture<'_> {
        ready(Some(1))
    }

    fn do_generate(&self, options: VideoModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates a fal provider with explicit settings.
pub fn create_fal(settings: FalProviderSettings) -> FalProvider {
    FalProvider::from_settings(settings)
}

/// Creates a fal provider with default settings.
pub fn fal() -> FalProvider {
    FalProvider::new()
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct FalImageModelOptions {
    values: JsonObject,
    deprecated_keys: Vec<String>,
    use_multiple_images: bool,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct FalVideoModelOptions {
    #[serde(default)]
    loop_: Option<bool>,
    #[serde(default)]
    motion_strength: Option<f64>,
    #[serde(default)]
    poll_interval_ms: Option<u64>,
    #[serde(default)]
    poll_timeout_ms: Option<u64>,
    #[serde(default)]
    resolution: Option<String>,
    #[serde(default)]
    negative_prompt: Option<String>,
    #[serde(default)]
    prompt_optimizer: Option<bool>,
    #[serde(flatten)]
    extra: JsonObject,
}

impl FalVideoModelOptions {
    fn validate(&self) -> Result<(), &'static str> {
        if self
            .motion_strength
            .is_some_and(|value| !(0.0..=1.0).contains(&value))
        {
            return Err("motionStrength must be between 0 and 1");
        }
        if self.poll_interval_ms.is_some_and(|value| value == 0) {
            return Err("pollIntervalMs must be positive");
        }
        if self.poll_timeout_ms.is_some_and(|value| value == 0) {
            return Err("pollTimeoutMs must be positive");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct FalImageResponse {
    #[serde(default)]
    images: Vec<FalImage>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    has_nsfw_concepts: Option<Vec<bool>>,
    #[serde(default)]
    nsfw_content_detected: Option<Vec<bool>>,
    #[serde(flatten)]
    extra: JsonObject,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct FalImage {
    url: String,
    #[serde(default)]
    width: Option<u64>,
    #[serde(default)]
    height: Option<u64>,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    file_data: Option<String>,
    #[serde(default)]
    file_size: Option<u64>,
    #[serde(flatten)]
    extra: JsonObject,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct FalRawImageResponse {
    #[serde(default)]
    images: Option<Vec<FalImage>>,
    #[serde(default)]
    image: Option<FalImage>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    has_nsfw_concepts: Option<Vec<bool>>,
    #[serde(default)]
    nsfw_content_detected: Option<Vec<bool>>,
    #[serde(flatten)]
    extra: JsonObject,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct FalJobResponse {
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    response_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct FalVideoResponse {
    #[serde(default)]
    video: Option<FalVideo>,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default)]
    timings: Option<JsonObject>,
    #[serde(default)]
    has_nsfw_concepts: Option<Vec<bool>>,
    #[serde(default)]
    prompt: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct FalVideo {
    url: String,
    #[serde(default)]
    width: Option<u64>,
    #[serde(default)]
    height: Option<u64>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    fps: Option<f64>,
    #[serde(default)]
    content_type: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct FalErrorResponse {
    #[serde(default)]
    error: Option<FalErrorBody>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    detail: Option<JsonValue>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct FalErrorBody {
    message: String,
    #[serde(default)]
    code: Option<i64>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct FalPollOverrides {
    poll_interval_millis: Option<u64>,
    poll_timeout_millis: Option<u64>,
}

fn fal_image_request_body(
    options: &ImageModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>), String> {
    let mut warnings = Vec::new();
    let fal_options = parse_provider_options(
        "fal",
        Some(&options.provider_options),
        fal_image_model_options,
    )
    .map_err(|error| error.to_string())?
    .unwrap_or_default();
    let mut body = JsonObject::new();

    insert_option_string_ref(&mut body, "prompt", options.prompt.as_ref());
    insert_option_u64(&mut body, "seed", options.seed);
    if let Some(image_size) =
        fal_image_size(options.size.as_deref(), options.aspect_ratio.as_deref())
    {
        body.insert("image_size".to_string(), image_size);
    }
    body.insert("num_images".to_string(), JsonValue::from(options.n));

    if let Some(files) = options.files.as_ref().filter(|files| !files.is_empty()) {
        if fal_options.use_multiple_images {
            body.insert(
                "image_urls".to_string(),
                JsonValue::Array(
                    files
                        .iter()
                        .map(|file| JsonValue::String(convert_image_model_file_to_data_uri(file)))
                        .collect(),
                ),
            );
        } else {
            body.insert(
                "image_url".to_string(),
                JsonValue::String(convert_image_model_file_to_data_uri(&files[0])),
            );
            if files.len() > 1 {
                warnings.push(Warning::Other {
                    message:
                        "Multiple input images provided but useMultipleImages is not enabled. Only the first image will be used. Set providerOptions.fal.useMultipleImages to true for models that support multiple images (e.g., fal-ai/flux-2/edit)."
                            .to_string(),
                });
            }
        }
    }

    if let Some(mask) = options.mask.as_ref() {
        body.insert(
            "mask_url".to_string(),
            JsonValue::String(convert_image_model_file_to_data_uri(mask)),
        );
    }

    if !fal_options.deprecated_keys.is_empty() {
        warnings.push(Warning::Other {
            message: fal_deprecated_warning(&fal_options.deprecated_keys),
        });
    }

    for (key, value) in fal_options.values {
        if key != "useMultipleImages" {
            body.insert(fal_image_api_key(&key).to_string(), value);
        }
    }

    Ok((JsonValue::Object(body), warnings))
}

fn fal_video_request_body(
    options: &VideoModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>, FalPollOverrides), String> {
    let warnings = Vec::new();
    let fal_options = parse_provider_options(
        "fal",
        Some(&options.provider_options),
        fal_video_model_options,
    )
    .map_err(|error| error.to_string())?
    .unwrap_or_default();
    let mut body = JsonObject::new();

    insert_option_string_ref(&mut body, "prompt", options.prompt.as_ref());
    if let Some(image) = options.image.as_ref() {
        body.insert(
            "image_url".to_string(),
            JsonValue::String(fal_video_file_data_uri(image)),
        );
    }
    insert_option_string_ref(&mut body, "aspect_ratio", options.aspect_ratio.as_ref());
    if let Some(duration) = options.duration {
        body.insert(
            "duration".to_string(),
            JsonValue::String(format!("{duration}s")),
        );
    }
    insert_option_u64(&mut body, "seed", options.seed);
    insert_option_bool(&mut body, "loop", fal_options.loop_);
    insert_option_f64(&mut body, "motion_strength", fal_options.motion_strength);
    insert_option_string(&mut body, "resolution", fal_options.resolution);
    insert_option_string(&mut body, "negative_prompt", fal_options.negative_prompt);
    insert_option_bool(&mut body, "prompt_optimizer", fal_options.prompt_optimizer);

    let handled = fal_video_handled_options();
    for (key, value) in fal_options.extra {
        if !handled.contains(key.as_str()) {
            body.insert(key, value);
        }
    }

    Ok((
        JsonValue::Object(body),
        warnings,
        FalPollOverrides {
            poll_interval_millis: fal_options.poll_interval_ms,
            poll_timeout_millis: fal_options.poll_timeout_ms,
        },
    ))
}

fn fal_image_model_options(value: &JsonValue) -> Result<FalImageModelOptions, String> {
    let Some(object) = value.as_object() else {
        return Err("fal provider options must be an object".to_string());
    };
    let mut values = JsonObject::new();
    let mut deprecated_keys = Vec::new();

    for (snake, camel) in [
        ("image_url", "imageUrl"),
        ("mask_url", "maskUrl"),
        ("guidance_scale", "guidanceScale"),
        ("num_inference_steps", "numInferenceSteps"),
        ("enable_safety_checker", "enableSafetyChecker"),
        ("output_format", "outputFormat"),
        ("sync_mode", "syncMode"),
        ("safety_tolerance", "safetyTolerance"),
    ] {
        if let Some(value) = object.get(snake).filter(|value| !value.is_null()) {
            deprecated_keys.push(snake.to_string());
            values.insert(camel.to_string(), value.clone());
        } else if let Some(value) = object.get(camel).filter(|value| !value.is_null()) {
            values.insert(camel.to_string(), value.clone());
        }
    }

    for key in ["strength", "acceleration", "useMultipleImages"] {
        if let Some(value) = object.get(key).filter(|value| !value.is_null()) {
            values.insert(key.to_string(), value.clone());
        }
    }

    for (key, value) in object {
        if !fal_known_image_option_keys().contains(key.as_str()) {
            values.insert(key.clone(), value.clone());
        }
    }

    validate_fal_image_options(&values)?;

    Ok(FalImageModelOptions {
        use_multiple_images: values
            .get("useMultipleImages")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false),
        values,
        deprecated_keys,
    })
}

fn fal_video_model_options(value: &JsonValue) -> Result<FalVideoModelOptions, String> {
    let options = serde_json::from_value::<FalVideoModelOptions>(value.clone())
        .map_err(|error| error.to_string())?;
    options.validate().map_err(str::to_string)?;
    Ok(options)
}

fn validate_fal_image_options(values: &JsonObject) -> Result<(), String> {
    if values
        .get("guidanceScale")
        .and_then(JsonValue::as_f64)
        .is_some_and(|value| !(1.0..=20.0).contains(&value))
    {
        return Err("guidanceScale must be between 1 and 20".to_string());
    }
    if values
        .get("numInferenceSteps")
        .and_then(JsonValue::as_f64)
        .is_some_and(|value| !(1.0..=50.0).contains(&value))
    {
        return Err("numInferenceSteps must be between 1 and 50".to_string());
    }
    if let Some(value) = values.get("outputFormat").and_then(JsonValue::as_str) {
        if !matches!(value, "jpeg" | "png") {
            return Err("outputFormat must be jpeg or png".to_string());
        }
    }
    Ok(())
}

fn fal_known_image_option_keys() -> BTreeSet<&'static str> {
    [
        "imageUrl",
        "maskUrl",
        "guidanceScale",
        "numInferenceSteps",
        "enableSafetyChecker",
        "outputFormat",
        "syncMode",
        "strength",
        "acceleration",
        "safetyTolerance",
        "useMultipleImages",
        "image_url",
        "mask_url",
        "guidance_scale",
        "num_inference_steps",
        "enable_safety_checker",
        "output_format",
        "sync_mode",
        "safety_tolerance",
    ]
    .into_iter()
    .collect()
}

fn fal_video_handled_options() -> BTreeSet<&'static str> {
    [
        "loop",
        "motionStrength",
        "pollIntervalMs",
        "pollTimeoutMs",
        "resolution",
        "negativePrompt",
        "promptOptimizer",
    ]
    .into_iter()
    .collect()
}

fn fal_image_api_key(key: &str) -> &str {
    match key {
        "imageUrl" => "image_url",
        "maskUrl" => "mask_url",
        "guidanceScale" => "guidance_scale",
        "numInferenceSteps" => "num_inference_steps",
        "enableSafetyChecker" => "enable_safety_checker",
        "outputFormat" => "output_format",
        "syncMode" => "sync_mode",
        "safetyTolerance" => "safety_tolerance",
        other => other,
    }
}

fn fal_deprecated_warning(deprecated_keys: &[String]) -> String {
    let replacements = deprecated_keys
        .iter()
        .map(|key| {
            let camel_case = snake_to_camel_case(key);
            format!("'{key}' (use '{camel_case}')")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "The following provider options use deprecated snake_case and will be removed in @ai-sdk/fal v2.0. Please use camelCase instead: {replacements}"
    )
}

fn snake_to_camel_case(value: &str) -> String {
    let mut output = String::new();
    let mut uppercase_next = false;
    for character in value.chars() {
        if character == '_' {
            uppercase_next = true;
        } else if uppercase_next {
            output.extend(character.to_uppercase());
            uppercase_next = false;
        } else {
            output.push(character);
        }
    }
    output
}

fn fal_image_size(size: Option<&str>, aspect_ratio: Option<&str>) -> Option<JsonValue> {
    if let Some(size) = size {
        let (width, height) = size.split_once('x')?;
        let mut object = JsonObject::new();
        object.insert(
            "width".to_string(),
            JsonValue::from(width.parse::<u64>().ok()?),
        );
        object.insert(
            "height".to_string(),
            JsonValue::from(height.parse::<u64>().ok()?),
        );
        return Some(JsonValue::Object(object));
    }

    match aspect_ratio {
        Some("1:1") => Some(JsonValue::String("square_hd".to_string())),
        Some("16:9") => Some(JsonValue::String("landscape_16_9".to_string())),
        Some("9:16") => Some(JsonValue::String("portrait_16_9".to_string())),
        Some("4:3") => Some(JsonValue::String("landscape_4_3".to_string())),
        Some("3:4") => Some(JsonValue::String("portrait_4_3".to_string())),
        Some("16:10") => Some(fal_image_size_object(1280, 800)),
        Some("10:16") => Some(fal_image_size_object(800, 1280)),
        Some("21:9") => Some(fal_image_size_object(2560, 1080)),
        Some("9:21") => Some(fal_image_size_object(1080, 2560)),
        _ => None,
    }
}

fn fal_image_size_object(width: u64, height: u64) -> JsonValue {
    let mut object = JsonObject::new();
    object.insert("width".to_string(), JsonValue::from(width));
    object.insert("height".to_string(), JsonValue::from(height));
    JsonValue::Object(object)
}

fn fal_video_file_data_uri(file: &ai_sdk_rust::VideoModelFile) -> String {
    match file {
        ai_sdk_rust::VideoModelFile::Url { url, .. } => url.as_str().to_string(),
        ai_sdk_rust::VideoModelFile::File {
            media_type, data, ..
        } => convert_image_model_file_to_data_uri(&ImageModelFile::file(
            media_type.clone(),
            data.clone(),
        )),
    }
}

fn fal_provider_header_entries(
    settings: &FalProviderSettings,
) -> Result<Vec<(String, Option<String>)>, LoadApiKeyError> {
    let mut headers = vec![(
        "Authorization".to_string(),
        Some(format!("Key {}", fal_api_key(settings.api_key.as_ref())?)),
    )];

    headers.extend(
        settings
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    );

    Ok(with_user_agent_suffix(
        Some(headers),
        [format!("ai-sdk/fal/{}", ai_sdk_rust::VERSION)],
    )
    .into_iter()
    .map(|(name, value)| (name, Some(value)))
    .collect())
}

fn optional_headers(headers: Option<&Headers>) -> Option<Vec<(String, Option<String>)>> {
    headers.map(|headers| {
        headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone())))
            .collect()
    })
}

fn fal_api_key(explicit_api_key: Option<&String>) -> Result<String, LoadApiKeyError> {
    if let Some(api_key) = explicit_api_key {
        return Ok(api_key.clone());
    }

    env::var("FAL_API_KEY")
        .or_else(|_| env::var("FAL_KEY"))
        .map_err(|_| {
            LoadApiKeyError::new(
                "fal.ai API key is missing. Pass it using the 'apiKey' parameter or set either the FAL_API_KEY or FAL_KEY environment variable."
                    .to_string(),
            )
        })
}

fn fal_image_response(value: &JsonValue) -> Result<FalImageResponse, serde_json::Error> {
    let raw = serde_json::from_value::<FalRawImageResponse>(value.clone())?;
    let images = raw
        .images
        .or_else(|| raw.image.map(|image| vec![image]))
        .unwrap_or_default();

    Ok(FalImageResponse {
        images,
        prompt: raw.prompt,
        has_nsfw_concepts: raw.has_nsfw_concepts,
        nsfw_content_detected: raw.nsfw_content_detected,
        extra: raw.extra,
    })
}

fn fal_job_response(value: &JsonValue) -> Result<FalJobResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn fal_video_response(value: &JsonValue) -> Result<FalVideoResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn fal_error_response(value: &JsonValue) -> Result<FalErrorResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn fal_error_message(error: &FalErrorResponse) -> String {
    if let Some(body) = error.error.as_ref() {
        return body.message.clone();
    }
    if let Some(detail) = error.detail.as_ref() {
        if let Some(detail) = detail.as_str() {
            return detail.to_string();
        }
        if let Some(details) = detail.as_array() {
            let messages = details
                .iter()
                .filter_map(|detail| {
                    let loc = detail
                        .get("loc")
                        .and_then(JsonValue::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(JsonValue::as_str)
                                .collect::<Vec<_>>()
                                .join(".")
                        })
                        .unwrap_or_default();
                    let msg = detail.get("msg").and_then(JsonValue::as_str)?;
                    Some(format!("{loc}: {msg}"))
                })
                .collect::<Vec<_>>();
            if !messages.is_empty() {
                return messages.join("\n");
            }
        }
    }
    error
        .message
        .clone()
        .unwrap_or_else(|| "Unknown fal error".to_string())
}

fn fal_handled_error_parts(error: HandledFetchError) -> (String, Option<Headers>) {
    match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
        ),
    }
}

fn fal_image_metadata(image: FalImage, nsfw: Option<bool>) -> JsonValue {
    let mut metadata = image.extra;
    insert_option_json_u64(&mut metadata, "width", image.width);
    insert_option_json_u64(&mut metadata, "height", image.height);
    insert_option_json_string(&mut metadata, "contentType", image.content_type);
    insert_option_json_string(&mut metadata, "fileName", image.file_name);
    insert_option_json_string(&mut metadata, "fileData", image.file_data);
    insert_option_json_u64(&mut metadata, "fileSize", image.file_size);
    if let Some(nsfw) = nsfw {
        metadata.insert("nsfw".to_string(), JsonValue::Bool(nsfw));
    }
    JsonValue::Object(metadata)
}

fn fal_image_result_from_error(
    model_id: &str,
    message: String,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> ImageModelResult {
    let mut result = ImageModelResult::new(
        Vec::new(),
        fal_image_response_metadata(model_id, headers, timestamp),
    )
    .with_provider_metadata(ImageModelProviderMetadata::from([(
        "fal".to_string(),
        ImageModelProviderMetadataEntry {
            images: Vec::new(),
            extra: object_with_string("errorMessage", message),
        },
    )]));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn fal_video_result_from_error(
    model_id: &str,
    message: String,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> VideoModelResult {
    let mut result = VideoModelResult::new(
        Vec::new(),
        fal_video_response_metadata(model_id, headers, timestamp),
    )
    .with_provider_metadata(ProviderMetadata::from([(
        "fal".to_string(),
        object_with_string("errorMessage", message),
    )]));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn fal_video_result_from_response(
    model_id: &str,
    response: FalVideoResponse,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> VideoModelResult {
    let Some(video) = response.video else {
        return fal_video_result_from_error(
            model_id,
            "No video URL in response".to_string(),
            headers,
            warnings,
            timestamp,
        );
    };
    let Ok(url) = Url::parse(&video.url) else {
        return fal_video_result_from_error(
            model_id,
            "No video URL in response".to_string(),
            headers,
            warnings,
            timestamp,
        );
    };
    let media_type = video
        .content_type
        .clone()
        .unwrap_or_else(|| "video/mp4".to_string());
    let mut provider = JsonObject::new();
    let mut video_metadata = JsonObject::new();
    video_metadata.insert("url".to_string(), JsonValue::String(video.url));
    insert_option_json_u64(&mut video_metadata, "width", video.width);
    insert_option_json_u64(&mut video_metadata, "height", video.height);
    insert_option_json_f64(&mut video_metadata, "duration", video.duration);
    insert_option_json_f64(&mut video_metadata, "fps", video.fps);
    insert_option_json_string(&mut video_metadata, "contentType", video.content_type);
    provider.insert(
        "videos".to_string(),
        JsonValue::Array(vec![JsonValue::Object(video_metadata)]),
    );
    insert_option_json_u64(&mut provider, "seed", response.seed);
    if let Some(timings) = response.timings {
        provider.insert("timings".to_string(), JsonValue::Object(timings));
    }
    if let Some(nsfw) = response.has_nsfw_concepts {
        provider.insert(
            "has_nsfw_concepts".to_string(),
            JsonValue::Array(nsfw.into_iter().map(JsonValue::Bool).collect()),
        );
    }
    insert_option_json_string(&mut provider, "prompt", response.prompt);

    let mut result = VideoModelResult::new(
        vec![VideoModelVideoData::url(url, media_type)],
        fal_video_response_metadata(model_id, headers, timestamp),
    )
    .with_provider_metadata(ProviderMetadata::from([("fal".to_string(), provider)]));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn fal_image_response_metadata(
    model_id: &str,
    headers: Option<Headers>,
    timestamp: OffsetDateTime,
) -> ImageModelResponse {
    let mut response = ImageModelResponse::new(timestamp, model_id);
    if let Some(headers) = headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }
    response
}

fn fal_video_response_metadata(
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

fn insert_option_string(body: &mut JsonObject, name: &str, value: Option<String>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::String(value));
    }
}

fn insert_option_string_ref(body: &mut JsonObject, name: &str, value: Option<&String>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::String(value.clone()));
    }
}

fn insert_option_bool(body: &mut JsonObject, name: &str, value: Option<bool>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::Bool(value));
    }
}

fn insert_option_u64(body: &mut JsonObject, name: &str, value: Option<u64>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::from(value));
    }
}

fn insert_option_f64(body: &mut JsonObject, name: &str, value: Option<f64>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::from(value));
    }
}

fn insert_option_json_string(body: &mut JsonObject, name: &str, value: Option<String>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::String(value));
    }
}

fn insert_option_json_u64(body: &mut JsonObject, name: &str, value: Option<u64>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::from(value));
    }
}

fn insert_option_json_f64(body: &mut JsonObject, name: &str, value: Option<f64>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::from(value));
    }
}

fn object_with_string(name: &str, value: impl Into<String>) -> JsonObject {
    let mut object = JsonObject::new();
    object.insert(name.to_string(), JsonValue::String(value.into()));
    object
}

fn default_fal_transport() -> FalTransport {
    Arc::new(|request| Box::pin(ready(execute_fal_request(request))))
}

fn execute_fal_request(request: ProviderApiRequest) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_fal_get_request(request),
        ProviderApiRequestMethod::Post => execute_fal_post_request(request),
    }
}

fn execute_fal_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    fal_provider_api_response(builder.config().http_status_as_error(false).build().call())
}

fn execute_fal_post_request(
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
                "multipart form data is not supported by the fal transport",
            ));
        }
        None => builder.send_empty(),
    };
    fal_provider_api_response(response)
}

fn fal_provider_api_response(
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
    use super::{FalProviderSettings, FalTransport, FalTransportFuture, create_fal};
    use ai_sdk_rust::{
        FileDataContent, ImageModel, ImageModelCallOptions, ImageModelFile, ProviderApiRequest,
        ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse, ProviderOptions,
        VideoModel, VideoModelCallOptions, Warning,
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

    fn request_body_json(request: &ProviderApiRequest) -> serde_json::Value {
        let Some(ProviderApiRequestBody::Text { content }) = request.body.as_ref() else {
            panic!("expected text request body");
        };
        serde_json::from_str(content).expect("request body is valid JSON")
    }

    #[test]
    fn fal_image_model_maps_request_downloads_metadata_and_warnings() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: FalTransport = Arc::new(move |request| -> FalTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());

            let response = match (request.method, request.url.as_str()) {
                (ProviderApiRequestMethod::Post, "https://api.example.com/fal-ai/qwen-image") => {
                    json_response(json!({
                        "images": [{
                            "url": "https://fal.example/image.png",
                            "width": 1024,
                            "height": 768,
                            "content_type": "image/png",
                            "file_name": "image.png",
                            "file_size": 42
                        }],
                        "has_nsfw_concepts": [false],
                        "seed": 123
                    }))
                    .with_headers(
                        [("x-request-id".to_string(), "img-1".to_string())]
                            .into_iter()
                            .collect(),
                    )
                }
                (ProviderApiRequestMethod::Get, "https://fal.example/image.png") => {
                    ProviderApiResponse::bytes(200, "OK", vec![1, 2, 3])
                }
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"message": "unexpected request"}).to_string(),
                ),
            };

            Box::pin(ready(Ok(response)))
        });
        let provider = create_fal(
            FalProviderSettings::new()
                .with_api_key("test-key")
                .with_base_url("https://api.example.com")
                .with_header("x-provider-header", "provider"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "fal".to_string(),
            serde_json::from_value(json!({
                "guidance_scale": 7.5,
                "useMultipleImages": true,
                "syncMode": true,
                "custom": "value"
            }))
            .expect("provider options deserialize"),
        );

        let result = poll_ready(
            provider.image("fal-ai/qwen-image").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A mountain")
                    .with_aspect_ratio("16:9")
                    .with_seed(42)
                    .with_files(vec![
                        ImageModelFile::url(
                            Url::parse("https://example.com/a.png").expect("valid URL"),
                        ),
                        ImageModelFile::file(
                            "image/png",
                            FileDataContent::Base64("iVBORw==".to_string()),
                        ),
                    ])
                    .with_provider_options(provider_options)
                    .with_header("x-request-header", "request"),
            ),
        );

        assert_eq!(result.images, vec![FileDataContent::Bytes(vec![1, 2, 3])]);
        assert_eq!(result.response.timestamp, fixed_timestamp());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("fal"))
                .and_then(|metadata| metadata.images.first())
                .and_then(|image| image.get("nsfw")),
            Some(&json!(false))
        );
        assert_eq!(result.warnings.len(), 1);
        assert!(matches!(result.warnings[0], Warning::Other { .. }));

        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(
            requests[0].headers.get("authorization"),
            Some(&"Key test-key".to_string())
        );
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "prompt": "A mountain",
                "seed": 42,
                "image_size": "landscape_16_9",
                "num_images": 1,
                "image_urls": [
                    "https://example.com/a.png",
                    "data:image/png;base64,iVBORw=="
                ],
                "guidance_scale": 7.5,
                "sync_mode": true,
                "custom": "value"
            })
        );
    }

    #[test]
    fn fal_image_model_warns_when_multiple_images_are_not_enabled() {
        let transport: FalTransport = Arc::new(move |request| -> FalTransportFuture {
            let response = match (request.method, request.url.as_str()) {
                (ProviderApiRequestMethod::Post, _) => json_response(json!({
                    "image": { "url": "https://fal.example/image.png" }
                })),
                (ProviderApiRequestMethod::Get, _) => {
                    ProviderApiResponse::bytes(200, "OK", vec![1])
                }
            };
            Box::pin(ready(Ok(response)))
        });
        let provider = create_fal(FalProviderSettings::new().with_api_key("test-key"))
            .with_transport(transport);

        let result = poll_ready(
            provider.image("fal-ai/edit").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("Edit")
                    .with_files(vec![
                        ImageModelFile::url(
                            Url::parse("https://example.com/a.png").expect("valid URL"),
                        ),
                        ImageModelFile::url(
                            Url::parse("https://example.com/b.png").expect("valid URL"),
                        ),
                    ]),
            ),
        );

        assert_eq!(result.images, vec![FileDataContent::Bytes(vec![1])]);
        assert!(matches!(result.warnings[0], Warning::Other { .. }));
    }

    #[test]
    fn fal_video_model_maps_queue_response_and_metadata() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: FalTransport = Arc::new(move |request| -> FalTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());
            let response = match (request.method, request.url.as_str()) {
                (
                    ProviderApiRequestMethod::Post,
                    "https://queue.fal.run/fal-ai/luma-dream-machine",
                ) => json_response(json!({
                    "request_id": "request-123",
                    "response_url": "https://queue.fal.run/fal-ai/luma-dream-machine/requests/request-123"
                })),
                (
                    ProviderApiRequestMethod::Get,
                    "https://queue.fal.run/fal-ai/luma-dream-machine/requests/request-123",
                ) => json_response(json!({
                    "video": {
                        "url": "https://fal.example/video.mp4",
                        "width": 1280,
                        "height": 720,
                        "duration": 5,
                        "fps": 24,
                        "content_type": "video/mp4"
                    },
                    "seed": 99,
                    "timings": { "inference": 1.5 },
                    "has_nsfw_concepts": [false],
                    "prompt": "Enhanced prompt"
                }))
                .with_headers(
                    [("x-status-id".to_string(), "status-1".to_string())]
                        .into_iter()
                        .collect(),
                ),
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"message": "unexpected request"}).to_string(),
                ),
            };
            Box::pin(ready(Ok(response)))
        });
        let provider = create_fal(FalProviderSettings::new().with_api_key("test-key"))
            .with_transport(transport)
            .with_current_date(fixed_timestamp);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "fal".to_string(),
            serde_json::from_value(json!({
                "motionStrength": 0.8,
                "resolution": "720p",
                "pollIntervalMs": 1,
                "pollTimeoutMs": 10
            }))
            .expect("provider options deserialize"),
        );

        let result = poll_ready(
            provider.video("luma-dream-machine").do_generate(
                VideoModelCallOptions::new(1)
                    .with_prompt("A river")
                    .with_duration(5.0)
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.videos.len(), 1);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("fal"))
                .and_then(|metadata| metadata.get("seed")),
            Some(&json!(99))
        );
        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "prompt": "A river",
                "duration": "5s",
                "motion_strength": 0.8,
                "resolution": "720p"
            })
        );
    }

    #[test]
    fn fal_models_map_api_errors_to_provider_metadata() {
        let transport: FalTransport = Arc::new(move |_request| -> FalTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                400,
                "Bad Request",
                json!({"detail": [{"loc": ["body", "prompt"], "msg": "invalid", "type": "value_error"}]}).to_string(),
            ))))
        });
        let provider = create_fal(FalProviderSettings::new().with_api_key("test-key"))
            .with_transport(transport)
            .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider
                .image("fal-ai/qwen-image")
                .do_generate(ImageModelCallOptions::new(1).with_prompt("bad")),
        );

        assert!(result.images.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("fal"))
                .and_then(|metadata| metadata.extra.get("errorMessage")),
            Some(&json!("body.prompt: invalid"))
        );
    }
}
