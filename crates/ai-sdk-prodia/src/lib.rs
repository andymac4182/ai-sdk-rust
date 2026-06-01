use std::collections::BTreeMap;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use ai_sdk_rust::{
    FetchErrorInfo, GetFromApiOptions, HandledFetchError, Headers, ImageModel,
    ImageModelCallOptions, ImageModelProviderMetadata, ImageModelProviderMetadataEntry,
    ImageModelResponse, ImageModelResult, JsonObject, JsonValue, LoadApiKeyError,
    LoadApiKeyOptions, ModelType, NoSuchModelError, OpenAICompatibleChatLanguageModel,
    OpenAICompatibleEmbeddingModel, PostToApiOptions, Provider, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, ProviderMetadata, ProviderWithVideoModel,
    ResponseHandlerResult, RuntimeEnvironment, VideoModel, VideoModelCallOptions, VideoModelFile,
    VideoModelResponse, VideoModelResult, VideoModelVideoData, Warning, combine_headers,
    convert_base64_to_bytes, create_binary_response_handler, create_json_error_response_handler,
    get_from_api, load_api_key, parse_provider_options, post_to_api, with_user_agent_suffix,
    without_trailing_slash,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Default base URL for upstream `@ai-sdk/prodia` API calls.
pub const DEFAULT_PRODIA_BASE_URL: &str = "https://inference.prodia.com/v2";

/// Settings for the upstream Prodia provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProdiaProviderSettings {
    /// Prodia API key. When omitted, `PRODIA_TOKEN` is read at request time.
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

impl ProdiaProviderSettings {
    /// Creates empty Prodia provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Prodia API key.
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

/// Upstream Prodia provider foundation.
#[derive(Clone)]
pub struct ProdiaProvider {
    base_url: String,
    settings: ProdiaProviderSettings,
    transport: ProdiaTransport,
    current_date: ProdiaDateProvider,
}

/// Prodia image model.
#[derive(Clone)]
pub struct ProdiaImageModel {
    model_id: String,
    base_url: String,
    settings: ProdiaProviderSettings,
    transport: ProdiaTransport,
    current_date: ProdiaDateProvider,
}

/// Prodia video model.
#[derive(Clone)]
pub struct ProdiaVideoModel {
    model_id: String,
    base_url: String,
    settings: ProdiaProviderSettings,
    transport: ProdiaTransport,
    current_date: ProdiaDateProvider,
}

/// Future returned by an injected Prodia HTTP transport.
pub type ProdiaTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by Prodia provider models.
pub type ProdiaTransport = Arc<dyn Fn(ProviderApiRequest) -> ProdiaTransportFuture + Send + Sync>;

type ProdiaDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type ProdiaImageGenerateFuture<'a> = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>;
type ProdiaVideoMaxVideosFuture<'a> = Ready<Option<usize>>;
type ProdiaVideoGenerateFuture<'a> = Pin<Box<dyn Future<Output = VideoModelResult> + Send + 'a>>;

impl ProdiaProvider {
    /// Creates a Prodia provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(ProdiaProviderSettings::new())
    }

    /// Creates a provider from explicit Prodia settings.
    pub fn from_settings(settings: ProdiaProviderSettings) -> Self {
        let base_url = without_trailing_slash(
            settings
                .base_url
                .as_deref()
                .or(Some(DEFAULT_PRODIA_BASE_URL)),
        )
        .expect("default Prodia base URL is present")
        .to_string();

        Self {
            base_url,
            settings,
            transport: default_prodia_transport(),
            current_date: Arc::new(OffsetDateTime::now_utc),
        }
    }

    /// Sets the Prodia API key for this provider.
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
    pub fn with_transport(mut self, transport: ProdiaTransport) -> Self {
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
    pub fn image(&self, model_id: impl Into<String>) -> ProdiaImageModel {
        self.image_model(model_id)
            .expect("Prodia image models are supported")
    }

    /// Creates an image model.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<ProdiaImageModel, NoSuchModelError> {
        Ok(ProdiaImageModel::new(
            model_id,
            self.base_url.clone(),
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Creates a video model.
    pub fn video(&self, model_id: impl Into<String>) -> ProdiaVideoModel {
        self.video_model(model_id)
            .expect("Prodia video models are supported")
    }

    /// Creates a video model.
    pub fn video_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<ProdiaVideoModel, NoSuchModelError> {
        Ok(ProdiaVideoModel::new(
            model_id,
            self.base_url.clone(),
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Reports that the Prodia language surface is outside this media-generation slice.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::with_message(
            model_id,
            ModelType::LanguageModel,
            "Prodia language models are not ported in the media generation provider slice",
        ))
    }

    /// Reports that Prodia does not expose embedding models through this provider.
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

impl Default for ProdiaProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for ProdiaProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = ProdiaImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        ProdiaProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        ProdiaProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        ProdiaProvider::image_model(self, model_id)
    }
}

impl ProviderWithVideoModel for ProdiaProvider {
    type VideoModel = ProdiaVideoModel;

    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
        ProdiaProvider::video_model(self, model_id)
    }
}

impl ProdiaImageModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: ProdiaProviderSettings,
        transport: ProdiaTransport,
        current_date: ProdiaDateProvider,
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
        "prodia.image"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: ProdiaTransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_generate_result(&self, options: ImageModelCallOptions) -> ImageModelResult {
        let timestamp = (self.current_date)();
        let (request_body, warnings) = match prodia_image_request_body(&self.model_id, &options) {
            Ok(args) => args,
            Err(error) => {
                return prodia_image_result_from_error(
                    &self.model_id,
                    error,
                    None,
                    Vec::new(),
                    timestamp,
                );
            }
        };
        let request_headers = match self
            .request_headers(options.headers.as_ref(), "multipart/form-data; image/png")
        {
            Ok(headers) => headers,
            Err(error) => {
                return prodia_image_result_from_error(
                    &self.model_id,
                    error.to_string(),
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let transport = Arc::clone(&self.transport);
        let result = match post_to_api(
            PostToApiOptions::new(
                self.job_url(),
                ProviderApiRequestBody::text(request_body.to_string()),
                request_body,
            )
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown()),
            move |request| (transport)(request),
            |request, response| prodia_image_multipart_response(request, response),
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    prodia_error_response,
                    prodia_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                let (message, headers) = prodia_handled_error_parts(error);
                return prodia_image_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    warnings,
                    timestamp,
                );
            }
        };

        let mut image_result = ImageModelResult::new(
            vec![ai_sdk_rust::FileDataContent::Bytes(
                result.value.image_bytes,
            )],
            prodia_image_response_metadata(&self.model_id, result.response_headers, timestamp),
        )
        .with_provider_metadata(ImageModelProviderMetadata::from([(
            "prodia".to_string(),
            ImageModelProviderMetadataEntry::new(vec![JsonValue::Object(
                prodia_provider_metadata(result.value.job_result),
            )]),
        )]));

        for warning in warnings {
            image_result = image_result.with_warning(warning);
        }

        image_result
    }

    fn job_url(&self) -> String {
        format!("{}/job?price=true", self.base_url)
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
        accept: &str,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(prodia_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
            Some(vec![
                ("Accept".to_string(), Some(accept.to_string())),
                (
                    "Content-Type".to_string(),
                    Some("application/json".to_string()),
                ),
            ]),
        ]))
    }
}

