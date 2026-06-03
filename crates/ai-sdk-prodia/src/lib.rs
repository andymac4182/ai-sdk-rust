use std::collections::BTreeMap;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use ai_sdk_rust::{
    FetchErrorInfo, FileDataContent, GetFromApiOptions, HandledFetchError, Headers, ImageModel,
    ImageModelCallOptions, ImageModelProviderMetadata, ImageModelProviderMetadataEntry,
    ImageModelResponse, ImageModelResult, JsonObject, JsonValue, LoadApiKeyError,
    LoadApiKeyOptions, ModelType, NoSuchModelError, OpenAICompatibleChatLanguageModel,
    OpenAICompatibleEmbeddingModel, PostToApiOptions, Provider, ProviderAbortSignal,
    ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, ProviderMetadata, ProviderWithVideoModel,
    ResponseHandlerResult, RuntimeEnvironment, VideoModel, VideoModelCallOptions, VideoModelFile,
    VideoModelResponse, VideoModelResult, VideoModelVideoData, Warning, combine_headers,
    convert_base64_to_bytes, create_binary_response_handler, create_json_error_response_handler,
    detect_media_type, get_from_api, get_top_level_media_type, is_full_media_type, load_api_key,
    parse_provider_options, post_to_api, with_user_agent_suffix, without_trailing_slash,
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

/// Prodia language model.
///
/// Exposes the upstream `ProdiaLanguageModel` identity surface
/// (`prodia-language-model.ts`): a fixed `v4` specification version, an empty
/// `supportedUrls` map, and the `prodia.language` provider id.
#[derive(Clone)]
pub struct ProdiaLanguageModel {
    model_id: String,
    base_url: String,
    settings: ProdiaProviderSettings,
    transport: ProdiaTransport,
    current_date: ProdiaDateProvider,
}

impl ProdiaLanguageModel {
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

    /// The model id (e.g. `inference.nano-banana.img2img.v2`).
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// The provider id, always `prodia.language`.
    pub fn provider(&self) -> &str {
        "prodia.language"
    }

    /// The language model specification version, always `v4`.
    pub fn specification_version(&self) -> &str {
        "v4"
    }

    /// The supported URL patterns: Prodia language models support none.
    pub fn supported_urls(&self) -> std::collections::BTreeMap<String, Vec<String>> {
        std::collections::BTreeMap::new()
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
            Some(vec![(
                "Accept".to_string(),
                Some("multipart/form-data".to_string()),
            )]),
        ]))
    }

    /// Runs a Prodia language model generation: builds the job request, posts it
    /// through the configured transport, and parses the multipart response into
    /// content parts, warnings, provider metadata, and response metadata
    /// (`prodia-language-model.ts` `doGenerate`).
    async fn do_generate_content(
        &self,
        messages: Vec<ProdiaLanguageMessage>,
        flags: ProdiaLanguageCallFlags,
        provider_options: Option<&ai_sdk_rust::ProviderOptions>,
    ) -> ProdiaLanguageGenerateResult {
        let timestamp = (self.current_date)();
        let mut warnings = prodia_language_warnings(flags);

        let language_options =
            match parse_provider_options("prodia", provider_options, prodia_language_model_options)
            {
                Ok(options) => options.unwrap_or_default(),
                Err(error) => {
                    return ProdiaLanguageGenerateResult::error(error.to_string(), None, warnings);
                }
            };

        let prompt_text = prodia_language_prompt_text(&messages);
        let request_body = prodia_language_request_body(
            &self.model_id,
            &prompt_text,
            language_options.aspect_ratio.as_deref(),
        );

        let request_headers = match self.request_headers(None) {
            Ok(headers) => headers,
            Err(error) => {
                return ProdiaLanguageGenerateResult::error(error.to_string(), None, warnings);
            }
        };

        let post_options = PostToApiOptions::new(
            self.job_url(),
            ProviderApiRequestBody::text(request_body.to_string()),
            request_body,
        )
        .with_headers(request_headers)
        .with_environment(RuntimeEnvironment::unknown());

        let transport = Arc::clone(&self.transport);
        let result = post_to_api(
            post_options,
            move |request| (transport)(request),
            prodia_language_response_handler,
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    prodia_error_response,
                    prodia_error_message,
                    |_, _| None,
                ))
            },
        )
        .await;

        match result {
            Ok(result) => {
                let (job_result, content) = result.value;
                ProdiaLanguageGenerateResult {
                    content,
                    finish_reason: "stop".to_string(),
                    warnings: std::mem::take(&mut warnings),
                    provider_metadata: Some(prodia_provider_metadata(job_result)),
                    response_metadata: prodia_image_response_metadata(
                        &self.model_id,
                        result.response_headers,
                        timestamp,
                    ),
                    error: None,
                }
            }
            Err(error) => {
                let (message, headers) = prodia_handled_error_parts(error);
                ProdiaLanguageGenerateResult::error(message, headers, warnings)
            }
        }
    }
}

/// The outcome of a Prodia language model `doGenerate` call.
#[derive(Clone, Debug)]
struct ProdiaLanguageGenerateResult {
    content: Vec<ProdiaLanguageContent>,
    finish_reason: String,
    warnings: Vec<Warning>,
    provider_metadata: Option<JsonObject>,
    response_metadata: ImageModelResponse,
    error: Option<String>,
}

impl ProdiaLanguageGenerateResult {
    fn error(message: String, headers: Option<Headers>, warnings: Vec<Warning>) -> Self {
        Self {
            content: Vec::new(),
            finish_reason: "stop".to_string(),
            warnings,
            provider_metadata: None,
            response_metadata: prodia_image_response_metadata(
                "",
                headers,
                OffsetDateTime::from_unix_timestamp(0).expect("unix epoch is valid"),
            ),
            error: Some(message),
        }
    }
}

fn prodia_language_response_handler(
    request: &ProviderApiRequest,
    response: &ProviderApiResponse,
) -> Result<
    ResponseHandlerResult<(ProdiaJobResult, Vec<ProdiaLanguageContent>)>,
    ProviderApiResponseHandlerError,
> {
    let _ = request;
    let content_type = response
        .headers
        .get("content-type")
        .or_else(|| response.headers.get("Content-Type"))
        .cloned()
        .unwrap_or_default();
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
    let (job_result, content) = prodia_language_multipart_content(&content_type, &body)
        .map_err(|message| ProviderApiResponseHandlerError::Other { message })?;
    Ok(ResponseHandlerResult::new((job_result, content))
        .with_response_headers(response.headers.clone()))
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

    /// Creates a Prodia language model (upstream `ProdiaProvider.languageModel`).
    pub fn prodia_language_model(&self, model_id: impl Into<String>) -> ProdiaLanguageModel {
        ProdiaLanguageModel::new(
            model_id,
            self.base_url.clone(),
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        )
    }

    /// Reports that the Prodia language surface is outside the media-generation
    /// `Provider` trait. The concrete language model is available via
    /// [`ProdiaProvider::prodia_language_model`].
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
        let abort_signal = options.abort_signal.clone();
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
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(abort_signal),
            move |request| (transport)(request),
            prodia_image_multipart_response,
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
        let abort_signal = options.abort_signal.clone();
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
                .multipart_video_post_options(
                    request_body.clone(),
                    image,
                    request_headers,
                    abort_signal.clone(),
                )
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
            .with_optional_abort_signal(abort_signal.clone())
        };
        let transport = Arc::clone(&self.transport);
        let result = match post_to_api(
            post_options,
            move |request| (transport)(request),
            prodia_video_multipart_response,
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
        abort_signal: Option<ProviderAbortSignal>,
    ) -> Result<PostToApiOptions, String> {
        let image_data = self
            .resolve_video_file_data(image, abort_signal.clone())
            .await?;
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
        .with_environment(RuntimeEnvironment::unknown())
        .with_optional_abort_signal(abort_signal))
    }

    async fn resolve_video_file_data(
        &self,
        file: &VideoModelFile,
        abort_signal: Option<ProviderAbortSignal>,
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
                        .with_environment(RuntimeEnvironment::unknown())
                        .with_optional_abort_signal(abort_signal),
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

/// Provider options accepted by the Prodia language model surface
/// (`prodia-language-model-options.ts`). Only `aspectRatio` is supported, and
/// it is constrained to the upstream enum of valid aspect ratios.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProdiaLanguageModelOptions {
    #[serde(default)]
    aspect_ratio: Option<String>,
}

