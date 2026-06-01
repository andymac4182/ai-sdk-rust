use std::collections::{BTreeMap, BTreeSet};
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use ai_sdk_rust::{
    DelayOptions, FetchErrorInfo, GetFromApiOptions, HandledFetchError, Headers, ImageModel,
    ImageModelCallOptions, ImageModelFile, ImageModelResponse, ImageModelResult, JsonObject,
    JsonValue, LoadApiKeyError, LoadApiKeyOptions, ModelType, NoSuchModelError,
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, PostJsonToApiOptions,
    Provider, ProviderAbortSignal, ProviderApiRequest, ProviderApiRequestBody,
    ProviderApiRequestMethod, ProviderApiResponse, ProviderApiResponseHandlerError,
    ProviderMetadata, ProviderWithVideoModel, RuntimeEnvironment, VideoModel,
    VideoModelCallOptions, VideoModelResponse, VideoModelResult, VideoModelVideoData, Warning,
    combine_headers, convert_image_model_file_to_data_uri, create_binary_response_handler,
    create_json_error_response_handler, create_json_response_handler, delay_with_options,
    get_from_api, load_api_key, parse_provider_options, post_json_to_api, with_user_agent_suffix,
    without_trailing_slash,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use url::Url;

/// Default base URL for upstream `@ai-sdk/replicate` API calls.
pub const DEFAULT_REPLICATE_BASE_URL: &str = "https://api.replicate.com/v1";

const DEFAULT_REPLICATE_POLL_INTERVAL_MILLIS: u64 = 2_000;
const DEFAULT_REPLICATE_POLL_TIMEOUT_MILLIS: u64 = 300_000;
const FLUX_2_MODEL_PREFIX: &str = "black-forest-labs/flux-2-";
const MAX_FLUX_2_INPUT_IMAGES: usize = 8;

/// Settings for the upstream Replicate provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplicateProviderSettings {
    /// Replicate API token. When omitted, `REPLICATE_API_TOKEN` is read at request time.
    #[serde(default, alias = "apiKey", skip_serializing_if = "Option::is_none")]
    pub api_token: Option<String>,

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

impl ReplicateProviderSettings {
    /// Creates empty Replicate provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Replicate API token.
    pub fn with_api_token(mut self, api_token: impl Into<String>) -> Self {
        self.api_token = Some(api_token.into());
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

/// Upstream Replicate provider foundation.
#[derive(Clone)]
pub struct ReplicateProvider {
    base_url: String,
    settings: ReplicateProviderSettings,
    transport: ReplicateTransport,
    current_date: ReplicateDateProvider,
}

/// Replicate image model.
#[derive(Clone)]
pub struct ReplicateImageModel {
    model_id: String,
    base_url: String,
    settings: ReplicateProviderSettings,
    transport: ReplicateTransport,
    current_date: ReplicateDateProvider,
}

/// Replicate video model.
#[derive(Clone)]
pub struct ReplicateVideoModel {
    model_id: String,
    base_url: String,
    settings: ReplicateProviderSettings,
    transport: ReplicateTransport,
    current_date: ReplicateDateProvider,
}

/// Future returned by an injected Replicate HTTP transport.
pub type ReplicateTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by Replicate provider models.
pub type ReplicateTransport =
    Arc<dyn Fn(ProviderApiRequest) -> ReplicateTransportFuture + Send + Sync>;

type ReplicateDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type ReplicateImageGenerateFuture<'a> = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>;
type ReplicateVideoMaxVideosFuture<'a> = Ready<Option<usize>>;
type ReplicateVideoGenerateFuture<'a> = Pin<Box<dyn Future<Output = VideoModelResult> + Send + 'a>>;

impl ReplicateProvider {
    /// Creates a Replicate provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(ReplicateProviderSettings::new())
    }

    /// Creates a provider from explicit Replicate settings.
    pub fn from_settings(settings: ReplicateProviderSettings) -> Self {
        let base_url = without_trailing_slash(
            settings
                .base_url
                .as_deref()
                .or(Some(DEFAULT_REPLICATE_BASE_URL)),
        )
        .expect("default Replicate base URL is present")
        .to_string();

        Self {
            base_url,
            settings,
            transport: default_replicate_transport(),
            current_date: Arc::new(OffsetDateTime::now_utc),
        }
    }

    /// Sets the Replicate API token for this provider.
    pub fn with_api_token(mut self, api_token: impl Into<String>) -> Self {
        self.settings.api_token = Some(api_token.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: ReplicateTransport) -> Self {
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
    pub fn image(&self, model_id: impl Into<String>) -> ReplicateImageModel {
        self.image_model(model_id)
            .expect("Replicate image models are supported")
    }

    /// Creates an image model.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<ReplicateImageModel, NoSuchModelError> {
        Ok(ReplicateImageModel::new(
            model_id,
            self.base_url.clone(),
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Creates a video model.
    pub fn video(&self, model_id: impl Into<String>) -> ReplicateVideoModel {
        self.video_model(model_id)
            .expect("Replicate video models are supported")
    }

    /// Creates a video model.
    pub fn video_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<ReplicateVideoModel, NoSuchModelError> {
        Ok(ReplicateVideoModel::new(
            model_id,
            self.base_url.clone(),
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Reports that Replicate does not expose language models through this provider.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::LanguageModel))
    }

    /// Reports that Replicate does not expose embedding models through this provider.
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

impl Default for ReplicateProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for ReplicateProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = ReplicateImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        ReplicateProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        ReplicateProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        ReplicateProvider::image_model(self, model_id)
    }
}

impl ProviderWithVideoModel for ReplicateProvider {
    type VideoModel = ReplicateVideoModel;

    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
        ReplicateProvider::video_model(self, model_id)
    }
}

impl ReplicateImageModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: ReplicateProviderSettings,
        transport: ReplicateTransport,
        current_date: ReplicateDateProvider,
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
        "replicate"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: ReplicateTransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_generate_result(&self, options: ImageModelCallOptions) -> ImageModelResult {
        let timestamp = (self.current_date)();
        let abort_signal = options.abort_signal.clone();
        let (request_body, warnings, prefer_header) =
            match replicate_image_request_body(&self.model_id, &options) {
                Ok(args) => args,
                Err(error) => {
                    return replicate_image_result_from_error(
                        &self.model_id,
                        error,
                        None,
                        Vec::new(),
                        timestamp,
                    );
                }
            };
        let request_headers = match self.request_headers(options.headers.as_ref(), prefer_header) {
            Ok(headers) => headers,
            Err(error) => {
                return replicate_image_result_from_error(
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
            PostJsonToApiOptions::new(self.prediction_url(), request_body)
                .with_headers(request_headers)
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(abort_signal.clone()),
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    replicate_image_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    replicate_error_response,
                    replicate_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                let (message, headers) = replicate_handled_error_parts(error);
                return replicate_image_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    warnings,
                    timestamp,
                );
            }
        };

        let urls = response.value.output_urls();
        let mut images = Vec::with_capacity(urls.len());

        for image_url in urls {
            let transport = Arc::clone(&self.transport);
            match get_from_api(
                GetFromApiOptions::new(image_url)
                    .with_environment(RuntimeEnvironment::unknown())
                    .with_optional_abort_signal(abort_signal.clone()),
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
                        replicate_error_response,
                        replicate_error_message,
                        |_, _| None,
                    ))
                },
            )
            .await
            {
                Ok(image) => images.push(ai_sdk_rust::FileDataContent::Bytes(image.value)),
                Err(error) => {
                    let (message, headers) = replicate_handled_error_parts(error);
                    return replicate_image_result_from_error(
                        &self.model_id,
                        message,
                        headers,
                        warnings,
                        timestamp,
                    );
                }
            }
        }

        let mut result = ImageModelResult::new(
            images,
            replicate_image_response_metadata(&self.model_id, response.response_headers, timestamp),
        );

        for warning in warnings {
            result = result.with_warning(warning);
        }

        result
    }

    fn prediction_url(&self) -> String {
        let (model_id, version) = split_model_version(&self.model_id);
        if version.is_some() {
            format!("{}/predictions", self.base_url)
        } else {
            format!("{}/models/{model_id}/predictions", self.base_url)
        }
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
        prefer: String,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(replicate_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
            Some(vec![("prefer".to_string(), Some(prefer))]),
        ]))
    }
}