impl ImageModel for ProdiaImageModel {
    type MaxImagesPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = ProdiaImageGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        ProdiaImageModel::provider(self)
    }

    fn model_id(&self) -> &str {
        ProdiaImageModel::model_id(self)
    }

    fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
        ready(Some(1))
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

impl ProdiaVideoModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: ProdiaProviderSettings,
        transport: ProdiaTransport,
        current_date: ProdiaDateProvider,
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
        "prodia.video"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: ProdiaTransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_generate_result(&self, options: VideoModelCallOptions) -> VideoModelResult {
        let timestamp = (self.current_date)();
        let (request_body, warnings) = match prodia_video_request_body(&self.model_id, &options) {
            Ok(args) => args,
            Err(error) => {
                return prodia_video_result_from_error(
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
                return prodia_video_result_from_error(
                    &self.model_id,
                    error.to_string(),
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let post_options = if let Some(image) = options.image.as_ref() {
            match self
                .multipart_video_post_options(request_body.clone(), image, request_headers)
                .await
            {
                Ok(options) => options,
                Err(error) => {
                    return prodia_video_result_from_error(
                        &self.model_id,
                        error,
                        None,
                        warnings,
                        timestamp,
                    );
                }
            }
        } else {
            PostToApiOptions::new(
                self.job_url(),
                ProviderApiRequestBody::text(request_body.to_string()),
                request_body,
            )
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
        };
        let transport = Arc::clone(&self.transport);
        let result = match post_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| prodia_video_multipart_response(request, response),
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    prodia_error_response,
                    prodia_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(result) => result,
            Err(error) => {
                let (message, headers) = prodia_handled_error_parts(error);
                return prodia_video_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    warnings,
                    timestamp,
                );
            }
        };

        let mut video_result = VideoModelResult::new(
            vec![VideoModelVideoData::binary(
                result.value.video_bytes,
                result.value.video_media_type,
            )],
            prodia_video_response_metadata(&self.model_id, result.response_headers, timestamp),
        )
        .with_provider_metadata(ProviderMetadata::from([(
            "prodia".to_string(),
            JsonObject::from_iter([(
                "videos".to_string(),
                JsonValue::Array(vec![JsonValue::Object(prodia_provider_metadata(
                    result.value.job_result,
                ))]),
            )]),
        )]));

        for warning in warnings {
            video_result = video_result.with_warning(warning);
        }

        video_result
    }

    async fn multipart_video_post_options(
        &self,
        request_body: JsonValue,
        image: &VideoModelFile,
        mut request_headers: BTreeMap<String, Option<String>>,
    ) -> Result<PostToApiOptions, String> {
        let image_data = self.resolve_video_file_data(image).await?;
        let boundary = "ai-sdk-rust-prodia-boundary";
        let body = encode_prodia_video_multipart(boundary, &request_body, &image_data);
        request_headers.insert(
            "Content-Type".to_string(),
            Some(format!("multipart/form-data; boundary={boundary}")),
        );

        Ok(PostToApiOptions::new(
            self.job_url(),
            ProviderApiRequestBody::bytes(body),
            request_body,
        )
        .with_headers(request_headers)
        .with_environment(RuntimeEnvironment::unknown()))
    }

    async fn resolve_video_file_data(
        &self,
        file: &VideoModelFile,
    ) -> Result<ProdiaInputFileData, String> {
        match file {
            VideoModelFile::File {
                media_type, data, ..
            } => Ok(ProdiaInputFileData {
                bytes: match data {
                    ai_sdk_rust::FileDataContent::Bytes(bytes) => bytes.clone(),
                    ai_sdk_rust::FileDataContent::Base64(base64) => {
                        convert_base64_to_bytes(base64).map_err(|error| error.to_string())?
                    }
                },
                media_type: media_type.clone(),
            }),
            VideoModelFile::Url { url, .. } => {
                let transport = Arc::clone(&self.transport);
                let response = get_from_api(
                    GetFromApiOptions::new(url.as_str())
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
                            prodia_error_response,
                            prodia_error_message,
                            |_, _| None,
                        ))
                    },
                )
                .await
                .map_err(|error| prodia_handled_error_parts(error).0)?;
                let media_type = response
                    .response_headers
                    .as_ref()
                    .and_then(|headers| {
                        headers
                            .get("content-type")
                            .or_else(|| headers.get("Content-Type"))
                            .cloned()
                    })
                    .unwrap_or_else(|| "application/octet-stream".to_string());
                Ok(ProdiaInputFileData {
                    bytes: response.value,
                    media_type,
                })
            }
        }
    }

    fn job_url(&self) -> String {
        format!("{}/job?price=true", self.base_url)
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(prodia_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
            Some(vec![
                (
                    "Accept".to_string(),
                    Some("multipart/form-data; video/mp4".to_string()),
                ),
                (
                    "Content-Type".to_string(),
                    Some("application/json".to_string()),
                ),
            ]),
        ]))
    }
}