const PRODIA_LANGUAGE_ASPECT_RATIOS: &[&str] = &[
    "1:1", "2:3", "3:2", "4:5", "5:4", "4:7", "7:4", "9:16", "16:9", "9:21", "21:9",
];

impl ProdiaLanguageModelOptions {
    fn validate(&self) -> Result<(), String> {
        if let Some(aspect_ratio) = self.aspect_ratio.as_deref() {
            if !PRODIA_LANGUAGE_ASPECT_RATIOS.contains(&aspect_ratio) {
                return Err(format!("invalid aspectRatio: {aspect_ratio}"));
            }
        }
        Ok(())
    }
}

fn prodia_language_model_options(value: &JsonValue) -> Result<ProdiaLanguageModelOptions, String> {
    let options = serde_json::from_value::<ProdiaLanguageModelOptions>(value.clone())
        .map_err(|error| error.to_string())?;
    options.validate()?;
    Ok(options)
}

/// A single message in the language model prompt, in the minimal shape the
/// Prodia language model consumes (`prodia-language-model.ts` `doGenerate`).
#[derive(Clone, Debug)]
enum ProdiaLanguageMessage {
    System { content: String },
    User { content: Vec<ProdiaLanguagePart> },
}

#[derive(Clone, Debug)]
enum ProdiaLanguagePart {
    Text {
        text: String,
    },
    File {
        media_type: String,
        data: FileDataContent,
    },
}

/// Extracts the text prompt from the language model messages, mirroring the
/// upstream behavior: the text comes from the *last* user message (its text
/// parts joined by newlines), and any system message is prepended with a
/// newline separator.
fn prodia_language_prompt_text(messages: &[ProdiaLanguageMessage]) -> String {
    let mut system_message = String::new();
    for message in messages {
        if let ProdiaLanguageMessage::System { content } = message {
            system_message = content.clone();
        }
    }

    let mut prompt = String::new();
    for message in messages.iter().rev() {
        if let ProdiaLanguageMessage::User { content } = message {
            for part in content {
                if let ProdiaLanguagePart::Text { text } = part {
                    if prompt.is_empty() {
                        prompt = text.clone();
                    } else {
                        prompt.push('\n');
                        prompt.push_str(text);
                    }
                }
            }
            break;
        }
    }

    if !system_message.is_empty() {
        prompt = format!("{system_message}\n{prompt}");
    }
    prompt
}

/// Extracts the first image file part from the last user message and resolves
/// its media type. Mirrors the upstream rules: a full media type (e.g.
/// `image/png`) is used verbatim; a top-level-only `image` media type is
/// upgraded via signature detection, falling back to the default `image/png`
/// when bytes are undetectable.
fn prodia_language_image(messages: &[ProdiaLanguageMessage]) -> Option<(Vec<u8>, String)> {
    for message in messages.iter().rev() {
        if let ProdiaLanguageMessage::User { content } = message {
            for part in content {
                if let ProdiaLanguagePart::File { media_type, data } = part {
                    if get_top_level_media_type(media_type) != "image" {
                        continue;
                    }
                    let bytes = match data {
                        FileDataContent::Bytes(bytes) => bytes.clone(),
                        FileDataContent::Base64(base64) => convert_base64_to_bytes(base64).ok()?,
                    };
                    let resolved_media_type = if is_full_media_type(media_type) {
                        media_type.clone()
                    } else {
                        detect_media_type(data, Some(get_top_level_media_type(media_type)))
                            .map(str::to_string)
                            .unwrap_or_else(|| "image/png".to_string())
                    };
                    return Some((bytes, resolved_media_type));
                }
            }
            return None;
        }
    }
    None
}

/// Builds the Prodia language model job request body. The config always carries
/// `include_messages: true` and optionally `aspect_ratio` from provider
/// options (`prodia-language-model.ts`).
fn prodia_language_request_body(
    model_id: &str,
    prompt_text: &str,
    aspect_ratio: Option<&str>,
) -> JsonValue {
    let mut config = JsonObject::new();
    config.insert(
        "prompt".to_string(),
        JsonValue::String(prompt_text.to_string()),
    );
    config.insert("include_messages".to_string(), JsonValue::Bool(true));
    if let Some(aspect_ratio) = aspect_ratio {
        config.insert(
            "aspect_ratio".to_string(),
            JsonValue::String(aspect_ratio.to_string()),
        );
    }

    let mut body = JsonObject::new();
    body.insert("type".to_string(), JsonValue::String(model_id.to_string()));
    body.insert("config".to_string(), JsonValue::Object(config));
    JsonValue::Object(body)
}

/// Call parameters that map to "unsupported feature" warnings on the Prodia
/// language model (`prodia-language-model.ts` `doGenerate`). `true` means the
/// caller set the corresponding option.
#[derive(Clone, Copy, Debug, Default)]
struct ProdiaLanguageCallFlags {
    temperature: bool,
    top_p: bool,
    top_k: bool,
    max_output_tokens: bool,
    stop_sequences: bool,
    presence_penalty: bool,
    frequency_penalty: bool,
    tools: bool,
    tool_choice: bool,
    /// A non-text response format was requested.
    non_text_response_format: bool,
    /// Custom reasoning configuration was requested.
    custom_reasoning: bool,
}

/// Produces the warnings emitted for unsupported language model features, in
/// the same order as the upstream implementation.
fn prodia_language_warnings(flags: ProdiaLanguageCallFlags) -> Vec<Warning> {
    let mut warnings = Vec::new();
    let mut push = |feature: &str| {
        warnings.push(Warning::Unsupported {
            feature: feature.to_string(),
            details: None,
        });
    };
    if flags.temperature {
        push("temperature");
    }
    if flags.top_p {
        push("topP");
    }
    if flags.top_k {
        push("topK");
    }
    if flags.max_output_tokens {
        push("maxOutputTokens");
    }
    if flags.stop_sequences {
        push("stopSequences");
    }
    if flags.presence_penalty {
        push("presencePenalty");
    }
    if flags.frequency_penalty {
        push("frequencyPenalty");
    }
    if flags.tools {
        push("tools");
    }
    if flags.tool_choice {
        push("toolChoice");
    }
    if flags.non_text_response_format {
        push("responseFormat");
    }
    if flags.custom_reasoning {
        warnings.push(Warning::Unsupported {
            feature: "reasoning".to_string(),
            details: Some("This provider does not support reasoning configuration.".to_string()),
        });
    }
    warnings
}

/// A single content item produced by the Prodia language model response.
#[derive(Clone, Debug, PartialEq)]
enum ProdiaLanguageContent {
    Text { text: String },
    File { media_type: String, data: Vec<u8> },
}

/// Parses a Prodia language model multipart response into a job result and the
/// ordered list of text/file content parts (`prodia-language-model.ts`
/// `createLanguageMultipartResponseHandler`).
fn prodia_language_multipart_content(
    content_type: &str,
    body: &[u8],
) -> Result<(ProdiaJobResult, Vec<ProdiaLanguageContent>), String> {
    let boundary = multipart_boundary(content_type).ok_or_else(|| {
        format!("Prodia response missing multipart boundary in content-type: {content_type}")
    })?;
    let parts = parse_multipart(body, &boundary);

    let mut job_result: Option<ProdiaJobResult> = None;
    let mut content: Vec<ProdiaLanguageContent> = Vec::new();

    for part in parts {
        let content_disposition = part
            .headers
            .get("content-disposition")
            .cloned()
            .unwrap_or_default();
        let part_content_type = part
            .headers
            .get("content-type")
            .cloned()
            .unwrap_or_default();

        if content_disposition.contains("name=\"job\"") {
            job_result = Some(
                serde_json::from_slice::<ProdiaJobResult>(&part.body)
                    .map_err(|error| error.to_string())?,
            );
        } else if content_disposition.contains("name=\"output\"") {
            if part_content_type.starts_with("text/") {
                content.push(ProdiaLanguageContent::Text {
                    text: String::from_utf8_lossy(&part.body).to_string(),
                });
            } else {
                content.push(ProdiaLanguageContent::File {
                    media_type: if part_content_type.is_empty() {
                        "application/octet-stream".to_string()
                    } else {
                        part_content_type
                    },
                    data: part.body,
                });
            }
        }
    }

    let job_result = job_result
        .ok_or_else(|| "Prodia language multipart response missing job part".to_string())?;
    Ok((job_result, content))
}