impl ImageModel for ReplicateImageModel {
    type MaxImagesPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = ReplicateImageGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        ReplicateImageModel::provider(self)
    }

    fn model_id(&self) -> &str {
        ReplicateImageModel::model_id(self)
    }

    fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
        ready(Some(if is_flux_2_model(&self.model_id) {
            MAX_FLUX_2_INPUT_IMAGES
        } else {
            1
        }))
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

impl ReplicateVideoModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: ReplicateProviderSettings,
        transport: ReplicateTransport,
        current_date: ReplicateDateProvider,
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
        "replicate.video"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: ReplicateTransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_generate_result(&self, options: VideoModelCallOptions) -> VideoModelResult {
        let timestamp = (self.current_date)();
        let abort_signal = options.abort_signal.clone();
        let (request_body, warnings, prefer_header, poll_overrides) =
            match replicate_video_request_body(&self.model_id, &options) {
                Ok(args) => args,
                Err(error) => {
                    return replicate_video_result_from_error(
                        &self.model_id,
                        error,
                        None,
                        Vec::new(),
                        timestamp,
                    );
                }
            };
        let request_headers = match self.request_headers(options.headers.as_ref(), prefer_header) {
            Ok(headers) => headers,
            Err(error) => {
                return replicate_video_result_from_error(
                    &self.model_id,
                    error.to_string(),
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let transport = Arc::clone(&self.transport);
        let prediction = match post_json_to_api(
            PostJsonToApiOptions::new(self.prediction_url(), request_body)
                .with_headers(request_headers.clone())
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(abort_signal.clone()),
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    replicate_prediction_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    replicate_error_response,
                    replicate_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                let (message, headers) = replicate_handled_error_parts(error);
                return replicate_video_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    warnings,
                    timestamp,
                );
            }
        };

        let response_headers = prediction.response_headers.clone();
        let final_prediction = match self
            .poll_prediction_if_needed(
                prediction.value,
                request_headers,
                poll_overrides,
                abort_signal,
            )
            .await
        {
            Ok(prediction) => prediction,
            Err(error) => {
                return replicate_video_result_from_error(
                    &self.model_id,
                    error,
                    response_headers,
                    warnings,
                    timestamp,
                );
            }
        };

        replicate_video_result_from_prediction(
            &self.model_id,
            final_prediction,
            response_headers,
            warnings,
            timestamp,
        )
    }

    async fn poll_prediction_if_needed(
        &self,
        mut prediction: ReplicatePredictionResponse,
        headers: BTreeMap<String, Option<String>>,
        overrides: ReplicatePollOverrides,
        abort_signal: Option<ProviderAbortSignal>,
    ) -> Result<ReplicatePredictionResponse, String> {
        if !prediction.is_pending() {
            return Ok(prediction);
        }

        let poll_interval_millis = overrides
            .poll_interval_millis
            .unwrap_or(DEFAULT_REPLICATE_POLL_INTERVAL_MILLIS);
        let poll_timeout_millis = overrides
            .poll_timeout_millis
            .unwrap_or(DEFAULT_REPLICATE_POLL_TIMEOUT_MILLIS);
        let start = Instant::now();

        while prediction.is_pending() {
            if start.elapsed().as_millis() > u128::from(poll_timeout_millis) {
                return Err(format!(
                    "Video generation timed out after {poll_timeout_millis}ms"
                ));
            }

            let mut delay_options = DelayOptions::new();
            if let Some(abort_signal) = abort_signal.clone() {
                delay_options = delay_options.with_abort_signal(abort_signal);
            }
            delay_with_options(Some(poll_interval_millis as i64), delay_options)
                .await
                .map_err(|error| error.to_string())?;

            let transport = Arc::clone(&self.transport);
            prediction = get_from_api(
                GetFromApiOptions::new(prediction.urls.get.clone())
                    .with_headers(headers.clone())
                    .with_environment(RuntimeEnvironment::unknown())
                    .with_optional_abort_signal(abort_signal.clone()),
                move |request| (transport)(request),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        replicate_prediction_response,
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| {
                    Ok(create_json_error_response_handler(
                        response.json_error_response_handler_options(request),
                        replicate_error_response,
                        replicate_error_message,
                        |_, _| None,
                    ))
                },
            )
            .await
            .map_err(|error| replicate_handled_error_parts(error).0)?
            .value;
        }

        Ok(prediction)
    }

    fn prediction_url(&self) -> String {
        let (model_id, version) = split_model_version(&self.model_id);
        if version.is_some() {
            format!("{}/predictions", self.base_url)
        } else {
            format!("{}/models/{model_id}/predictions", self.base_url)
        }
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
        prefer: String,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(replicate_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
            Some(vec![("prefer".to_string(), Some(prefer))]),
        ]))
    }
}