impl VideoModel for ProdiaVideoModel {
    type MaxVideosPerCallFuture<'a>
        = ProdiaVideoMaxVideosFuture<'a>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = ProdiaVideoGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        ProdiaVideoModel::provider(self)
    }

    fn model_id(&self) -> &str {
        ProdiaVideoModel::model_id(self)
    }

    fn max_videos_per_call(&self) -> Self::MaxVideosPerCallFuture<'_> {
        ready(Some(1))
    }

    fn do_generate(&self, options: VideoModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates a Prodia provider with explicit settings.
pub fn create_prodia(settings: ProdiaProviderSettings) -> ProdiaProvider {
    ProdiaProvider::from_settings(settings)
}

/// Creates a Prodia provider with default settings.
pub fn prodia() -> ProdiaProvider {
    ProdiaProvider::new()
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProdiaImageModelOptions {
    #[serde(default)]
    steps: Option<u64>,
    #[serde(default)]
    width: Option<u64>,
    #[serde(default)]
    height: Option<u64>,
    #[serde(default)]
    style_preset: Option<String>,
    #[serde(default)]
    loras: Option<Vec<String>>,
    #[serde(default)]
    progressive: Option<bool>,
}

impl ProdiaImageModelOptions {
    fn validate(&self) -> Result<(), &'static str> {
        if self.steps.is_some_and(|value| !(1..=4).contains(&value)) {
            return Err("steps must be between 1 and 4");
        }
        if self
            .width
            .is_some_and(|value| !(256..=1920).contains(&value))
        {
            return Err("width must be between 256 and 1920");
        }
        if self
            .height
            .is_some_and(|value| !(256..=1920).contains(&value))
        {
            return Err("height must be between 256 and 1920");
        }
        if self.loras.as_ref().is_some_and(|loras| loras.len() > 3) {
            return Err("loras must contain at most 3 entries");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProdiaVideoModelOptions {
    #[serde(default)]
    resolution: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct ProdiaJobResult {
    id: String,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    config: Option<ProdiaJobConfig>,
    #[serde(default)]
    metrics: Option<ProdiaJobMetrics>,
    #[serde(default)]
    price: Option<ProdiaJobPrice>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct ProdiaJobConfig {
    #[serde(default)]
    seed: Option<u64>,
    #[serde(flatten)]
    _extra: JsonObject,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct ProdiaJobMetrics {
    #[serde(default)]
    elapsed: Option<f64>,
    #[serde(default)]
    ips: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct ProdiaJobPrice {
    #[serde(default)]
    product: Option<String>,
    #[serde(default)]
    dollars: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct ProdiaErrorResponse {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    detail: Option<JsonValue>,
    #[serde(default)]
    error: Option<String>,
}

struct ProdiaImageMultipartResult {
    job_result: ProdiaJobResult,
    image_bytes: Vec<u8>,
}

struct ProdiaVideoMultipartResult {
    job_result: ProdiaJobResult,
    video_bytes: Vec<u8>,
    video_media_type: String,
}

struct ProdiaInputFileData {
    bytes: Vec<u8>,
    media_type: String,
}

struct MultipartPart {
    headers: Headers,
    body: Vec<u8>,
}

fn prodia_image_request_body(
    model_id: &str,
    options: &ImageModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>), String> {
    let mut warnings = Vec::new();
    let provider_options = parse_provider_options(
        "prodia",
        Some(&options.provider_options),
        prodia_image_model_options,
    )
    .map_err(|error| error.to_string())?
    .unwrap_or_default();
    let mut config = JsonObject::new();
    insert_option_string_ref(&mut config, "prompt", options.prompt.as_ref());
    if let Some(size) = options.size.as_deref() {
        match parse_size(size) {
            Some((width, height)) => {
                config.insert(
                    "width".to_string(),
                    JsonValue::from(provider_options.width.unwrap_or(width)),
                );
                config.insert(
                    "height".to_string(),
                    JsonValue::from(provider_options.height.unwrap_or(height)),
                );
            }
            None => warnings.push(Warning::Unsupported {
                feature: "size".to_string(),
                details: Some(format!(
                    "Invalid size format: {size}. Expected format: WIDTHxHEIGHT (e.g., 1024x1024)"
                )),
            }),
        }
    } else {
        insert_option_u64(&mut config, "width", provider_options.width);
        insert_option_u64(&mut config, "height", provider_options.height);
    }
    insert_option_u64(&mut config, "seed", options.seed);
    insert_option_u64(&mut config, "steps", provider_options.steps);
    insert_option_string(&mut config, "style_preset", provider_options.style_preset);
    if let Some(loras) = provider_options.loras {
        config.insert(
            "loras".to_string(),
            JsonValue::Array(loras.into_iter().map(JsonValue::String).collect()),
        );
    }
    insert_option_bool(&mut config, "progressive", provider_options.progressive);

    let mut body = JsonObject::new();
    body.insert("type".to_string(), JsonValue::String(model_id.to_string()));
    body.insert("config".to_string(), JsonValue::Object(config));
    Ok((JsonValue::Object(body), warnings))
}

fn prodia_video_request_body(
    model_id: &str,
    options: &VideoModelCallOptions,
) -> Result<(JsonValue, Vec<Warning>), String> {
    let warnings = Vec::new();
    let provider_options = parse_provider_options(
        "prodia",
        Some(&options.provider_options),
        prodia_video_model_options,
    )
    .map_err(|error| error.to_string())?
    .unwrap_or_default();
    let mut config = JsonObject::new();
    insert_option_string_ref(&mut config, "prompt", options.prompt.as_ref());
    insert_option_u64(&mut config, "seed", options.seed);
    insert_option_string(&mut config, "resolution", provider_options.resolution);

    let mut body = JsonObject::new();
    body.insert("type".to_string(), JsonValue::String(model_id.to_string()));
    body.insert("config".to_string(), JsonValue::Object(config));
    Ok((JsonValue::Object(body), warnings))
}

fn prodia_image_model_options(value: &JsonValue) -> Result<ProdiaImageModelOptions, String> {
    let options = serde_json::from_value::<ProdiaImageModelOptions>(value.clone())
        .map_err(|error| error.to_string())?;
    options.validate().map_err(str::to_string)?;
    Ok(options)
}

fn prodia_video_model_options(value: &JsonValue) -> Result<ProdiaVideoModelOptions, String> {
    serde_json::from_value::<ProdiaVideoModelOptions>(value.clone())
        .map_err(|error| error.to_string())
}

fn parse_size(size: &str) -> Option<(u64, u64)> {
    let (width, height) = size.split_once('x')?;
    Some((width.parse().ok()?, height.parse().ok()?))
}

fn prodia_image_multipart_response(
    request: &ProviderApiRequest,
    response: &ProviderApiResponse,
) -> Result<ResponseHandlerResult<ProdiaImageMultipartResult>, ProviderApiResponseHandlerError> {
    let _ = request;
    let content_type = response
        .headers
        .get("content-type")
        .or_else(|| response.headers.get("Content-Type"))
        .cloned()
        .unwrap_or_default();
    let boundary = multipart_boundary(&content_type).ok_or_else(|| {
        ProviderApiResponseHandlerError::Other {
            message: format!(
                "Prodia response missing multipart boundary in content-type: {content_type}"
            ),
        }
    })?;
    let body = response
        .body
        .as_ref()
        .map(|body| match body {
            ai_sdk_rust::ProviderApiResponseBody::Text { content } => content.as_bytes().to_vec(),
            ai_sdk_rust::ProviderApiResponseBody::Bytes { content } => content.clone(),
        })
        .ok_or_else(|| ProviderApiResponseHandlerError::Other {
            message: "Prodia response body is empty".to_string(),
        })?;
    let parts = parse_multipart(&body, &boundary);
    let (job_result, output, _) = prodia_parts(parts, "image")?;

    Ok(ResponseHandlerResult::new(ProdiaImageMultipartResult {
        job_result,
        image_bytes: output,
    })
    .with_response_headers(response.headers.clone()))
}

fn prodia_video_multipart_response(
    request: &ProviderApiRequest,
    response: &ProviderApiResponse,
) -> Result<ResponseHandlerResult<ProdiaVideoMultipartResult>, ProviderApiResponseHandlerError> {
    let _ = request;
    let content_type = response
        .headers
        .get("content-type")
        .or_else(|| response.headers.get("Content-Type"))
        .cloned()
        .unwrap_or_default();
    let boundary = multipart_boundary(&content_type).ok_or_else(|| {
        ProviderApiResponseHandlerError::Other {
            message: format!(
                "Prodia response missing multipart boundary in content-type: {content_type}"
            ),
        }
    })?;
    let body = response
        .body
        .as_ref()
        .map(|body| match body {
            ai_sdk_rust::ProviderApiResponseBody::Text { content } => content.as_bytes().to_vec(),
            ai_sdk_rust::ProviderApiResponseBody::Bytes { content } => content.clone(),
        })
        .ok_or_else(|| ProviderApiResponseHandlerError::Other {
            message: "Prodia response body is empty".to_string(),
        })?;
    let parts = parse_multipart(&body, &boundary);
    let (job_result, output, media_type) = prodia_parts(parts, "video")?;

    Ok(ResponseHandlerResult::new(ProdiaVideoMultipartResult {
        job_result,
        video_bytes: output,
        video_media_type: media_type.unwrap_or_else(|| "video/mp4".to_string()),
    })
    .with_response_headers(response.headers.clone()))
}

fn prodia_parts(
    parts: Vec<MultipartPart>,
    media_prefix: &str,
) -> Result<(ProdiaJobResult, Vec<u8>, Option<String>), ProviderApiResponseHandlerError> {
    let mut job_result = None;
    let mut output = None;
    let mut output_media_type = None;

    for part in parts {
        let content_disposition = part
            .headers
            .get("content-disposition")
            .cloned()
            .unwrap_or_default();
        let content_type = part
            .headers
            .get("content-type")
            .cloned()
            .unwrap_or_default();
        if content_disposition.contains("name=\"job\"") {
            job_result = Some(
                serde_json::from_slice::<ProdiaJobResult>(&part.body).map_err(|error| {
                    ProviderApiResponseHandlerError::Other {
                        message: error.to_string(),
                    }
                })?,
            );
        } else if content_disposition.contains("name=\"output\"")
            || content_type.starts_with(media_prefix)
        {
            output = Some(part.body);
            if content_type.starts_with(media_prefix) {
                output_media_type = Some(content_type);
            }
        }
    }

    let job_result = job_result.ok_or_else(|| ProviderApiResponseHandlerError::Other {
        message: "Prodia multipart response missing job part".to_string(),
    })?;
    let output = output.ok_or_else(|| ProviderApiResponseHandlerError::Other {
        message: format!("Prodia multipart response missing output {media_prefix}"),
    })?;

    Ok((job_result, output, output_media_type))
}

fn multipart_boundary(content_type: &str) -> Option<String> {
    content_type
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix("boundary=").map(str::to_string))
}

fn parse_multipart(data: &[u8], boundary: &str) -> Vec<MultipartPart> {
    let text = String::from_utf8_lossy(data);
    let marker = format!("--{boundary}");
    let mut parts = Vec::new();

    for section in text.split(&marker).skip(1) {
        let section = section.trim_start_matches(['\r', '\n']);
        if section.starts_with("--") {
            break;
        }
        let Some((headers, body)) = section
            .split_once("\r\n\r\n")
            .or_else(|| section.split_once("\n\n"))
        else {
            continue;
        };
        let mut parsed_headers = Headers::new();
        for line in headers.lines() {
            if let Some((name, value)) = line.split_once(':') {
                parsed_headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
            }
        }
        let body = body
            .trim_end_matches('\n')
            .trim_end_matches('\r')
            .as_bytes()
            .to_vec();
        parts.push(MultipartPart {
            headers: parsed_headers,
            body,
        });
    }

    parts
}

fn encode_prodia_video_multipart(
    boundary: &str,
    body: &JsonValue,
    image_data: &ProdiaInputFileData,
) -> Vec<u8> {
    let extension = media_type_extension(&image_data.media_type);
    let mut output = Vec::new();
    output.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    output.extend_from_slice(
        b"Content-Disposition: form-data; name=\"job\"; filename=\"job.json\"\r\nContent-Type: application/json\r\n\r\n",
    );
    output.extend_from_slice(body.to_string().as_bytes());
    output.extend_from_slice(b"\r\n");
    output.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    output.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"input\"; filename=\"input{extension}\"\r\nContent-Type: {}\r\n\r\n",
            image_data.media_type
        )
        .as_bytes(),
    );
    output.extend_from_slice(&image_data.bytes);
    output.extend_from_slice(b"\r\n");
    output.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    output
}

fn media_type_extension(media_type: &str) -> &'static str {
    match media_type {
        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        "image/webp" => ".webp",
        "video/mp4" => ".mp4",
        "video/webm" => ".webm",
        _ => "",
    }
}

fn prodia_provider_metadata(job_result: ProdiaJobResult) -> JsonObject {
    let mut metadata = JsonObject::new();
    metadata.insert("jobId".to_string(), JsonValue::String(job_result.id));
    if let Some(seed) = job_result.config.and_then(|config| config.seed) {
        metadata.insert("seed".to_string(), JsonValue::from(seed));
    }
    if let Some(metrics) = job_result.metrics {
        insert_option_f64(&mut metadata, "elapsed", metrics.elapsed);
        insert_option_f64(&mut metadata, "iterationsPerSecond", metrics.ips);
    }
    insert_option_string(&mut metadata, "createdAt", job_result.created_at);
    insert_option_string(&mut metadata, "updatedAt", job_result.updated_at);
    if let Some(price) = job_result.price {
        insert_option_f64(&mut metadata, "dollars", price.dollars);
    }
    metadata
}

fn prodia_provider_header_entries(
    settings: &ProdiaProviderSettings,
) -> Result<Vec<(String, Option<String>)>, LoadApiKeyError> {
    let mut headers = vec![(
        "Authorization".to_string(),
        Some(format!(
            "Bearer {}",
            prodia_api_key(settings.api_key.as_ref())?
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
        [format!("ai-sdk/prodia/{}", ai_sdk_rust::VERSION)],
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

fn prodia_api_key(explicit_api_key: Option<&String>) -> Result<String, LoadApiKeyError> {
    let mut options = LoadApiKeyOptions::new("PRODIA_TOKEN", "Prodia");
    if let Some(api_key) = explicit_api_key {
        options = options.with_api_key(api_key.clone());
    }
    load_api_key(options)
}

fn prodia_error_response(value: &JsonValue) -> Result<ProdiaErrorResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn prodia_error_message(error: &ProdiaErrorResponse) -> String {
    if let Some(detail) = error.detail.as_ref() {
        if let Some(detail) = detail.as_str() {
            return detail.to_string();
        }
        if !detail.is_null() {
            return serde_json::to_string(detail)
                .unwrap_or_else(|_| "Unknown Prodia error".to_string());
        }
    }
    error
        .error
        .clone()
        .or_else(|| error.message.clone())
        .unwrap_or_else(|| "Unknown Prodia error".to_string())
}

fn prodia_handled_error_parts(error: HandledFetchError) -> (String, Option<Headers>) {
    match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
        ),
    }
}

fn prodia_image_result_from_error(
    model_id: &str,
    message: String,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> ImageModelResult {
    let mut result = ImageModelResult::new(
        Vec::new(),
        prodia_image_response_metadata(model_id, headers, timestamp),
    )
    .with_provider_metadata(ImageModelProviderMetadata::from([(
        "prodia".to_string(),
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

fn prodia_video_result_from_error(
    model_id: &str,
    message: String,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> VideoModelResult {
    let mut result = VideoModelResult::new(
        Vec::new(),
        prodia_video_response_metadata(model_id, headers, timestamp),
    )
    .with_provider_metadata(ProviderMetadata::from([(
        "prodia".to_string(),
        object_with_string("errorMessage", message),
    )]));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn prodia_image_response_metadata(
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

fn prodia_video_response_metadata(
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

fn object_with_string(name: &str, value: impl Into<String>) -> JsonObject {
    let mut object = JsonObject::new();
    object.insert(name.to_string(), JsonValue::String(value.into()));
    object
}

fn default_prodia_transport() -> ProdiaTransport {
    Arc::new(|request| Box::pin(ready(execute_prodia_request(request))))
}

fn execute_prodia_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_prodia_get_request(request),
        ProviderApiRequestMethod::Post => execute_prodia_post_request(request),
    }
}

fn execute_prodia_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    prodia_provider_api_response(builder.config().http_status_as_error(false).build().call())
}

fn execute_prodia_post_request(
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
                "multipart form data body encoding is not supported by the Prodia transport",
            ));
        }
        None => builder.send_empty(),
    };
    prodia_provider_api_response(response)
}

fn prodia_provider_api_response(
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
    use super::{ProdiaProviderSettings, ProdiaTransport, ProdiaTransportFuture, create_prodia};
    use ai_sdk_rust::{
        FileDataContent, ImageModel, ImageModelCallOptions, ProviderApiRequest,
        ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse, ProviderOptions,
        VideoModel, VideoModelCallOptions, VideoModelFile,
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

    fn multipart_response(
        boundary: &str,
        media_type: &str,
        media_bytes: &[u8],
    ) -> ProviderApiResponse {
        let job = json!({
            "id": "job-123",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:01Z",
            "config": { "seed": 42 },
            "metrics": { "elapsed": 1.5, "ips": 2.0 },
            "price": { "product": "test", "dollars": 0.01 }
        })
        .to_string();
        let mut body = Vec::new();
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(b"Content-Disposition: form-data; name=\"job\"\r\nContent-Type: application/json\r\n\r\n");
        body.extend_from_slice(job.as_bytes());
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(format!("Content-Disposition: form-data; name=\"output\"\r\nContent-Type: {media_type}\r\n\r\n").as_bytes());
        body.extend_from_slice(media_bytes);
        body.extend_from_slice(b"\r\n");
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        ProviderApiResponse::bytes(200, "OK", body).with_headers(
            [(
                "content-type".to_string(),
                format!("multipart/form-data; boundary={boundary}"),
            )]
            .into_iter()
            .collect(),
        )
    }

    fn request_body_json(request: &ProviderApiRequest) -> serde_json::Value {
        let Some(ProviderApiRequestBody::Text { content }) = request.body.as_ref() else {
            panic!("expected text request body");
        };
        serde_json::from_str(content).expect("request body is valid JSON")
    }

    #[test]
    fn prodia_image_model_maps_json_request_multipart_response_and_metadata() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: ProdiaTransport = Arc::new(move |request| -> ProdiaTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());
            Box::pin(ready(Ok(multipart_response(
                "boundary",
                "image/png",
                &[1, 2, 3],
            ))))
        });
        let provider = create_prodia(
            ProdiaProviderSettings::new()
                .with_api_key("test-token")
                .with_base_url("https://api.example.com/v2"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "prodia".to_string(),
            serde_json::from_value(json!({
                "steps": 4,
                "width": 1024,
                "height": 768,
                "stylePreset": "anime",
                "loras": ["a", "b"],
                "progressive": true
            }))
            .expect("provider options deserialize"),
        );

        let result = poll_ready(
            provider.image("inference.sd3.txt2img.v2").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A castle")
                    .with_seed(42)
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.images, vec![FileDataContent::Bytes(vec![1, 2, 3])]);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("prodia"))
                .and_then(|metadata| metadata.images.first())
                .and_then(|image| image.get("jobId")),
            Some(&json!("job-123"))
        );
        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(requests[0].url, "https://api.example.com/v2/job?price=true");
        assert_eq!(
            requests[0].headers.get("authorization"),
            Some(&"Bearer test-token".to_string())
        );
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "type": "inference.sd3.txt2img.v2",
                "config": {
                    "prompt": "A castle",
                    "seed": 42,
                    "steps": 4,
                    "width": 1024,
                    "height": 768,
                    "style_preset": "anime",
                    "loras": ["a", "b"],
                    "progressive": true
                }
            })
        );
    }

    #[test]
    fn prodia_video_model_maps_json_request_and_binary_video_response() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: ProdiaTransport = Arc::new(move |request| -> ProdiaTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());
            Box::pin(ready(Ok(multipart_response(
                "boundary",
                "video/mp4",
                &[7, 8, 9],
            ))))
        });
        let provider = create_prodia(
            ProdiaProviderSettings::new()
                .with_api_key("test-token")
                .with_base_url("https://api.example.com/v2"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "prodia".to_string(),
            serde_json::from_value(json!({ "resolution": "720p" }))
                .expect("provider options deserialize"),
        );

        let result = poll_ready(
            provider
                .video("inference.wan2-2.lightning.txt2vid.v0")
                .do_generate(
                    VideoModelCallOptions::new(1)
                        .with_prompt("A wave")
                        .with_seed(42)
                        .with_provider_options(provider_options),
                ),
        );

        assert_eq!(result.videos.len(), 1);
        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "type": "inference.wan2-2.lightning.txt2vid.v0",
                "config": {
                    "prompt": "A wave",
                    "seed": 42,
                    "resolution": "720p"
                }
            })
        );
    }

    #[test]
    fn prodia_video_model_sends_multipart_for_image_to_video() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: ProdiaTransport = Arc::new(move |request| -> ProdiaTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());
            Box::pin(ready(Ok(multipart_response(
                "boundary",
                "video/mp4",
                &[7, 8, 9],
            ))))
        });
        let provider = create_prodia(
            ProdiaProviderSettings::new()
                .with_api_key("test-token")
                .with_base_url("https://api.example.com/v2"),
        )
        .with_transport(transport);

        let result = poll_ready(
            provider
                .video("inference.wan2-2.lightning.img2vid.v0")
                .do_generate(
                    VideoModelCallOptions::new(1)
                        .with_prompt("A wave")
                        .with_image(VideoModelFile::file(
                            "image/png",
                            FileDataContent::Bytes(vec![1, 2, 3]),
                        )),
                ),
        );

        assert_eq!(result.videos.len(), 1);
        let requests = requests.lock().expect("request list mutex is not poisoned");
        let Some(ProviderApiRequestBody::Bytes { content }) = requests[0].body.as_ref() else {
            panic!("expected multipart bytes body");
        };
        let body = String::from_utf8_lossy(content);
        assert!(body.contains("name=\"job\""));
        assert!(body.contains("name=\"input\""));
    }

    #[test]
    fn prodia_models_map_api_errors_to_metadata() {
        let transport: ProdiaTransport = Arc::new(move |_request| -> ProdiaTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                400,
                "Bad Request",
                json!({"detail": {"error": "invalid prompt"}}).to_string(),
            ))))
        });
        let provider = create_prodia(ProdiaProviderSettings::new().with_api_key("test-token"))
            .with_transport(transport)
            .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider
                .image("inference.sd3.txt2img.v2")
                .do_generate(ImageModelCallOptions::new(1).with_prompt("bad")),
        );

        assert!(result.images.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("prodia"))
                .and_then(|metadata| metadata.extra.get("errorMessage")),
            Some(&json!(r#"{"error":"invalid prompt"}"#))
        );
    }

    #[test]
    fn prodia_url_video_input_downloads_media_before_multipart_post() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: ProdiaTransport = Arc::new(move |request| -> ProdiaTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());
            let response = match request.url.as_str() {
                "https://example.com/input.png" => {
                    ProviderApiResponse::bytes(200, "OK", vec![1, 2, 3]).with_headers(
                        [("content-type".to_string(), "image/png".to_string())]
                            .into_iter()
                            .collect(),
                    )
                }
                _ => multipart_response("boundary", "video/mp4", &[7, 8, 9]),
            };
            Box::pin(ready(Ok(response)))
        });
        let provider = create_prodia(ProdiaProviderSettings::new().with_api_key("test-token"))
            .with_transport(transport);

        let result = poll_ready(
            provider
                .video("inference.wan2-2.lightning.img2vid.v0")
                .do_generate(
                    VideoModelCallOptions::new(1).with_image(VideoModelFile::url(
                        Url::parse("https://example.com/input.png").expect("valid URL"),
                    )),
                ),
        );

        assert_eq!(result.videos.len(), 1);
        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(requests[0].method, ProviderApiRequestMethod::Get);
        assert_eq!(requests[1].method, ProviderApiRequestMethod::Post);
    }
}