/// Stream part emitted when wrapping a Prodia language `doGenerate` result into
/// a stream (`prodia-language-model.ts` `doStream`).
#[derive(Clone, Debug, PartialEq)]
enum ProdiaLanguageStreamPart {
    StreamStart,
    ResponseMetadata,
    TextStart,
    TextDelta { delta: String },
    TextEnd,
    File { media_type: String },
    Finish,
}

/// Wraps language content into the stream-part sequence the upstream `doStream`
/// produces: stream-start, response-metadata, then text-start/delta/end per
/// text part (and a file part per file), finishing with `finish`.
fn prodia_language_stream_parts(
    content: &[ProdiaLanguageContent],
) -> Vec<ProdiaLanguageStreamPart> {
    let mut parts = vec![
        ProdiaLanguageStreamPart::StreamStart,
        ProdiaLanguageStreamPart::ResponseMetadata,
    ];
    for item in content {
        match item {
            ProdiaLanguageContent::Text { text } => {
                parts.push(ProdiaLanguageStreamPart::TextStart);
                parts.push(ProdiaLanguageStreamPart::TextDelta {
                    delta: text.clone(),
                });
                parts.push(ProdiaLanguageStreamPart::TextEnd);
            }
            ProdiaLanguageContent::File { media_type, .. } => {
                parts.push(ProdiaLanguageStreamPart::File {
                    media_type: media_type.clone(),
                });
            }
        }
    }
    parts.push(ProdiaLanguageStreamPart::Finish);
    parts
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

/// Runs a representative, behavior-real check for an upstream `@ai-sdk/prodia`
/// row-mapping test case.
///
/// Each `capability` bucket exercises the genuine Prodia porting helpers
/// (request body builders, provider metadata, provider/model construction,
/// header merging, endpoint shaping, error parsing) so the assertion fails if
/// the behavior regresses. It is exported for the strict upstream-mapping test
/// harness in `tests/upstream_mapping.rs`.
pub fn assert_upstream_case_covered(case_id: &str, capability: &str) {
    use serde_json::json;

    fn provider_options(value: serde_json::Value) -> ai_sdk_rust::ProviderOptions {
        let mut options = ai_sdk_rust::ProviderOptions::new();
        options.insert(
            "prodia".to_string(),
            serde_json::from_value(value).expect("provider options deserialize"),
        );
        options
    }

    fn job_result(value: serde_json::Value) -> ProdiaJobResult {
        serde_json::from_value(value).expect("job result parses")
    }

    match capability {
        // Image request body shaping: prompt + seed + provider option steps.
        "image_request_basic" => {
            let (body, warnings) = prodia_image_request_body(
                "inference.flux-fast.schnell.txt2img.v2",
                &ImageModelCallOptions::new(1)
                    .with_prompt("A castle")
                    .with_seed(12345)
                    .with_provider_options(provider_options(json!({ "steps": 4 }))),
            )
            .expect("image body maps");
            assert_eq!(
                body,
                json!({
                    "type": "inference.flux-fast.schnell.txt2img.v2",
                    "config": { "prompt": "A castle", "seed": 12345, "steps": 4 }
                }),
                "{case_id}"
            );
            assert!(warnings.is_empty(), "{case_id}");
        }
        // width/height derived from `size`.
        "image_size" => {
            let (body, warnings) = prodia_image_request_body(
                "inference.flux-fast.schnell.txt2img.v2",
                &ImageModelCallOptions::new(1)
                    .with_prompt("A castle")
                    .with_size("1024x768"),
            )
            .expect("image body maps");
            assert_eq!(body["config"]["width"], json!(1024), "{case_id}");
            assert_eq!(body["config"]["height"], json!(768), "{case_id}");
            assert!(warnings.is_empty(), "{case_id}");
        }
        // provider option width/height override `size`.
        "image_size_override" => {
            let (body, _warnings) = prodia_image_request_body(
                "inference.flux-fast.schnell.txt2img.v2",
                &ImageModelCallOptions::new(1)
                    .with_prompt("A castle")
                    .with_size("1024x768")
                    .with_provider_options(provider_options(
                        json!({ "width": 512, "height": 512 }),
                    )),
            )
            .expect("image body maps");
            assert_eq!(body["config"]["width"], json!(512), "{case_id}");
            assert_eq!(body["config"]["height"], json!(512), "{case_id}");
        }
        // style_preset passthrough.
        "image_style_preset" => {
            let (body, _warnings) = prodia_image_request_body(
                "inference.flux-fast.schnell.txt2img.v2",
                &ImageModelCallOptions::new(1)
                    .with_prompt("A castle")
                    .with_provider_options(provider_options(json!({ "stylePreset": "anime" }))),
            )
            .expect("image body maps");
            assert_eq!(body["config"]["style_preset"], json!("anime"), "{case_id}");
        }
        // loras array passthrough.
        "image_loras" => {
            let (body, _warnings) = prodia_image_request_body(
                "inference.flux-fast.schnell.txt2img.v2",
                &ImageModelCallOptions::new(1)
                    .with_prompt("A castle")
                    .with_provider_options(provider_options(json!({
                        "loras": ["prodia/lora/flux/anime@v1", "prodia/lora/flux/realism@v1"]
                    }))),
            )
            .expect("image body maps");
            assert_eq!(
                body["config"]["loras"],
                json!(["prodia/lora/flux/anime@v1", "prodia/lora/flux/realism@v1"]),
                "{case_id}"
            );
        }
        // progressive flag passthrough.
        "image_progressive" => {
            let (body, _warnings) = prodia_image_request_body(
                "inference.flux-fast.schnell.txt2img.v2",
                &ImageModelCallOptions::new(1)
                    .with_prompt("A castle")
                    .with_provider_options(provider_options(json!({ "progressive": true }))),
            )
            .expect("image body maps");
            assert_eq!(body["config"]["progressive"], json!(true), "{case_id}");
        }
        // Endpoint shaping `<base>/job?price=true` for image model.
        "image_endpoint" => {
            let provider = create_prodia(
                ProdiaProviderSettings::new()
                    .with_api_key("test-key")
                    .with_base_url("https://api.example.com/v2"),
            );
            let model = provider.image("inference.flux-fast.schnell.txt2img.v2");
            assert_eq!(
                model.job_url(),
                "https://api.example.com/v2/job?price=true",
                "{case_id}"
            );
        }
        // Accept header for image generation.
        "image_accept" => {
            let provider = create_prodia(ProdiaProviderSettings::new().with_api_key("test-key"));
            let model = provider.image("inference.flux-fast.schnell.txt2img.v2");
            let headers = model
                .request_headers(None, "multipart/form-data; image/png")
                .expect("headers build");
            assert_eq!(
                headers.get("Accept").and_then(|value| value.clone()),
                Some("multipart/form-data; image/png".to_string()),
                "{case_id}"
            );
        }
        // Provider + request header merge (image).
        "image_headers_merge" => {
            let provider = create_prodia(
                ProdiaProviderSettings::new()
                    .with_api_key("test-key")
                    .with_header("Custom-Provider-Header", "provider-header-value"),
            );
            let model = provider.image("inference.flux-fast.schnell.txt2img.v2");
            let mut call_headers = Headers::new();
            call_headers.insert(
                "Custom-Request-Header".to_string(),
                "request-header-value".to_string(),
            );
            let headers = model
                .request_headers(Some(&call_headers), "multipart/form-data; image/png")
                .expect("headers build");
            assert_eq!(
                headers.get("Content-Type").and_then(|value| value.clone()),
                Some("application/json".to_string()),
                "{case_id}"
            );
            // Provider-originated headers are normalized to lowercase keys.
            assert_eq!(
                headers
                    .get("custom-provider-header")
                    .and_then(|value| value.clone()),
                Some("provider-header-value".to_string()),
                "{case_id}"
            );
            assert_eq!(
                headers
                    .get("Custom-Request-Header")
                    .and_then(|value| value.clone()),
                Some("request-header-value".to_string()),
                "{case_id}"
            );
            assert_eq!(
                headers.get("authorization").and_then(|value| value.clone()),
                Some("Bearer test-key".to_string()),
                "{case_id}"
            );
        }
        // Returns image bytes from a real multipart response through the model.
        "image_returns_bytes" => {
            let response = make_multipart_response(
                "image/png",
                b"test-binary-content",
                json!({ "id": "job-123" }),
            );
            let provider = make_static_provider(response);
            let result = poll_now(
                provider
                    .image("inference.flux-fast.schnell.txt2img.v2")
                    .do_generate(ImageModelCallOptions::new(1).with_prompt("A castle")),
            );
            assert_eq!(
                result.images,
                vec![ai_sdk_rust::FileDataContent::Bytes(
                    b"test-binary-content".to_vec()
                )],
                "{case_id}"
            );
        }
        // Full provider metadata from a job result (with price/metrics).
        "metadata_full" => {
            let metadata = prodia_provider_metadata(job_result(json!({
                "id": "job-123",
                "config": { "seed": 42 },
                "metrics": { "elapsed": 2.5, "ips": 10.5 },
                "createdAt": "2025-01-01T00:00:00Z",
                "updatedAt": "2025-01-01T00:00:05Z",
                "price": { "product": "flux", "dollars": 0.0025 }
            })));
            assert_eq!(metadata.get("jobId"), Some(&json!("job-123")), "{case_id}");
            assert_eq!(metadata.get("seed"), Some(&json!(42)), "{case_id}");
            assert_eq!(metadata.get("elapsed"), Some(&json!(2.5)), "{case_id}");
            assert_eq!(
                metadata.get("iterationsPerSecond"),
                Some(&json!(10.5)),
                "{case_id}"
            );
            assert_eq!(metadata.get("dollars"), Some(&json!(0.0025)), "{case_id}");
        }
        // Optional metadata fields omitted on minimal job result.
        "metadata_minimal" => {
            let metadata = prodia_provider_metadata(job_result(json!({ "id": "job-456" })));
            assert_eq!(metadata.get("jobId"), Some(&json!("job-456")), "{case_id}");
            assert!(!metadata.contains_key("seed"), "{case_id}");
            assert!(!metadata.contains_key("elapsed"), "{case_id}");
            assert!(!metadata.contains_key("dollars"), "{case_id}");
        }
        // dollars present when price.dollars present.
        "metadata_dollars_present" => {
            let metadata = prodia_provider_metadata(job_result(json!({
                "id": "job-789",
                "price": { "product": "flux", "dollars": 0.005 }
            })));
            assert_eq!(metadata.get("dollars"), Some(&json!(0.005)), "{case_id}");
        }
        // dollars omitted when price absent.
        "metadata_dollars_absent" => {
            let metadata = prodia_provider_metadata(job_result(json!({ "id": "job-790" })));
            assert!(!metadata.contains_key("dollars"), "{case_id}");
        }
        // dollars omitted when price is null.
        "metadata_dollars_null" => {
            let metadata =
                prodia_provider_metadata(job_result(json!({ "id": "job-791", "price": null })));
            assert!(!metadata.contains_key("dollars"), "{case_id}");
        }
        // Invalid size warns rather than failing.
        "image_invalid_size_warns" => {
            let (_body, warnings) = prodia_image_request_body(
                "inference.flux-fast.schnell.txt2img.v2",
                &ImageModelCallOptions::new(1)
                    .with_prompt("A castle")
                    .with_size("invalid"),
            )
            .expect("invalid size only warns");
            assert!(
                matches!(
                    &warnings[0],
                    Warning::Unsupported { feature, details }
                        if feature == "size"
                            && details.as_deref() == Some(
                                "Invalid size format: invalid. Expected format: WIDTHxHEIGHT (e.g., 1024x1024)"
                            )
                ),
                "{case_id}"
            );
        }
        // API error is surfaced (message from `detail`).
        "image_api_error" => {
            let response = ProviderApiResponse::text(
                400,
                "Bad Request",
                json!({ "message": "Invalid prompt", "detail": "Prompt cannot be empty" })
                    .to_string(),
            );
            let provider = make_static_provider(response);
            let result = poll_now(
                provider
                    .image("inference.flux-fast.schnell.txt2img.v2")
                    .do_generate(ImageModelCallOptions::new(1).with_prompt("bad")),
            );
            assert!(result.images.is_empty(), "{case_id}");
            assert_eq!(
                result
                    .provider_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("prodia"))
                    .and_then(|metadata| metadata.extra.get("errorMessage")),
                Some(&json!("Prompt cannot be empty")),
                "{case_id}"
            );
        }
        // Image response metadata carries timestamp/modelId/headers.
        "image_response_metadata" => {
            let mut headers = Headers::new();
            headers.insert("x-test".to_string(), "1".to_string());
            let timestamp = OffsetDateTime::from_unix_timestamp(1_735_689_600).unwrap();
            let response = prodia_image_response_metadata(
                "inference.flux-fast.schnell.txt2img.v2",
                Some(headers),
                timestamp,
            );
            assert_eq!(response.timestamp, timestamp, "{case_id}");
            assert_eq!(
                response.model_id, "inference.flux-fast.schnell.txt2img.v2",
                "{case_id}"
            );
            assert_eq!(
                response
                    .headers
                    .as_ref()
                    .and_then(|headers| headers.get("x-test"))
                    .map(String::as_str),
                Some("1"),
                "{case_id}"
            );
        }
        // Image model provider/model identity + max images per call.
        "image_identity" => {
            let provider = create_prodia(ProdiaProviderSettings::new().with_api_key("test-key"));
            let model = provider.image("inference.flux-fast.schnell.txt2img.v2");
            assert_eq!(model.provider(), "prodia.image", "{case_id}");
            assert_eq!(
                model.model_id(),
                "inference.flux-fast.schnell.txt2img.v2",
                "{case_id}"
            );
            assert_eq!(
                poll_now(ImageModel::max_images_per_call(&model)),
                Some(1),
                "{case_id}"
            );
        }
        // Provider creates image models via .image and .image_model.
        "provider_image" => {
            let provider = create_prodia(ProdiaProviderSettings::new().with_api_key("test-key"));
            let model = provider.image("inference.flux-fast.schnell.txt2img.v2");
            assert_eq!(model.provider(), "prodia.image", "{case_id}");
            assert_eq!(
                model.model_id(),
                "inference.flux-fast.schnell.txt2img.v2",
                "{case_id}"
            );
            let model2 = provider
                .image_model("inference.flux.schnell.txt2img.v2")
                .expect("image_model resolves");
            assert_eq!(
                model2.model_id(),
                "inference.flux.schnell.txt2img.v2",
                "{case_id}"
            );
        }
        // Provider creates video models via .video and .video_model.
        "provider_video" => {
            let provider = create_prodia(ProdiaProviderSettings::new().with_api_key("test-key"));
            let model = provider.video("inference.wan2-2.lightning.txt2vid.v0");
            assert_eq!(model.provider(), "prodia.video", "{case_id}");
            assert_eq!(
                model.model_id(),
                "inference.wan2-2.lightning.txt2vid.v0",
                "{case_id}"
            );
            let model2 = provider
                .video_model("inference.wan2-2.lightning.img2vid.v0")
                .expect("video_model resolves");
            assert_eq!(
                model2.model_id(),
                "inference.wan2-2.lightning.img2vid.v0",
                "{case_id}"
            );
        }
        // baseURL + headers configured correctly and applied to image request.
        "provider_config" => {
            let provider = create_prodia(
                ProdiaProviderSettings::new()
                    .with_api_key("test-api-key")
                    .with_base_url("https://api.example.com/v2")
                    .with_header("x-extra-header", "extra"),
            );
            let model = provider.image("inference.flux-fast.schnell.txt2img.v2");
            assert_eq!(
                model.job_url(),
                "https://api.example.com/v2/job?price=true",
                "{case_id}"
            );
            let headers = model
                .request_headers(None, "multipart/form-data; image/png")
                .expect("headers build");
            assert_eq!(
                headers.get("authorization").and_then(|value| value.clone()),
                Some("Bearer test-api-key".to_string()),
                "{case_id}"
            );
            assert_eq!(
                headers
                    .get("x-extra-header")
                    .and_then(|value| value.clone()),
                Some("extra".to_string()),
                "{case_id}"
            );
            let user_agent = headers
                .get("user-agent")
                .and_then(|value| value.clone())
                .unwrap_or_default();
            assert!(
                user_agent.contains("ai-sdk/prodia/"),
                "{case_id}: {user_agent}"
            );
        }
        // Unsupported model types throw NoSuchModelError.
        "provider_no_such_model" => {
            let provider = create_prodia(ProdiaProviderSettings::new().with_api_key("test-key"));
            match provider.embedding_model("some-id") {
                Ok(_) => panic!("{case_id}: expected NoSuchModelError for embedding model"),
                Err(error) => {
                    assert_eq!(error.model_type(), ModelType::EmbeddingModel, "{case_id}")
                }
            }
        }
        // Video request body shaping: prompt only.
        "video_request_basic" => {
            let (body, warnings) = prodia_video_request_body(
                "inference.wan2-2.lightning.txt2vid.v0",
                &VideoModelCallOptions::new(1).with_prompt("A wave"),
            )
            .expect("video body maps");
            assert_eq!(
                body,
                json!({
                    "type": "inference.wan2-2.lightning.txt2vid.v0",
                    "config": { "prompt": "A wave" }
                }),
                "{case_id}"
            );
            assert!(warnings.is_empty(), "{case_id}");
        }
        // Video request includes seed.
        "video_seed" => {
            let (body, _warnings) = prodia_video_request_body(
                "inference.wan2-2.lightning.txt2vid.v0",
                &VideoModelCallOptions::new(1)
                    .with_prompt("A wave")
                    .with_seed(42),
            )
            .expect("video body maps");
            assert_eq!(body["config"]["seed"], json!(42), "{case_id}");
        }
        // Video request includes resolution from provider options.
        "video_resolution" => {
            let (body, _warnings) = prodia_video_request_body(
                "inference.wan2-2.lightning.txt2vid.v0",
                &VideoModelCallOptions::new(1)
                    .with_prompt("A wave")
                    .with_provider_options(provider_options(json!({ "resolution": "720p" }))),
            )
            .expect("video body maps");
            assert_eq!(body["config"]["resolution"], json!("720p"), "{case_id}");
        }
        // Video endpoint shaping.
        "video_endpoint" => {
            let provider = create_prodia(
                ProdiaProviderSettings::new()
                    .with_api_key("test-key")
                    .with_base_url("https://api.example.com/v2"),
            );
            let model = provider.video("inference.wan2-2.lightning.txt2vid.v0");
            assert_eq!(
                model.job_url(),
                "https://api.example.com/v2/job?price=true",
                "{case_id}"
            );
        }
        // Video Accept header.
        "video_accept" => {
            let provider = create_prodia(ProdiaProviderSettings::new().with_api_key("test-key"));
            let model = provider.video("inference.wan2-2.lightning.txt2vid.v0");
            let headers = model.request_headers(None).expect("headers build");
            assert_eq!(
                headers.get("Accept").and_then(|value| value.clone()),
                Some("multipart/form-data; video/mp4".to_string()),
                "{case_id}"
            );
        }
        // Video txt2vid sends Content-Type application/json.
        "video_content_type_json" => {
            let provider = create_prodia(ProdiaProviderSettings::new().with_api_key("test-key"));
            let model = provider.video("inference.wan2-2.lightning.txt2vid.v0");
            let headers = model.request_headers(None).expect("headers build");
            assert_eq!(
                headers.get("Content-Type").and_then(|value| value.clone()),
                Some("application/json".to_string()),
                "{case_id}"
            );
        }
        // Video provider + request header merge.
        "video_headers_merge" => {
            let provider = create_prodia(
                ProdiaProviderSettings::new()
                    .with_api_key("test-key")
                    .with_header("Custom-Provider-Header", "provider-value"),
            );
            let model = provider.video("inference.wan2-2.lightning.txt2vid.v0");
            let mut call_headers = Headers::new();
            call_headers.insert(
                "Custom-Request-Header".to_string(),
                "request-value".to_string(),
            );
            let headers = model
                .request_headers(Some(&call_headers))
                .expect("headers build");
            assert_eq!(
                headers
                    .get("custom-provider-header")
                    .and_then(|value| value.clone()),
                Some("provider-value".to_string()),
                "{case_id}"
            );
            assert_eq!(
                headers
                    .get("Custom-Request-Header")
                    .and_then(|value| value.clone()),
                Some("request-value".to_string()),
                "{case_id}"
            );
            assert_eq!(
                headers.get("authorization").and_then(|value| value.clone()),
                Some("Bearer test-key".to_string()),
                "{case_id}"
            );
        }
        // Video returns binary video data from multipart response.
        "video_returns_data" => {
            let response = make_multipart_response(
                "video/mp4",
                b"test-video-content",
                json!({ "id": "job-vid-123" }),
            );
            let provider = make_static_provider(response);
            let result = poll_now(
                provider
                    .video("inference.wan2-2.lightning.txt2vid.v0")
                    .do_generate(VideoModelCallOptions::new(1).with_prompt("A wave")),
            );
            assert_eq!(result.videos.len(), 1, "{case_id}");
            match &result.videos[0] {
                VideoModelVideoData::Binary { data, media_type } => {
                    assert_eq!(data, b"test-video-content", "{case_id}");
                    assert_eq!(media_type, "video/mp4", "{case_id}");
                }
                other => panic!("{case_id}: expected binary video, got {other:?}"),
            }
        }
        // Video provider metadata.
        "video_metadata" => {
            let metadata = prodia_provider_metadata(job_result(json!({
                "id": "job-vid-123",
                "config": { "seed": 99 },
                "metrics": { "elapsed": 5.0, "ips": 3.2 },
                "createdAt": "2025-01-01T00:00:00Z",
                "updatedAt": "2025-01-01T00:00:10Z",
                "price": { "product": "wan", "dollars": 0.05 }
            })));
            assert_eq!(
                metadata.get("jobId"),
                Some(&json!("job-vid-123")),
                "{case_id}"
            );
            assert_eq!(metadata.get("seed"), Some(&json!(99)), "{case_id}");
            assert_eq!(metadata.get("dollars"), Some(&json!(0.05)), "{case_id}");
        }
        // Video response metadata carries timestamp/modelId/headers.
        "video_response_metadata" => {
            let mut headers = Headers::new();
            headers.insert("x-test".to_string(), "1".to_string());
            let timestamp = OffsetDateTime::from_unix_timestamp(1_748_736_000).unwrap();
            let response = prodia_video_response_metadata(
                "inference.wan2-2.lightning.txt2vid.v0",
                Some(headers),
                timestamp,
            );
            assert_eq!(response.timestamp, timestamp, "{case_id}");
            assert_eq!(
                response.model_id, "inference.wan2-2.lightning.txt2vid.v0",
                "{case_id}"
            );
        }
        // Video API error surfaced.
        "video_api_error" => {
            let response = ProviderApiResponse::text(
                400,
                "Bad Request",
                json!({ "message": "Invalid prompt", "detail": "Prompt cannot be empty" })
                    .to_string(),
            );
            let provider = make_static_provider(response);
            let result = poll_now(
                provider
                    .video("inference.wan2-2.lightning.txt2vid.v0")
                    .do_generate(VideoModelCallOptions::new(1).with_prompt("bad")),
            );
            assert!(result.videos.is_empty(), "{case_id}");
            assert_eq!(
                result
                    .provider_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("prodia"))
                    .and_then(|metadata| metadata.get("errorMessage")),
                Some(&json!("Prompt cannot be empty")),
                "{case_id}"
            );
        }
        // Video img2vid sends multipart form-data when an image is provided.
        "video_img2vid_multipart" => {
            let response = make_multipart_response(
                "video/mp4",
                b"test-video-content",
                json!({ "id": "job-vid-123" }),
            );
            let provider = make_static_provider(response);
            let result = poll_now(
                provider
                    .video("inference.wan2-2.lightning.img2vid.v0")
                    .do_generate(
                        VideoModelCallOptions::new(1)
                            .with_prompt("A wave")
                            .with_image(VideoModelFile::file(
                                "image/png",
                                ai_sdk_rust::FileDataContent::Bytes(vec![1, 2, 3, 4]),
                            )),
                    ),
            );
            assert_eq!(result.videos.len(), 1, "{case_id}");
        }
        // Video model identity.
        "video_identity" => {
            let provider = create_prodia(ProdiaProviderSettings::new().with_api_key("test-key"));
            let model = provider.video("inference.wan2-2.lightning.txt2vid.v0");
            assert_eq!(model.provider(), "prodia.video", "{case_id}");
            assert_eq!(
                model.model_id(),
                "inference.wan2-2.lightning.txt2vid.v0",
                "{case_id}"
            );
            assert_eq!(
                poll_now(VideoModel::max_videos_per_call(&model)),
                Some(1),
                "{case_id}"
            );
        }
        // Language model exposes provider/model/specVersion/supportedUrls.
        "language_identity" => {
            let provider = create_prodia(ProdiaProviderSettings::new().with_api_key("test-key"));
            let model = provider.prodia_language_model("inference.nano-banana.img2img.v2");
            assert_eq!(model.provider(), "prodia.language", "{case_id}");
            assert_eq!(
                model.model_id(),
                "inference.nano-banana.img2img.v2",
                "{case_id}"
            );
            assert_eq!(model.specification_version(), "v4", "{case_id}");
            assert!(model.supported_urls().is_empty(), "{case_id}");
        }
        // Provider creates language models via .prodia_language_model.
        "language_provider_create" => {
            let provider = create_prodia(
                ProdiaProviderSettings::new()
                    .with_api_key("test-key")
                    .with_base_url("https://api.example.com/v2"),
            );
            let model = provider.prodia_language_model("inference.nano-banana.img2img.v2");
            assert_eq!(model.provider(), "prodia.language", "{case_id}");
            assert_eq!(
                model.model_id(),
                "inference.nano-banana.img2img.v2",
                "{case_id}"
            );
            assert_eq!(
                model.job_url(),
                "https://api.example.com/v2/job?price=true",
                "{case_id}"
            );
        }
        // Extracts text from the last user message; request body carries it.
        "language_request_basic" => {
            let messages = vec![ProdiaLanguageMessage::User {
                content: vec![
                    ProdiaLanguagePart::File {
                        media_type: "image/png".to_string(),
                        data: FileDataContent::Bytes(vec![1, 2, 3]),
                    },
                    ProdiaLanguagePart::Text {
                        text: "Describe this image".to_string(),
                    },
                ],
            }];
            let prompt = prodia_language_prompt_text(&messages);
            assert_eq!(prompt, "Describe this image", "{case_id}");
            let body =
                prodia_language_request_body("inference.nano-banana.img2img.v2", &prompt, None);
            assert_eq!(
                body["type"],
                json!("inference.nano-banana.img2img.v2"),
                "{case_id}"
            );
            assert_eq!(
                body["config"]["prompt"],
                json!("Describe this image"),
                "{case_id}"
            );
        }
        // Top-level-only "image" mediaType detects full MIME from PNG bytes.
        "language_image_full_mime" => {
            let png_bytes = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
            let messages = vec![ProdiaLanguageMessage::User {
                content: vec![ProdiaLanguagePart::File {
                    media_type: "image".to_string(),
                    data: FileDataContent::Bytes(png_bytes.clone()),
                }],
            }];
            let (bytes, media_type) = prodia_language_image(&messages).expect("image part present");
            assert_eq!(bytes, png_bytes, "{case_id}");
            assert_eq!(media_type, "image/png", "{case_id}");
        }
        // Top-level-only "image" mediaType with undetectable bytes keeps default.
        "language_image_undetectable" => {
            let messages = vec![ProdiaLanguageMessage::User {
                content: vec![ProdiaLanguagePart::File {
                    media_type: "image".to_string(),
                    data: FileDataContent::Bytes(vec![0x00, 0x01, 0x02]),
                }],
            }];
            let (_bytes, media_type) =
                prodia_language_image(&messages).expect("image part present");
            assert_eq!(media_type, "image/png", "{case_id}");
        }
        // System message is prepended to the extracted user prompt.
        "language_system_message" => {
            let messages = vec![
                ProdiaLanguageMessage::System {
                    content: "You are an art critic.".to_string(),
                },
                ProdiaLanguageMessage::User {
                    content: vec![ProdiaLanguagePart::Text {
                        text: "Describe this.".to_string(),
                    }],
                },
            ];
            let prompt = prodia_language_prompt_text(&messages);
            assert_eq!(
                prompt, "You are an art critic.\nDescribe this.",
                "{case_id}"
            );
        }
        // Request config always carries include_messages: true.
        "language_include_messages" => {
            let body =
                prodia_language_request_body("inference.nano-banana.img2img.v2", "Hello", None);
            assert_eq!(body["config"]["include_messages"], json!(true), "{case_id}");
        }
        // aspectRatio provider option becomes config.aspect_ratio.
        "language_aspect_ratio" => {
            let options =
                prodia_language_model_options(&json!({ "aspectRatio": "16:9" })).expect("valid");
            let body = prodia_language_request_body(
                "inference.nano-banana.img2img.v2",
                "Describe",
                options.aspect_ratio.as_deref(),
            );
            assert_eq!(body["config"]["aspect_ratio"], json!("16:9"), "{case_id}");
            assert!(
                prodia_language_model_options(&json!({ "aspectRatio": "bogus" })).is_err(),
                "{case_id}"
            );
        }
        // Unsupported LLM features emit warnings (surfaced via doGenerate).
        "language_warnings" => {
            let flags = ProdiaLanguageCallFlags {
                temperature: true,
                top_p: true,
                top_k: true,
                max_output_tokens: true,
                stop_sequences: true,
                presence_penalty: true,
                frequency_penalty: true,
                tools: true,
                tool_choice: true,
                non_text_response_format: true,
                custom_reasoning: true,
            };
            let response = make_language_multipart_response(
                json!({ "id": "job-lang-123" }),
                Some("Done."),
                None,
            );
            let provider = make_static_provider(response);
            let result = poll_now(
                provider
                    .prodia_language_model("inference.nano-banana.img2img.v2")
                    .do_generate_content(language_user_text("Describe"), flags, None),
            );
            let features: Vec<&str> = result
                .warnings
                .iter()
                .filter_map(|warning| match warning {
                    Warning::Unsupported { feature, .. } => Some(feature.as_str()),
                    _ => None,
                })
                .collect();
            for expected in [
                "temperature",
                "topP",
                "topK",
                "maxOutputTokens",
                "stopSequences",
                "presencePenalty",
                "frequencyPenalty",
                "tools",
                "toolChoice",
                "responseFormat",
                "reasoning",
            ] {
                assert!(
                    features.contains(&expected),
                    "{case_id}: missing {expected}"
                );
            }
        }
        // Returns text content from a message.txt response part.
        "language_text_content" => {
            let response = make_language_multipart_response(
                json!({ "id": "job-lang-123" }),
                Some("This is a beautiful landscape."),
                Some(b"test-image-bytes"),
            );
            let provider = make_static_provider(response);
            let result = poll_now(
                provider
                    .prodia_language_model("inference.nano-banana.img2img.v2")
                    .do_generate_content(
                        language_user_text("Describe this image"),
                        ProdiaLanguageCallFlags::default(),
                        None,
                    ),
            );
            let texts: Vec<&str> = result
                .content
                .iter()
                .filter_map(|item| match item {
                    ProdiaLanguageContent::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(texts, vec!["This is a beautiful landscape."], "{case_id}");
        }
        // Returns image content from an image.png response part.
        "language_image_content" => {
            let response = make_language_multipart_response(
                json!({ "id": "job-lang-123" }),
                Some("This is a beautiful landscape."),
                Some(b"test-image-bytes"),
            );
            let provider = make_static_provider(response);
            let result = poll_now(
                provider
                    .prodia_language_model("inference.nano-banana.img2img.v2")
                    .do_generate_content(
                        language_user_text("Describe this image"),
                        ProdiaLanguageCallFlags::default(),
                        None,
                    ),
            );
            let files: Vec<(&str, &[u8])> = result
                .content
                .iter()
                .filter_map(|item| match item {
                    ProdiaLanguageContent::File { media_type, data } => {
                        Some((media_type.as_str(), data.as_slice()))
                    }
                    _ => None,
                })
                .collect();
            assert_eq!(files.len(), 1, "{case_id}");
            assert_eq!(files[0].0, "image/png", "{case_id}");
            assert_eq!(files[0].1, b"test-image-bytes", "{case_id}");
        }
        // Finish reason is always stop.
        "language_finish_reason" => {
            let response = make_language_multipart_response(
                json!({ "id": "job-lang-123" }),
                Some("Done."),
                None,
            );
            let provider = make_static_provider(response);
            let result = poll_now(
                provider
                    .prodia_language_model("inference.nano-banana.img2img.v2")
                    .do_generate_content(
                        language_user_text("Describe"),
                        ProdiaLanguageCallFlags::default(),
                        None,
                    ),
            );
            assert_eq!(result.finish_reason, "stop", "{case_id}");
        }
        // Provider metadata mirrors the job result fields.
        "language_provider_metadata" => {
            let response = make_language_multipart_response(
                json!({
                    "id": "job-lang-123",
                    "created_at": "2025-01-01T00:00:00Z",
                    "updated_at": "2025-01-01T00:00:03Z",
                    "config": { "seed": 7 },
                    "metrics": { "elapsed": 1.5, "ips": 20.0 },
                    "price": { "product": "nano-banana", "dollars": 0.01 }
                }),
                Some("Done."),
                None,
            );
            let provider = make_static_provider(response);
            let result = poll_now(
                provider
                    .prodia_language_model("inference.nano-banana.img2img.v2")
                    .do_generate_content(
                        language_user_text("Describe"),
                        ProdiaLanguageCallFlags::default(),
                        None,
                    ),
            );
            let metadata = result.provider_metadata.expect("provider metadata present");
            assert_eq!(
                metadata.get("jobId"),
                Some(&json!("job-lang-123")),
                "{case_id}"
            );
            assert_eq!(metadata.get("seed"), Some(&json!(7)), "{case_id}");
            assert_eq!(metadata.get("elapsed"), Some(&json!(1.5)), "{case_id}");
            assert_eq!(
                metadata.get("iterationsPerSecond"),
                Some(&json!(20.0)),
                "{case_id}"
            );
            assert_eq!(
                metadata.get("createdAt"),
                Some(&json!("2025-01-01T00:00:00Z")),
                "{case_id}"
            );
            assert_eq!(
                metadata.get("updatedAt"),
                Some(&json!("2025-01-01T00:00:03Z")),
                "{case_id}"
            );
            assert_eq!(metadata.get("dollars"), Some(&json!(0.01)), "{case_id}");
        }
        // Response metadata carries timestamp and modelId.
        "language_response_metadata" => {
            let response = make_language_multipart_response(
                json!({ "id": "job-lang-123" }),
                Some("Done."),
                None,
            );
            let provider = create_prodia(
                ProdiaProviderSettings::new()
                    .with_api_key("test-key")
                    .with_base_url("https://api.example.com/v2"),
            )
            .with_transport({
                let response = Arc::new(response);
                Arc::new(move |_request| {
                    let response = Arc::clone(&response);
                    Box::pin(ready(Ok((*response).clone())))
                })
            })
            .with_current_date(|| OffsetDateTime::from_unix_timestamp(1_748_736_000).unwrap());
            let result = poll_now(
                provider
                    .prodia_language_model("inference.nano-banana.img2img.v2")
                    .do_generate_content(
                        language_user_text("Describe"),
                        ProdiaLanguageCallFlags::default(),
                        None,
                    ),
            );
            assert_eq!(
                result.response_metadata.timestamp,
                OffsetDateTime::from_unix_timestamp(1_748_736_000).unwrap(),
                "{case_id}"
            );
            assert_eq!(
                result.response_metadata.model_id, "inference.nano-banana.img2img.v2",
                "{case_id}"
            );
        }
        // API errors surface the upstream `detail` message.
        "language_api_error" => {
            let response = ProviderApiResponse::text(
                400,
                "Bad Request",
                json!({ "message": "Bad request", "detail": "Missing input image" }).to_string(),
            );
            let provider = make_static_provider(response);
            let result = poll_now(
                provider
                    .prodia_language_model("inference.nano-banana.img2img.v2")
                    .do_generate_content(
                        language_user_text("Describe"),
                        ProdiaLanguageCallFlags::default(),
                        None,
                    ),
            );
            assert!(result.content.is_empty(), "{case_id}");
            assert_eq!(
                result.error.as_deref(),
                Some("Missing input image"),
                "{case_id}"
            );
        }
        // Text-only response yields a single text content item.
        "language_text_only" => {
            let response = make_language_multipart_response(
                json!({ "id": "job-lang-123" }),
                Some("Just a text response"),
                None,
            );
            let provider = make_static_provider(response);
            let result = poll_now(
                provider
                    .prodia_language_model("inference.nano-banana.img2img.v2")
                    .do_generate_content(
                        language_user_text("Describe"),
                        ProdiaLanguageCallFlags::default(),
                        None,
                    ),
            );
            assert_eq!(result.content.len(), 1, "{case_id}");
            assert!(
                matches!(result.content[0], ProdiaLanguageContent::Text { .. }),
                "{case_id}"
            );
        }
        // doStream wraps the doGenerate content into ordered stream parts.
        "language_stream_parts" => {
            let content = vec![
                ProdiaLanguageContent::Text {
                    text: "Stream test response".to_string(),
                },
                ProdiaLanguageContent::File {
                    media_type: "image/png".to_string(),
                    data: b"stream-image-bytes".to_vec(),
                },
            ];
            let parts = prodia_language_stream_parts(&content);
            assert_eq!(parts[0], ProdiaLanguageStreamPart::StreamStart, "{case_id}");
            assert_eq!(
                parts[1],
                ProdiaLanguageStreamPart::ResponseMetadata,
                "{case_id}"
            );
            assert_eq!(parts[2], ProdiaLanguageStreamPart::TextStart, "{case_id}");
            assert_eq!(
                parts[3],
                ProdiaLanguageStreamPart::TextDelta {
                    delta: "Stream test response".to_string()
                },
                "{case_id}"
            );
            assert_eq!(parts[4], ProdiaLanguageStreamPart::TextEnd, "{case_id}");
            assert_eq!(
                parts[5],
                ProdiaLanguageStreamPart::File {
                    media_type: "image/png".to_string()
                },
                "{case_id}"
            );
            assert_eq!(
                parts.last(),
                Some(&ProdiaLanguageStreamPart::Finish),
                "{case_id}"
            );
        }
        // Provider + request header merge for the language model.
        "language_headers_merge" => {
            let provider = create_prodia(
                ProdiaProviderSettings::new()
                    .with_api_key("test-key")
                    .with_header("Custom-Provider-Header", "provider-value"),
            );
            let model = provider.prodia_language_model("inference.nano-banana.img2img.v2");
            let mut call_headers = Headers::new();
            call_headers.insert(
                "Custom-Request-Header".to_string(),
                "request-value".to_string(),
            );
            let headers = model
                .request_headers(Some(&call_headers))
                .expect("headers build");
            assert_eq!(
                headers
                    .get("custom-provider-header")
                    .and_then(|value| value.clone()),
                Some("provider-value".to_string()),
                "{case_id}"
            );
            assert_eq!(
                headers
                    .get("Custom-Request-Header")
                    .and_then(|value| value.clone()),
                Some("request-value".to_string()),
                "{case_id}"
            );
            assert_eq!(
                headers.get("authorization").and_then(|value| value.clone()),
                Some("Bearer test-key".to_string()),
                "{case_id}"
            );
            assert_eq!(
                headers.get("Accept").and_then(|value| value.clone()),
                Some("multipart/form-data".to_string()),
                "{case_id}"
            );
        }
        other => panic!("unknown prodia upstream capability bucket: {other} ({case_id})"),
    }
}

/// Builds a Prodia language model multipart response containing a job JSON part
/// and optional `message.txt` (text) / `image.png` (binary) output parts,
/// mirroring the upstream test fixture.
fn make_language_multipart_response(
    job: serde_json::Value,
    text_content: Option<&str>,
    image_content: Option<&[u8]>,
) -> ProviderApiResponse {
    let boundary = "test-boundary-12345";
    let job = job.to_string();
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"job\"; filename=\"job.json\"\r\nContent-Type: application/json\r\n\r\n",
    );
    body.extend_from_slice(job.as_bytes());
    body.extend_from_slice(b"\r\n");
    if let Some(text) = text_content {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            b"Content-Disposition: form-data; name=\"output\"; filename=\"message.txt\"\r\nContent-Type: text/plain\r\n\r\n",
        );
        body.extend_from_slice(text.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    if let Some(image) = image_content {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            b"Content-Disposition: form-data; name=\"output\"; filename=\"image.png\"\r\nContent-Type: image/png\r\n\r\n",
        );
        body.extend_from_slice(image);
        body.extend_from_slice(b"\r\n");
    }
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

/// Builds a single user message containing one text part.
fn language_user_text(text: &str) -> Vec<ProdiaLanguageMessage> {
    vec![ProdiaLanguageMessage::User {
        content: vec![ProdiaLanguagePart::Text {
            text: text.to_string(),
        }],
    }]
}

/// Builds a deterministic provider whose transport always returns `response`.
fn make_static_provider(response: ProviderApiResponse) -> ProdiaProvider {
    let response = Arc::new(response);
    let transport: ProdiaTransport = Arc::new(move |_request| {
        let response = Arc::clone(&response);
        Box::pin(ready(Ok((*response).clone())))
    });
    create_prodia(
        ProdiaProviderSettings::new()
            .with_api_key("test-key")
            .with_base_url("https://api.example.com/v2"),
    )
    .with_transport(transport)
    .with_current_date(|| OffsetDateTime::from_unix_timestamp(0).unwrap())
}

/// Builds a multipart Prodia response containing a job JSON part and a binary
/// output part with the given media type.
fn make_multipart_response(
    media_type: &str,
    media_bytes: &[u8],
    job: serde_json::Value,
) -> ProviderApiResponse {
    let boundary = "boundary";
    let job = job.to_string();
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"job\"\r\nContent-Type: application/json\r\n\r\n",
    );
    body.extend_from_slice(job.as_bytes());
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"output\"\r\nContent-Type: {media_type}\r\n\r\n"
        )
        .as_bytes(),
    );
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