impl VideoModel for ReplicateVideoModel {
    type MaxVideosPerCallFuture<'a>
        = ReplicateVideoMaxVideosFuture<'a>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = ReplicateVideoGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        ReplicateVideoModel::provider(self)
    }

    fn model_id(&self) -> &str {
        ReplicateVideoModel::model_id(self)
    }

    fn max_videos_per_call(&self) -> Self::MaxVideosPerCallFuture<'_> {
        ready(Some(1))
    }

    fn do_generate(&self, options: VideoModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates a Replicate provider with explicit settings.
pub fn create_replicate(settings: ReplicateProviderSettings) -> ReplicateProvider {
    ReplicateProvider::from_settings(settings)
}

/// Creates a Replicate provider with default settings.
pub fn replicate() -> ReplicateProvider {
    ReplicateProvider::new()
}

/// Provider-specific image options accepted by upstream Replicate.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplicateImageModelOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_wait_time_in_seconds: Option<f64>,
    #[serde(default)]
    pub guidance_scale: Option<f64>,
    #[serde(default)]
    pub num_inference_steps: Option<f64>,
    #[serde(default)]
    pub negative_prompt: Option<String>,
    #[serde(default)]
    pub output_format: Option<String>,
    #[serde(default)]
    pub output_quality: Option<f64>,
    #[serde(default)]
    pub strength: Option<f64>,
    #[serde(flatten)]
    pub extra: JsonObject,
}

impl ReplicateImageModelOptions {
    fn validate(&self) -> Result<(), &'static str> {
        if self
            .max_wait_time_in_seconds
            .is_some_and(|value| value <= 0.0)
        {
            return Err("maxWaitTimeInSeconds must be positive");
        }
        if let Some(output_format) = self.output_format.as_deref() {
            if !matches!(output_format, "png" | "jpg" | "webp") {
                return Err("outputFormat must be png, jpg, or webp");
            }
        }
        if self
            .output_quality
            .is_some_and(|value| !(1.0..=100.0).contains(&value))
        {
            return Err("outputQuality must be between 1 and 100");
        }
        if self
            .strength
            .is_some_and(|value| !(0.0..=1.0).contains(&value))
        {
            return Err("strength must be between 0 and 1");
        }
        Ok(())
    }
}

/// Provider-specific video options accepted by upstream Replicate.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplicateVideoModelOptions {
    #[serde(default)]
    pub poll_interval_ms: Option<u64>,
    #[serde(default)]
    pub poll_timeout_ms: Option<u64>,
    #[serde(default)]
    pub max_wait_time_in_seconds: Option<f64>,
    #[serde(flatten)]
    pub extra: JsonObject,
}

impl ReplicateVideoModelOptions {
    fn validate(&self) -> Result<(), &'static str> {
        if self.poll_interval_ms.is_some_and(|value| value == 0) {
            return Err("pollIntervalMs must be positive");
        }
        if self.poll_timeout_ms.is_some_and(|value| value == 0) {
            return Err("pollTimeoutMs must be positive");
        }
        if self
            .max_wait_time_in_seconds
            .is_some_and(|value| value <= 0.0)
        {
            return Err("maxWaitTimeInSeconds must be positive");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
enum ReplicateImageOutput {
    One(String),
    Many(Vec<String>),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct ReplicateImageResponse {
    output: ReplicateImageOutput,
}

impl ReplicateImageResponse {
    fn output_urls(self) -> Vec<String> {
        match self.output {
            ReplicateImageOutput::One(url) => vec![url],
            ReplicateImageOutput::Many(urls) => urls,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct ReplicatePredictionResponse {
    id: String,
    status: String,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    error: Option<String>,
    urls: ReplicatePredictionUrls,
    #[serde(default)]
    metrics: Option<ReplicatePredictionMetrics>,
}

impl ReplicatePredictionResponse {
    fn is_pending(&self) -> bool {
        matches!(self.status.as_str(), "starting" | "processing")
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct ReplicatePredictionUrls {
    get: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct ReplicatePredictionMetrics {
    #[serde(default)]
    predict_time: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct ReplicateErrorResponse {
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct ReplicatePollOverrides {
    poll_interval_millis: Option<u64>,
    poll_timeout_millis: Option<u64>,
}

fn replicate_image_request_body(
    model_id: &str,
    options: &ImageModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>, String), String> {
    let mut warnings = Vec::new();
    let provider_options = parse_provider_options(
        "replicate",
        Some(&options.provider_options),
        replicate_image_model_options,
    )
    .map_err(|error| error.to_string())?
    .unwrap_or_default();
    let (model_name, version) = split_model_version(model_id);
    let mut input = JsonObject::new();

    insert_option_string_ref(&mut input, "prompt", options.prompt.as_ref());
    insert_option_string_ref(&mut input, "aspect_ratio", options.aspect_ratio.as_ref());
    insert_option_string_ref(&mut input, "size", options.size.as_ref());
    insert_option_u64(&mut input, "seed", options.seed);
    input.insert("num_outputs".to_string(), JsonValue::from(options.n));

    if let Some(files) = options.files.as_ref().filter(|files| !files.is_empty()) {
        if is_flux_2_model(model_id) {
            for (index, file) in files.iter().take(MAX_FLUX_2_INPUT_IMAGES).enumerate() {
                let key = if index == 0 {
                    "input_image".to_string()
                } else {
                    format!("input_image_{}", index + 1)
                };
                input.insert(key, JsonValue::String(replicate_image_file_data_uri(file)));
            }
            if files.len() > MAX_FLUX_2_INPUT_IMAGES {
                warnings.push(Warning::Other {
                    message: format!(
                        "Flux-2 models support up to {MAX_FLUX_2_INPUT_IMAGES} input images. Additional images are ignored."
                    ),
                });
            }
        } else {
            input.insert(
                "image".to_string(),
                JsonValue::String(replicate_image_file_data_uri(&files[0])),
            );
            if files.len() > 1 {
                warnings.push(Warning::Other {
                    message:
                        "This Replicate model only supports a single input image. Additional images are ignored."
                            .to_string(),
                });
            }
        }
    }

    if let Some(mask) = options.mask.as_ref() {
        if is_flux_2_model(model_id) {
            warnings.push(Warning::Other {
                message: "Flux-2 models do not support mask input. The mask will be ignored."
                    .to_string(),
            });
        } else {
            input.insert(
                "mask".to_string(),
                JsonValue::String(replicate_image_file_data_uri(mask)),
            );
        }
    }

    insert_option_f64(
        &mut input,
        "guidance_scale",
        provider_options.guidance_scale,
    );
    insert_option_f64(
        &mut input,
        "num_inference_steps",
        provider_options.num_inference_steps,
    );
    insert_option_string(
        &mut input,
        "negative_prompt",
        provider_options.negative_prompt,
    );
    insert_option_string(&mut input, "output_format", provider_options.output_format);
    insert_option_f64(
        &mut input,
        "output_quality",
        provider_options.output_quality,
    );
    insert_option_f64(&mut input, "strength", provider_options.strength);
    input.extend(provider_options.extra);

    let mut body = JsonObject::new();
    body.insert("input".to_string(), JsonValue::Object(input));
    if let Some(version) = version {
        body.insert(
            "version".to_string(),
            JsonValue::String(version.to_string()),
        );
    } else {
        let _ = model_name;
    }

    Ok((
        JsonValue::Object(body),
        warnings,
        prefer_header(provider_options.max_wait_time_in_seconds),
    ))
}

fn replicate_video_request_body(
    model_id: &str,
    options: &VideoModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>, String, ReplicatePollOverrides), String> {
    let warnings = Vec::new();
    let provider_options = parse_provider_options(
        "replicate",
        Some(&options.provider_options),
        replicate_video_model_options,
    )
    .map_err(|error| error.to_string())?
    .unwrap_or_default();
    let (_, version) = split_model_version(model_id);
    let mut input = JsonObject::new();

    insert_option_string_ref(&mut input, "prompt", options.prompt.as_ref());
    if let Some(image) = options.image.as_ref() {
        input.insert(
            "image".to_string(),
            JsonValue::String(replicate_video_file_data_uri(image)),
        );
    }
    insert_option_string_ref(&mut input, "aspect_ratio", options.aspect_ratio.as_ref());
    insert_option_string_ref(&mut input, "size", options.resolution.as_ref());
    insert_option_f64(&mut input, "duration", options.duration);
    insert_option_f64(&mut input, "fps", options.fps);
    insert_option_u64(&mut input, "seed", options.seed);

    for (key, value) in provider_options.extra.clone() {
        if !replicate_video_control_keys().contains(key.as_str()) {
            input.insert(key, value);
        }
    }

    let mut body = JsonObject::new();
    body.insert("input".to_string(), JsonValue::Object(input));
    if let Some(version) = version {
        body.insert(
            "version".to_string(),
            JsonValue::String(version.to_string()),
        );
    }

    Ok((
        JsonValue::Object(body),
        warnings,
        prefer_header(provider_options.max_wait_time_in_seconds),
        ReplicatePollOverrides {
            poll_interval_millis: provider_options.poll_interval_ms,
            poll_timeout_millis: provider_options.poll_timeout_ms,
        },
    ))
}

fn replicate_image_model_options(value: &JsonValue) -> Result<ReplicateImageModelOptions, String> {
    let options = serde_json::from_value::<ReplicateImageModelOptions>(value.clone())
        .map_err(|error| error.to_string())?;
    options.validate().map_err(str::to_string)?;
    Ok(options)
}

fn replicate_video_model_options(value: &JsonValue) -> Result<ReplicateVideoModelOptions, String> {
    let options = serde_json::from_value::<ReplicateVideoModelOptions>(value.clone())
        .map_err(|error| error.to_string())?;
    options.validate().map_err(str::to_string)?;
    Ok(options)
}

fn split_model_version(model_id: &str) -> (&str, Option<&str>) {
    model_id
        .split_once(':')
        .map_or((model_id, None), |(model, version)| (model, Some(version)))
}

fn is_flux_2_model(model_id: &str) -> bool {
    let (model_id, _) = split_model_version(model_id);
    model_id.starts_with(FLUX_2_MODEL_PREFIX)
}

fn prefer_header(max_wait_time_in_seconds: Option<f64>) -> String {
    match max_wait_time_in_seconds {
        Some(value) => format!("wait={value}"),
        None => "wait".to_string(),
    }
}

fn replicate_image_file_data_uri(file: &ImageModelFile) -> String {
    convert_image_model_file_to_data_uri(file)
}

fn replicate_video_file_data_uri(file: &ai_sdk_rust::VideoModelFile) -> String {
    match file {
        ai_sdk_rust::VideoModelFile::Url { url, .. } => url.as_str().to_string(),
        ai_sdk_rust::VideoModelFile::File {
            media_type, data, ..
        } => {
            let file = ImageModelFile::file(media_type.clone(), data.clone());
            convert_image_model_file_to_data_uri(&file)
        }
    }
}

fn replicate_video_control_keys() -> BTreeSet<&'static str> {
    ["pollIntervalMs", "pollTimeoutMs", "maxWaitTimeInSeconds"]
        .into_iter()
        .collect()
}

fn replicate_provider_header_entries(
    settings: &ReplicateProviderSettings,
) -> Result<Vec<(String, Option<String>)>, LoadApiKeyError> {
    let mut headers = vec![(
        "Authorization".to_string(),
        Some(format!(
            "Bearer {}",
            replicate_api_token(settings.api_token.as_ref())?
        )),
    )];

    headers.extend(
        settings
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    );

    Ok(with_user_agent_suffix(
        Some(headers),
        [format!("ai-sdk/replicate/{}", ai_sdk_rust::VERSION)],
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

fn replicate_api_token(explicit_api_token: Option<&String>) -> Result<String, LoadApiKeyError> {
    let mut options = LoadApiKeyOptions::new("REPLICATE_API_TOKEN", "Replicate");

    if let Some(api_token) = explicit_api_token {
        options = options.with_api_key(api_token.clone());
    }

    load_api_key(options)
}

fn replicate_image_response(
    value: &JsonValue,
) -> Result<ReplicateImageResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn replicate_prediction_response(
    value: &JsonValue,
) -> Result<ReplicatePredictionResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn replicate_error_response(
    value: &JsonValue,
) -> Result<ReplicateErrorResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn replicate_error_message(error: &ReplicateErrorResponse) -> String {
    error
        .detail
        .clone()
        .or_else(|| error.error.clone())
        .unwrap_or_else(|| "Unknown Replicate error".to_string())
}

fn replicate_handled_error_parts(error: HandledFetchError) -> (String, Option<Headers>) {
    match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
        ),
    }
}

fn replicate_image_result_from_error(
    model_id: &str,
    message: String,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> ImageModelResult {
    let mut result = ImageModelResult::new(
        Vec::new(),
        replicate_image_response_metadata(model_id, headers, timestamp),
    )
    .with_provider_metadata(ai_sdk_rust::ImageModelProviderMetadata::from([(
        "replicate".to_string(),
        ai_sdk_rust::ImageModelProviderMetadataEntry {
            images: Vec::new(),
            extra: object_with_string("errorMessage", message),
        },
    )]));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn replicate_video_result_from_error(
    model_id: &str,
    message: String,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> VideoModelResult {
    let mut result = VideoModelResult::new(
        Vec::new(),
        replicate_video_response_metadata(model_id, headers, timestamp),
    )
    .with_provider_metadata(ProviderMetadata::from([(
        "replicate".to_string(),
        object_with_string("errorMessage", message),
    )]));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn replicate_video_result_from_prediction(
    model_id: &str,
    prediction: ReplicatePredictionResponse,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> VideoModelResult {
    if prediction.status == "failed" {
        return replicate_video_result_from_error(
            model_id,
            format!(
                "Video generation failed: {}",
                prediction
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string())
            ),
            headers,
            warnings,
            timestamp,
        );
    }
    if prediction.status == "canceled" {
        return replicate_video_result_from_error(
            model_id,
            "Video generation was canceled".to_string(),
            headers,
            warnings,
            timestamp,
        );
    }

    let Some(video_url) = prediction.output.as_ref() else {
        return replicate_video_result_from_error(
            model_id,
            "No video URL in response".to_string(),
            headers,
            warnings,
            timestamp,
        );
    };
    let Ok(url) = Url::parse(video_url) else {
        return replicate_video_result_from_error(
            model_id,
            "No video URL in response".to_string(),
            headers,
            warnings,
            timestamp,
        );
    };

    let mut provider = JsonObject::new();
    provider.insert(
        "videos".to_string(),
        JsonValue::Array(vec![JsonValue::Object(object_with_string(
            "url",
            video_url.clone(),
        ))]),
    );
    provider.insert("predictionId".to_string(), JsonValue::String(prediction.id));
    if let Some(metrics) = prediction.metrics {
        provider.insert(
            "metrics".to_string(),
            serde_json::to_value(metrics).expect("Replicate metrics serialize"),
        );
    }

    let mut result = VideoModelResult::new(
        vec![VideoModelVideoData::url(url, "video/mp4")],
        replicate_video_response_metadata(model_id, headers, timestamp),
    )
    .with_provider_metadata(ProviderMetadata::from([(
        "replicate".to_string(),
        provider,
    )]));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn replicate_image_response_metadata(
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

fn replicate_video_response_metadata(
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

fn object_with_string(name: &str, value: impl Into<String>) -> JsonObject {
    let mut object = JsonObject::new();
    object.insert(name.to_string(), JsonValue::String(value.into()));
    object
}

fn default_replicate_transport() -> ReplicateTransport {
    Arc::new(|request| Box::pin(ready(execute_replicate_request(request))))
}

fn execute_replicate_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_replicate_get_request(request),
        ProviderApiRequestMethod::Post => execute_replicate_post_request(request),
    }
}

fn execute_replicate_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    replicate_provider_api_response(builder.config().http_status_as_error(false).build().call())
}

fn execute_replicate_post_request(
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
                "multipart form data is not supported by the Replicate transport",
            ));
        }
        None => builder.send_empty(),
    };

    replicate_provider_api_response(response)
}

fn replicate_provider_api_response(
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
        ReplicateProviderSettings, ReplicateTransport, ReplicateTransportFuture, create_replicate,
        replicate_image_request_body, replicate_prediction_response, replicate_video_request_body,
        replicate_video_result_from_prediction,
    };
    use ai_sdk_rust::{
        FileDataContent, ImageModel, ImageModelCallOptions, ImageModelFile,
        ProviderAbortController, ProviderApiRequest, ProviderApiRequestBody,
        ProviderApiRequestMethod, ProviderApiResponse, ProviderOptions, VideoModel,
        VideoModelCallOptions, Warning,
    };
    use serde_json::json;
    use std::env;
    use std::future::Future;
    use std::future::ready;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use std::thread;
    use std::time::{Duration, Instant};
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

    fn poll_until_ready<F>(future: F, timeout: Duration) -> F::Output
    where
        F: Future,
    {
        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        let start = Instant::now();

        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => {
                    assert!(
                        start.elapsed() <= timeout,
                        "future did not complete within {timeout:?}"
                    );
                    thread::sleep(Duration::from_millis(5));
                }
            }
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

    fn replicate_provider_options(value: serde_json::Value) -> ProviderOptions {
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "replicate".to_string(),
            serde_json::from_value(value).expect("provider options deserialize"),
        );
        provider_options
    }

    #[test]
    #[ignore = "requires REPLICATE_API_TOKEN and performs live Replicate image/video generation"]
    fn live_replicate_image_and_video_generation_validate_provider_contract() {
        if env::var("REPLICATE_API_TOKEN").is_err() {
            eprintln!("skipping live Replicate test: REPLICATE_API_TOKEN is not set");
            return;
        }

        let provider = create_replicate(ReplicateProviderSettings::new());
        let image = poll_until_ready(
            provider
                .image("black-forest-labs/flux-schnell")
                .do_generate(ImageModelCallOptions::new(1).with_prompt("A small blue cube")),
            Duration::from_secs(180),
        );
        let video = poll_until_ready(
            provider
                .video("minimax/video-01")
                .do_generate(VideoModelCallOptions::new(1).with_prompt("A small blue cube")),
            Duration::from_secs(240),
        );

        assert!(!image.images.is_empty());
        assert!(!video.videos.is_empty());
    }

    #[test]
    fn replicate_image_model_maps_version_wait_header_and_editing_options() {
        let (body, warnings, prefer) = replicate_image_request_body(
            "owner/model:version-123",
            &ImageModelCallOptions::new(1)
                .with_prompt("A portrait")
                .with_files(vec![ImageModelFile::url(
                    Url::parse("https://example.com/input.png").expect("valid URL"),
                )])
                .with_mask(ImageModelFile::file(
                    "image/png",
                    FileDataContent::Bytes(vec![1, 2, 3]),
                ))
                .with_provider_options(replicate_provider_options(json!({
                    "maxWaitTimeInSeconds": 12,
                    "guidance_scale": 7.5,
                    "num_inference_steps": 28,
                    "negative_prompt": "blur",
                    "output_format": "png",
                    "output_quality": 80,
                    "strength": 0.4
                }))),
        )
        .expect("image request body maps");

        assert_eq!(prefer, "wait=12");
        assert!(warnings.is_empty());
        assert_eq!(
            body,
            json!({
                "input": {
                    "prompt": "A portrait",
                    "num_outputs": 1,
                    "image": "https://example.com/input.png",
                    "mask": "data:image/png;base64,AQID",
                    "guidance_scale": 7.5,
                    "num_inference_steps": 28,
                    "negative_prompt": "blur",
                    "output_format": "png",
                    "output_quality": 80,
                    "strength": 0.4
                },
                "version": "version-123"
            })
        );
    }

    #[test]
    fn replicate_image_model_warns_for_flux2_image_limit_and_mask() {
        let files = (0..9)
            .map(|index| {
                ImageModelFile::url(
                    Url::parse(&format!("https://example.com/{index}.png")).expect("valid URL"),
                )
            })
            .collect::<Vec<_>>();
        let (body, warnings, prefer) = replicate_image_request_body(
            "black-forest-labs/flux-2-dev",
            &ImageModelCallOptions::new(1)
                .with_files(files)
                .with_mask(ImageModelFile::url(
                    Url::parse("https://example.com/mask.png").expect("valid URL"),
                )),
        )
        .expect("Flux-2 request body maps");

        assert_eq!(prefer, "wait");
        assert_eq!(warnings.len(), 2);
        assert_eq!(
            body["input"]["input_image"],
            json!("https://example.com/0.png")
        );
        assert_eq!(
            body["input"]["input_image_8"],
            json!("https://example.com/7.png")
        );
        assert!(body["input"].get("input_image_9").is_none());
        assert!(body["input"].get("mask").is_none());
    }

    #[test]
    fn replicate_video_model_maps_prediction_body_provider_options_and_wait_headers() {
        let (body, warnings, prefer, poll) = replicate_video_request_body(
            "minimax/video-01:version-id",
            &VideoModelCallOptions::new(1)
                .with_prompt("A rocket launch")
                .with_aspect_ratio("9:16")
                .with_resolution("720p")
                .with_duration(5.0)
                .with_fps(24.0)
                .with_seed(42)
                .with_image(ai_sdk_rust::VideoModelFile::file(
                    "image/png",
                    FileDataContent::Base64("iVBORw==".to_string()),
                ))
                .with_provider_options(replicate_provider_options(json!({
                    "maxWaitTimeInSeconds": 5,
                    "pollIntervalMs": 11,
                    "pollTimeoutMs": 22,
                    "guidance_scale": 7.5,
                    "num_inference_steps": 20,
                    "motion_bucket_id": 127,
                    "prompt_optimizer": true,
                    "custom": "value"
                }))),
        )
        .expect("video body maps");

        assert_eq!(prefer, "wait=5");
        assert!(warnings.is_empty());
        assert_eq!(poll.poll_interval_millis, Some(11));
        assert_eq!(poll.poll_timeout_millis, Some(22));
        assert_eq!(
            body,
            json!({
                "input": {
                    "prompt": "A rocket launch",
                    "image": "data:image/png;base64,iVBORw==",
                    "aspect_ratio": "9:16",
                    "size": "720p",
                    "duration": 5.0,
                    "fps": 24.0,
                    "seed": 42,
                    "guidance_scale": 7.5,
                    "num_inference_steps": 20,
                    "motion_bucket_id": 127,
                    "prompt_optimizer": true,
                    "custom": "value"
                },
                "version": "version-id"
            })
        );
    }

    #[test]
    fn replicate_video_model_maps_failed_canceled_and_missing_output_predictions() {
        let failed = replicate_video_result_from_prediction(
            "minimax/video-01",
            replicate_prediction_response(&json!({
                "id": "prediction-failed",
                "status": "failed",
                "error": "bad prompt",
                "urls": {"get": "https://api.example.com/prediction-failed"}
            }))
            .expect("prediction parses"),
            None,
            Vec::new(),
            fixed_timestamp(),
        );
        let canceled = replicate_video_result_from_prediction(
            "minimax/video-01",
            replicate_prediction_response(&json!({
                "id": "prediction-canceled",
                "status": "canceled",
                "urls": {"get": "https://api.example.com/prediction-canceled"}
            }))
            .expect("prediction parses"),
            None,
            Vec::new(),
            fixed_timestamp(),
        );
        let missing = replicate_video_result_from_prediction(
            "minimax/video-01",
            replicate_prediction_response(&json!({
                "id": "prediction-empty",
                "status": "succeeded",
                "urls": {"get": "https://api.example.com/prediction-empty"}
            }))
            .expect("prediction parses"),
            None,
            Vec::new(),
            fixed_timestamp(),
        );

        assert_eq!(
            failed
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("replicate"))
                .and_then(|metadata| metadata.get("errorMessage")),
            Some(&json!("Video generation failed: bad prompt"))
        );
        assert_eq!(
            canceled
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("replicate"))
                .and_then(|metadata| metadata.get("errorMessage")),
            Some(&json!("Video generation was canceled"))
        );
        assert_eq!(
            missing
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("replicate"))
                .and_then(|metadata| metadata.get("errorMessage")),
            Some(&json!("No video URL in response"))
        );
    }

    #[test]
    fn replicate_video_model_respects_abort_signal_before_prediction_submit() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: ReplicateTransport = Arc::new(move |request| -> ReplicateTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request);
            Box::pin(ready(Ok(json_response(json!({
                "id": "prediction-123",
                "status": "succeeded",
                "output": "https://replicate.example/video.mp4",
                "urls": {"get": "https://api.example.com/v1/predictions/prediction-123"}
            })))))
        });
        let provider = create_replicate(
            ReplicateProviderSettings::new()
                .with_api_token("test-token")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport);
        let abort_controller = ProviderAbortController::new();
        abort_controller.abort();

        let result = poll_ready(
            provider.video("minimax/video-01").do_generate(
                VideoModelCallOptions::new(1)
                    .with_prompt("Abort")
                    .with_abort_signal(abort_controller.signal()),
            ),
        );

        assert!(result.videos.is_empty());
        assert_eq!(
            requests
                .lock()
                .expect("request list mutex is not poisoned")
                .len(),
            0
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("replicate"))
                .and_then(|metadata| metadata.get("errorMessage")),
            Some(&json!("Aborted"))
        );
    }

    fn replicate_image_transport() -> (Arc<Mutex<Vec<ProviderApiRequest>>>, ReplicateTransport) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: ReplicateTransport = Arc::new(move |request| -> ReplicateTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());

            let response = match (request.method, request.url.as_str()) {
                (
                    ProviderApiRequestMethod::Post,
                    "https://api.example.com/v1/models/black-forest-labs/flux-2-dev/predictions",
                ) => json_response(json!({
                    "output": [
                        "https://replicate.example/image-a.png",
                        "https://replicate.example/image-b.png"
                    ]
                }))
                .with_headers(
                    [("x-request-id".to_string(), "pred-1".to_string())]
                        .into_iter()
                        .collect(),
                ),
                (ProviderApiRequestMethod::Get, "https://replicate.example/image-a.png") => {
                    ProviderApiResponse::bytes(200, "OK", vec![1, 2, 3])
                }
                (ProviderApiRequestMethod::Get, "https://replicate.example/image-b.png") => {
                    ProviderApiResponse::bytes(200, "OK", vec![4, 5, 6])
                }
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"detail": "unexpected request"}).to_string(),
                ),
            };

            Box::pin(ready(Ok(response)))
        });

        (requests, transport)
    }

    #[test]
    fn replicate_image_model_maps_flux2_request_downloads_and_warnings() {
        let (requests, transport) = replicate_image_transport();
        let provider = create_replicate(
            ReplicateProviderSettings::new()
                .with_api_token("test-token")
                .with_base_url("https://api.example.com/v1")
                .with_header("x-provider-header", "provider"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "replicate".to_string(),
            serde_json::from_value(json!({
                "maxWaitTimeInSeconds": 12,
                "output_format": "webp",
                "custom": "value"
            }))
            .expect("provider options deserialize"),
        );

        let result = poll_ready(
            provider.image("black-forest-labs/flux-2-dev").do_generate(
                ImageModelCallOptions::new(2)
                    .with_prompt("A city at night")
                    .with_aspect_ratio("16:9")
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

        assert_eq!(
            result.images,
            vec![
                FileDataContent::Bytes(vec![1, 2, 3]),
                FileDataContent::Bytes(vec![4, 5, 6])
            ]
        );
        assert_eq!(result.response.model_id, "black-forest-labs/flux-2-dev");
        assert_eq!(result.response.timestamp, fixed_timestamp());

        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method, ProviderApiRequestMethod::Post);
        assert_eq!(
            requests[0].headers.get("authorization"),
            Some(&"Bearer test-token".to_string())
        );
        assert_eq!(
            requests[0].headers.get("prefer"),
            Some(&"wait=12".to_string())
        );
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "input": {
                    "prompt": "A city at night",
                    "aspect_ratio": "16:9",
                    "num_outputs": 2,
                    "input_image": "https://example.com/a.png",
                    "input_image_2": "data:image/png;base64,iVBORw==",
                    "output_format": "webp",
                    "custom": "value"
                }
            })
        );
        assert!(result.warnings.is_empty());
        assert!(!requests[1].headers.contains_key("Authorization"));
    }

    #[test]
    fn replicate_video_model_maps_prediction_response_and_metadata() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: ReplicateTransport = Arc::new(move |request| -> ReplicateTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());

            Box::pin(ready(Ok(json_response(json!({
                "id": "prediction-123",
                "status": "succeeded",
                "output": "https://replicate.example/video.mp4",
                "urls": { "get": "https://api.example.com/v1/predictions/prediction-123" },
                "metrics": { "predict_time": 1.25 }
            }))
            .with_headers(
                [("x-prediction-id".to_string(), "prediction-123".to_string())]
                    .into_iter()
                    .collect(),
            ))))
        });
        let provider = create_replicate(
            ReplicateProviderSettings::new()
                .with_api_token("test-token")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "replicate".to_string(),
            serde_json::from_value(json!({
                "maxWaitTimeInSeconds": 5,
                "guidance_scale": 7.5,
                "prompt_optimizer": true
            }))
            .expect("provider options deserialize"),
        );

        let result = poll_ready(
            provider.video("minimax/video-01:version-id").do_generate(
                VideoModelCallOptions::new(1)
                    .with_prompt("A rocket launch")
                    .with_resolution("720p")
                    .with_seed(42)
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.videos.len(), 1);
        assert_eq!(result.response.model_id, "minimax/video-01:version-id");
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("replicate"))
                .and_then(|metadata| metadata.get("predictionId")),
            Some(&json!("prediction-123"))
        );

        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(requests[0].url, "https://api.example.com/v1/predictions");
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "input": {
                    "prompt": "A rocket launch",
                    "size": "720p",
                    "seed": 42,
                    "guidance_scale": 7.5,
                    "prompt_optimizer": true
                },
                "version": "version-id"
            })
        );
    }

    #[test]
    fn replicate_image_model_maps_api_errors_to_metadata() {
        let transport: ReplicateTransport = Arc::new(move |_request| -> ReplicateTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                400,
                "Bad Request",
                json!({"detail": "invalid prompt"}).to_string(),
            ))))
        });
        let provider = create_replicate(
            ReplicateProviderSettings::new()
                .with_api_token("test-token")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider
                .image("black-forest-labs/flux-schnell")
                .do_generate(ImageModelCallOptions::new(1).with_prompt("bad")),
        );

        assert!(result.images.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("replicate"))
                .and_then(|metadata| metadata.extra.get("errorMessage")),
            Some(&json!("invalid prompt"))
        );
    }

    #[test]
    fn replicate_image_model_warns_when_extra_non_flux_images_are_ignored() {
        let (requests, transport) = replicate_image_transport();
        let provider = create_replicate(
            ReplicateProviderSettings::new()
                .with_api_token("test-token")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport);

        let result = poll_ready(
            provider
                .image("black-forest-labs/flux-schnell")
                .do_generate(
                    ImageModelCallOptions::new(1)
                        .with_prompt("A portrait")
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

        assert_eq!(
            result.warnings,
            vec![Warning::Other {
                message:
                    "This Replicate model only supports a single input image. Additional images are ignored."
                        .to_string()
            }]
        );
        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(
            request_body_json(&requests[0])["input"]["image"],
            json!("https://example.com/a.png")
        );
    }
}