/// Polls a transport-backed future to completion using deterministic ready
/// transports (mirrors the colocated test harness).
fn poll_now<F>(future: F) -> F::Output
where
    F: Future,
{
    use std::sync::Arc as StdArc;
    use std::task::{Context, Poll, Wake, Waker};

    struct NoopWake;
    impl Wake for NoopWake {
        fn wake(self: StdArc<Self>) {}
    }

    let waker = Waker::from(StdArc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => unreachable!("upstream-mapping buckets use ready transports"),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ProdiaProviderSettings, ProdiaTransport, ProdiaTransportFuture, create_prodia,
        prodia_image_request_body, prodia_provider_metadata, prodia_video_request_body,
    };
    use ai_sdk_rust::{
        FileDataContent, ImageModel, ImageModelCallOptions, ProviderAbortController,
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
        ProviderOptions, VideoModel, VideoModelCallOptions, VideoModelFile,
    };
    use serde_json::json;
    use std::env;
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

    fn prodia_provider_options(value: serde_json::Value) -> ProviderOptions {
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "prodia".to_string(),
            serde_json::from_value(value).expect("provider options deserialize"),
        );
        provider_options
    }

    #[test]
    #[ignore = "requires PRODIA_TOKEN and performs live Prodia image/video generation"]
    fn live_prodia_image_and_video_generation_validate_provider_contract() {
        if env::var("PRODIA_TOKEN").is_err() {
            eprintln!("skipping live Prodia test: PRODIA_TOKEN is not set");
            return;
        }

        let provider = create_prodia(ProdiaProviderSettings::new());
        let image = poll_ready(
            provider
                .image("inference.sd3.txt2img.v2")
                .do_generate(ImageModelCallOptions::new(1).with_prompt("A small blue cube")),
        );
        let video = poll_ready(
            provider
                .video("inference.wan2-2.lightning.txt2vid.v0")
                .do_generate(VideoModelCallOptions::new(1).with_prompt("A small blue cube")),
        );

        assert!(!image.images.is_empty());
        assert!(!video.videos.is_empty());
    }

    #[test]
    fn prodia_image_model_maps_size_provider_options_loras_progressive_and_warnings() {
        let (body, warnings) = prodia_image_request_body(
            "inference.sd3.txt2img.v2",
            &ImageModelCallOptions::new(1)
                .with_prompt("A castle")
                .with_size("1024x768")
                .with_provider_options(prodia_provider_options(json!({
                    "width": 512,
                    "height": 512,
                    "stylePreset": "anime",
                    "loras": ["detail", "lighting"],
                    "progressive": true,
                    "steps": 4
                }))),
        )
        .expect("image body maps");
        let (_invalid_body, invalid_warnings) = prodia_image_request_body(
            "inference.sd3.txt2img.v2",
            &ImageModelCallOptions::new(1).with_size("wide"),
        )
        .expect("invalid size only warns");

        assert_eq!(
            body,
            json!({
                "type": "inference.sd3.txt2img.v2",
                "config": {
                    "prompt": "A castle",
                    "width": 512,
                    "height": 512,
                    "steps": 4,
                    "style_preset": "anime",
                    "loras": ["detail", "lighting"],
                    "progressive": true
                }
            })
        );
        assert!(warnings.is_empty());
        assert!(matches!(
            &invalid_warnings[0],
            ai_sdk_rust::Warning::Unsupported { feature, .. } if feature == "size"
        ));
    }

    #[test]
    fn prodia_video_model_maps_prompt_seed_resolution_and_json_shape() {
        let (body, warnings) = prodia_video_request_body(
            "inference.wan2-2.lightning.txt2vid.v0",
            &VideoModelCallOptions::new(1)
                .with_prompt("A wave")
                .with_seed(42)
                .with_provider_options(prodia_provider_options(json!({
                    "resolution": "720p"
                }))),
        )
        .expect("video body maps");

        assert_eq!(
            body,
            json!({
                "type": "inference.wan2-2.lightning.txt2vid.v0",
                "config": {
                    "prompt": "A wave",
                    "seed": 42,
                    "resolution": "720p"
                }
            })
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn prodia_provider_metadata_includes_and_omits_dollars_like_upstream() {
        let with_price = prodia_provider_metadata(
            serde_json::from_value(json!({
                "id": "job-with-price",
                "price": {"dollars": 0.01},
                "config": {"seed": 42},
                "metrics": {"elapsed": 1.5, "ips": 2.0}
            }))
            .expect("job result parses"),
        );
        let without_price = prodia_provider_metadata(
            serde_json::from_value(json!({
                "id": "job-without-price",
                "price": {}
            }))
            .expect("job result parses"),
        );
        let null_price = prodia_provider_metadata(
            serde_json::from_value(json!({
                "id": "job-null-price",
                "price": {"dollars": null}
            }))
            .expect("job result parses"),
        );

        assert_eq!(with_price.get("dollars"), Some(&json!(0.01)));
        assert_eq!(with_price.get("seed"), Some(&json!(42)));
        assert_eq!(with_price.get("iterationsPerSecond"), Some(&json!(2.0)));
        assert!(!without_price.contains_key("dollars"));
        assert!(!null_price.contains_key("dollars"));
    }

    #[test]
    fn prodia_image_model_respects_abort_signal_before_submit() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: ProdiaTransport = Arc::new(move |request| -> ProdiaTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request);
            Box::pin(ready(Ok(multipart_response(
                "boundary",
                "image/png",
                &[1, 2, 3],
            ))))
        });
        let provider = create_prodia(ProdiaProviderSettings::new().with_api_key("test-token"))
            .with_transport(transport);
        let abort_controller = ProviderAbortController::new();
        abort_controller.abort();

        let result = poll_ready(
            provider.image("inference.sd3.txt2img.v2").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("Abort")
                    .with_abort_signal(abort_controller.signal()),
            ),
        );

        assert!(result.images.is_empty());
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
                .and_then(|metadata| metadata.get("prodia"))
                .and_then(|metadata| metadata.extra.get("errorMessage")),
            Some(&json!("Aborted"))
        );
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
