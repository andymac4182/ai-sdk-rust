use std::collections::BTreeMap;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use ai_sdk_rust::{
    FetchErrorInfo, FileDataContent, GetFromApiOptions, HandledFetchError, Headers, ImageModel,
    ImageModelCallOptions, ImageModelFile, ImageModelProviderMetadata,
    ImageModelProviderMetadataEntry, ImageModelResponse, ImageModelResult, LoadApiKeyError,
    LoadApiKeyOptions, ModelType, NoSuchModelError, OpenAICompatibleChatLanguageModel,
    OpenAICompatibleEmbeddingModel, PostJsonToApiOptions, Provider, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, RuntimeEnvironment, Warning, combine_headers,
    create_binary_response_handler, create_json_error_response_handler,
    create_json_response_handler, create_status_code_error_response_handler, delay, get_from_api,
    load_api_key, parse_provider_options, post_json_to_api, with_user_agent_suffix,
    without_trailing_slash,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Default base URL for upstream `@ai-sdk/luma` API calls.
pub const DEFAULT_LUMA_BASE_URL: &str = "https://api.lumalabs.ai";

/// Default polling interval used by upstream Luma image generation.
pub const DEFAULT_LUMA_POLL_INTERVAL_MILLIS: u64 = 500;

/// Default number of polling attempts used by upstream Luma image generation.
pub const DEFAULT_LUMA_MAX_POLL_ATTEMPTS: u64 = 120;

/// Settings for the upstream Luma provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LumaProviderSettings {
    /// Luma API key. When omitted, `LUMA_API_KEY` is read at request time.
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

impl LumaProviderSettings {
    /// Creates empty Luma provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Luma API key.
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

/// Upstream Luma provider foundation.
#[derive(Clone)]
pub struct LumaProvider {
    base_url: String,
    settings: LumaProviderSettings,
    transport: LumaTransport,
    current_date: LumaDateProvider,
}

/// Luma image model.
#[derive(Clone)]
pub struct LumaImageModel {
    model_id: String,
    base_url: String,
    settings: LumaProviderSettings,
    transport: LumaTransport,
    current_date: LumaDateProvider,
}

/// Future returned by an injected Luma HTTP transport.
pub type LumaTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by Luma provider models.
pub type LumaTransport = Arc<dyn Fn(ProviderApiRequest) -> LumaTransportFuture + Send + Sync>;

type LumaDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type LumaImageGenerateFuture<'a> = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>;

impl LumaProvider {
    /// Creates a Luma provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(LumaProviderSettings::new())
    }

    /// Creates a provider from explicit Luma settings.
    pub fn from_settings(settings: LumaProviderSettings) -> Self {
        let base_url =
            without_trailing_slash(settings.base_url.as_deref().or(Some(DEFAULT_LUMA_BASE_URL)))
                .expect("default Luma base URL is present")
                .to_string();

        Self {
            base_url,
            settings,
            transport: default_luma_transport(),
            current_date: default_luma_date_provider(),
        }
    }

    /// Sets the Luma API key for this provider.
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
    pub fn with_transport(mut self, transport: LumaTransport) -> Self {
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
    pub fn image(&self, model_id: impl Into<String>) -> LumaImageModel {
        self.image_model(model_id)
            .expect("Luma image models are supported")
    }

    /// Creates an image model.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<LumaImageModel, NoSuchModelError> {
        Ok(LumaImageModel::new(
            model_id,
            self.base_url.clone(),
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Reports that Luma does not expose language models through this provider.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::LanguageModel))
    }

    /// Reports that Luma does not expose embedding models through this provider.
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

impl Default for LumaProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for LumaProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = LumaImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        LumaProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        LumaProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        LumaProvider::image_model(self, model_id)
    }
}

impl LumaImageModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: LumaProviderSettings,
        transport: LumaTransport,
        current_date: LumaDateProvider,
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
        "luma.image"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: LumaTransport) -> Self {
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

    async fn do_generate_result(&self, options: ImageModelCallOptions) -> ImageModelResult {
        let timestamp = (self.current_date)();
        let (request_body, warnings, poll_overrides) =
            match luma_image_request_body(&self.model_id, &options) {
                Ok(args) => args,
                Err(error) => {
                    return luma_image_result_from_error(
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
                return luma_image_result_from_error(
                    &self.model_id,
                    error.to_string(),
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let submit_options = PostJsonToApiOptions::new(self.generations_url(None), request_body)
            .with_headers(request_headers.clone())
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);
        let submit = match post_json_to_api(
            submit_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    luma_generation_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    luma_error_response,
                    luma_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                let (message, headers) = luma_handled_error_parts(error);
                return luma_image_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    warnings,
                    timestamp,
                );
            }
        };

        let image_url = match self
            .poll_for_image_url(&submit.value.id, request_headers, poll_overrides)
            .await
        {
            Ok(image_url) => image_url,
            Err(error) => {
                return luma_image_result_from_error(
                    &self.model_id,
                    error,
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let transport = Arc::clone(&self.transport);
        let image = match get_from_api(
            GetFromApiOptions::new(image_url).with_environment(RuntimeEnvironment::unknown()),
            move |request| (transport)(request),
            |request, response| {
                create_binary_response_handler(response.binary_response_handler_options(request))
                    .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                let (message, headers) = luma_handled_error_parts(error);
                return luma_image_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    warnings,
                    timestamp,
                );
            }
        };

        let mut result = ImageModelResult::new(
            vec![FileDataContent::Bytes(image.value)],
            luma_image_response_metadata(&self.model_id, submit.response_headers, timestamp),
        );

        for warning in warnings {
            result = result.with_warning(warning);
        }

        result
    }

    async fn poll_for_image_url(
        &self,
        generation_id: &str,
        headers: BTreeMap<String, Option<String>>,
        overrides: LumaPollOverrides,
    ) -> Result<String, String> {
        let poll_interval_millis = overrides
            .poll_interval_millis
            .unwrap_or(DEFAULT_LUMA_POLL_INTERVAL_MILLIS);
        let max_poll_attempts = overrides
            .max_poll_attempts
            .unwrap_or(DEFAULT_LUMA_MAX_POLL_ATTEMPTS);

        for attempt in 0..max_poll_attempts {
            let transport = Arc::clone(&self.transport);
            let response = get_from_api(
                GetFromApiOptions::new(self.generations_url(Some(generation_id)))
                    .with_headers(headers.clone())
                    .with_environment(RuntimeEnvironment::unknown()),
                move |request| (transport)(request),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        luma_generation_response,
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| {
                    Ok(create_json_error_response_handler(
                        response.json_error_response_handler_options(request),
                        luma_error_response,
                        luma_error_message,
                        |_, _| None,
                    ))
                },
            )
            .await
            .map_err(|error| luma_handled_error_parts(error).0)?;

            match response.value.into_image_url() {
                Ok(image_url) => return Ok(image_url),
                Err(LumaGenerationStatus::Failed) => {
                    return Err("Image generation failed.".to_string());
                }
                Err(LumaGenerationStatus::MissingImage) => {
                    return Err("Image generation completed but no image was found.".to_string());
                }
                Err(LumaGenerationStatus::Pending) => {
                    if attempt + 1 < max_poll_attempts {
                        delay(Some(poll_interval_millis as i64)).await;
                    }
                }
            }
        }

        Err(format!(
            "Image generation timed out after {DEFAULT_LUMA_MAX_POLL_ATTEMPTS} attempts."
        ))
    }

    fn generations_url(&self, generation_id: Option<&str>) -> String {
        format!(
            "{}/dream-machine/v1/generations/{}",
            self.base_url,
            generation_id.unwrap_or("image")
        )
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(luma_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
        ]))
    }
}

impl ImageModel for LumaImageModel {
    type MaxImagesPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = LumaImageGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        LumaImageModel::provider(self)
    }

    fn model_id(&self) -> &str {
        LumaImageModel::model_id(self)
    }

    fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
        ready(Some(1))
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates a Luma provider with explicit settings.
pub fn create_luma(settings: LumaProviderSettings) -> LumaProvider {
    LumaProvider::from_settings(settings)
}

/// Creates a Luma provider with default settings.
pub fn luma() -> LumaProvider {
    LumaProvider::new()
}

/// Type of image reference to use when providing input images.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LumaReferenceType {
    /// Guide generation using reference images.
    #[default]
    Image,
    /// Apply a specific style from reference image inputs.
    Style,
    /// Create consistent characters from reference image inputs.
    Character,
    /// Transform a single input image with prompt guidance.
    ModifyImage,
}

/// Per-image configuration for Luma image references.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LumaImageConfig {
    /// Weight of this image's influence on the generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f64>,

    /// Identity id for character references.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

/// Provider-specific image options accepted by upstream Luma.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LumaImageModelOptions {
    /// Type of image reference to use when input images are supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_type: Option<LumaReferenceType>,

    /// Per-image reference configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<LumaImageConfig>>,

    /// Per-call polling interval override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_interval_millis: Option<u64>,

    /// Per-call maximum polling attempts override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_poll_attempts: Option<u64>,

    /// Additional Luma request options passed through to the API.
    #[serde(flatten)]
    pub extra: ai_sdk_rust::JsonObject,
}

impl LumaImageModelOptions {
    fn validate(&self) -> Result<(), &'static str> {
        for image in self.images.iter().flatten() {
            if image
                .weight
                .is_some_and(|weight| !(0.0..=1.0).contains(&weight))
            {
                return Err("image weights must be between 0 and 1");
            }
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct LumaPollOverrides {
    poll_interval_millis: Option<u64>,
    max_poll_attempts: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct LumaGenerationResponse {
    id: String,
    state: String,
    #[serde(default)]
    assets: Option<LumaGenerationAssets>,
    #[serde(default)]
    failure_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct LumaGenerationAssets {
    #[serde(default)]
    image: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LumaGenerationStatus {
    Pending,
    Failed,
    MissingImage,
}

impl LumaGenerationResponse {
    fn into_image_url(self) -> Result<String, LumaGenerationStatus> {
        match self.state.as_str() {
            "completed" => self
                .assets
                .and_then(|assets| assets.image)
                .ok_or(LumaGenerationStatus::MissingImage),
            "failed" => Err(LumaGenerationStatus::Failed),
            _ => Err(LumaGenerationStatus::Pending),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct LumaErrorResponse {
    detail: Vec<LumaErrorDetail>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct LumaErrorDetail {
    msg: String,
}

fn luma_image_request_body(
    model_id: &str,
    options: &ImageModelCallOptions,
) -> Result<(ai_sdk_rust::JsonValue, Vec<Warning>, LumaPollOverrides), String> {
    let mut warnings = Vec::new();
    let provider_options = parse_provider_options(
        "luma",
        Some(&options.provider_options),
        luma_image_model_options,
    )
    .map_err(|error| error.to_string())?
    .unwrap_or_default();
    let editing_options = luma_editing_options(
        options.files.as_deref(),
        options.mask.as_ref(),
        provider_options.reference_type.unwrap_or_default(),
        provider_options.images.as_deref().unwrap_or_default(),
    )?;
    let mut body = ai_sdk_rust::JsonObject::new();

    if options.seed.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "seed".to_string(),
            details: Some("This model does not support the `seed` option.".to_string()),
        });
    }

    if options.size.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "size".to_string(),
            details: Some(
                "This model does not support the `size` option. Use `aspectRatio` instead."
                    .to_string(),
            ),
        });
    }

    if let Some(prompt) = options.prompt.as_ref() {
        body.insert(
            "prompt".to_string(),
            ai_sdk_rust::JsonValue::String(prompt.clone()),
        );
    }

    if let Some(aspect_ratio) = options.aspect_ratio.as_ref() {
        body.insert(
            "aspect_ratio".to_string(),
            ai_sdk_rust::JsonValue::String(aspect_ratio.clone()),
        );
    }

    body.insert(
        "model".to_string(),
        ai_sdk_rust::JsonValue::String(model_id.to_string()),
    );
    body.extend(editing_options);
    body.extend(provider_options.extra.clone());

    Ok((
        ai_sdk_rust::JsonValue::Object(body),
        warnings,
        LumaPollOverrides {
            poll_interval_millis: provider_options.poll_interval_millis,
            max_poll_attempts: provider_options.max_poll_attempts,
        },
    ))
}

fn luma_image_model_options(
    value: &ai_sdk_rust::JsonValue,
) -> Result<LumaImageModelOptions, String> {
    let options = serde_json::from_value::<LumaImageModelOptions>(value.clone())
        .map_err(|error| error.to_string())?;
    options.validate().map_err(str::to_string)?;
    Ok(options)
}

fn luma_editing_options(
    files: Option<&[ImageModelFile]>,
    mask: Option<&ImageModelFile>,
    reference_type: LumaReferenceType,
    image_configs: &[LumaImageConfig],
) -> Result<ai_sdk_rust::JsonObject, String> {
    let mut options = ai_sdk_rust::JsonObject::new();

    if mask.is_some() {
        return Err(
            "Luma AI does not support mask-based image editing. Use the prompt to describe the changes you want to make, along with `prompt.images` containing the source image URL."
                .to_string(),
        );
    }

    let Some(files) = files else {
        return Ok(options);
    };

    if files.is_empty() {
        return Ok(options);
    }

    let urls = files
        .iter()
        .map(luma_url_file)
        .collect::<Result<Vec<_>, _>>()?;

    match reference_type {
        LumaReferenceType::Image => {
            if urls.len() > 4 {
                return Err(format!(
                    "Luma AI image supports up to 4 reference images. You provided {} images.",
                    urls.len()
                ));
            }
            options.insert(
                "image".to_string(),
                ai_sdk_rust::JsonValue::Array(
                    urls.into_iter()
                        .enumerate()
                        .map(|(index, url)| {
                            weighted_url_object(
                                url,
                                image_configs
                                    .get(index)
                                    .and_then(|config| config.weight)
                                    .unwrap_or(0.85),
                            )
                        })
                        .collect(),
                ),
            );
        }
        LumaReferenceType::Style => {
            options.insert(
                "style".to_string(),
                ai_sdk_rust::JsonValue::Array(
                    urls.into_iter()
                        .enumerate()
                        .map(|(index, url)| {
                            weighted_url_object(
                                url,
                                image_configs
                                    .get(index)
                                    .and_then(|config| config.weight)
                                    .unwrap_or(0.8),
                            )
                        })
                        .collect(),
                ),
            );
        }
        LumaReferenceType::Character => {
            let mut identities = BTreeMap::<String, Vec<String>>::new();

            for (index, url) in urls.into_iter().enumerate() {
                let identity_id = image_configs
                    .get(index)
                    .and_then(|config| config.id.clone())
                    .unwrap_or_else(|| "identity0".to_string());
                identities.entry(identity_id).or_default().push(url);
            }

            for (identity_id, images) in &identities {
                if images.len() > 4 {
                    return Err(format!(
                        "Luma AI character supports up to 4 images per identity. Identity '{identity_id}' has {} images.",
                        images.len()
                    ));
                }
            }

            options.insert(
                "character".to_string(),
                ai_sdk_rust::JsonValue::Object(
                    identities
                        .into_iter()
                        .map(|(identity_id, images)| {
                            let mut identity = ai_sdk_rust::JsonObject::new();
                            identity.insert(
                                "images".to_string(),
                                ai_sdk_rust::JsonValue::Array(
                                    images
                                        .into_iter()
                                        .map(ai_sdk_rust::JsonValue::String)
                                        .collect(),
                                ),
                            );
                            (identity_id, ai_sdk_rust::JsonValue::Object(identity))
                        })
                        .collect(),
                ),
            );
        }
        LumaReferenceType::ModifyImage => {
            if urls.len() > 1 {
                return Err(format!(
                    "Luma AI modify_image only supports a single input image. You provided {} images.",
                    urls.len()
                ));
            }
            let url = urls.into_iter().next().expect("non-empty input image list");
            options.insert(
                "modify_image".to_string(),
                weighted_url_object(
                    url,
                    image_configs
                        .first()
                        .and_then(|config| config.weight)
                        .unwrap_or(1.0),
                ),
            );
        }
    }

    Ok(options)
}

fn weighted_url_object(url: String, weight: f64) -> ai_sdk_rust::JsonValue {
    let mut object = ai_sdk_rust::JsonObject::new();
    object.insert("url".to_string(), ai_sdk_rust::JsonValue::String(url));
    object.insert("weight".to_string(), ai_sdk_rust::JsonValue::from(weight));
    ai_sdk_rust::JsonValue::Object(object)
}

fn luma_url_file(file: &ImageModelFile) -> Result<String, String> {
    match file {
        ImageModelFile::Url { url, .. } => Ok(url.as_str().to_string()),
        ImageModelFile::File { .. } => Err(
            "Luma AI only supports URL-based images. Please provide image URLs using `prompt.images` with publicly accessible URLs. Base64 and Uint8Array data are not supported."
                .to_string(),
        ),
    }
}

fn luma_provider_header_entries(
    settings: &LumaProviderSettings,
) -> Result<Vec<(String, Option<String>)>, LoadApiKeyError> {
    let mut headers = vec![(
        "authorization".to_string(),
        Some(format!(
            "Bearer {}",
            luma_api_key(settings.api_key.as_ref())?
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
        [format!("ai-sdk/luma/{}", ai_sdk_rust::VERSION)],
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

fn luma_api_key(explicit_api_key: Option<&String>) -> Result<String, LoadApiKeyError> {
    let mut options = LoadApiKeyOptions::new("LUMA_API_KEY", "Luma");

    if let Some(api_key) = explicit_api_key {
        options = options.with_api_key(api_key.clone());
    }

    load_api_key(options)
}

fn luma_generation_response(
    value: &ai_sdk_rust::JsonValue,
) -> Result<LumaGenerationResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn luma_error_response(
    value: &ai_sdk_rust::JsonValue,
) -> Result<LumaErrorResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn luma_error_message(value: &LumaErrorResponse) -> String {
    value
        .detail
        .first()
        .map(|detail| detail.msg.clone())
        .unwrap_or_else(|| "Unknown error".to_string())
}

fn luma_handled_error_parts(error: HandledFetchError) -> (String, Option<Headers>) {
    match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
        ),
    }
}

fn luma_image_result_from_error(
    model_id: &str,
    message: String,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> ImageModelResult {
    let mut result = ImageModelResult::new(
        Vec::new(),
        luma_image_response_metadata(model_id, headers, timestamp),
    )
    .with_provider_metadata(luma_image_error_metadata(message));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn luma_image_response_metadata(
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

fn luma_image_error_metadata(message: String) -> ImageModelProviderMetadata {
    let mut extra = ai_sdk_rust::JsonObject::new();
    extra.insert(
        "errorMessage".to_string(),
        ai_sdk_rust::JsonValue::String(message),
    );

    ImageModelProviderMetadata::from([(
        "luma".to_string(),
        ImageModelProviderMetadataEntry {
            images: Vec::new(),
            extra,
        },
    )])
}

fn default_luma_date_provider() -> LumaDateProvider {
    Arc::new(OffsetDateTime::now_utc)
}

fn default_luma_transport() -> LumaTransport {
    Arc::new(|request| Box::pin(ready(execute_luma_request(request))))
}

fn execute_luma_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_luma_get_request(request),
        ProviderApiRequestMethod::Post => execute_luma_post_request(request),
    }
}

fn execute_luma_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    luma_provider_api_response(response)
}

fn execute_luma_post_request(
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
                "multipart form data is not supported by the Luma transport",
            ));
        }
        None => builder.send_empty(),
    };

    luma_provider_api_response(response)
}

fn luma_provider_api_response(
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
        DEFAULT_LUMA_BASE_URL, LumaProviderSettings, LumaTransport, LumaTransportFuture,
        create_luma, luma,
    };
    use ai_sdk_rust::{
        FileDataContent, ImageModel, ImageModelCallOptions, ImageModelFile, ModelType, Provider,
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
        ProviderOptions, Warning,
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

    fn luma_success_transport() -> (Arc<Mutex<Vec<ProviderApiRequest>>>, LumaTransport) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: LumaTransport = Arc::new(move |request| -> LumaTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());

            let response = match (request.method, request.url.as_str()) {
                (
                    ProviderApiRequestMethod::Post,
                    "https://api.example.com/dream-machine/v1/generations/image",
                ) => json_response(json!({
                    "id": "generation-123",
                    "state": "queued"
                }))
                .with_headers(
                    [("x-generation-id".to_string(), "generation-123".to_string())]
                        .into_iter()
                        .collect(),
                ),
                (
                    ProviderApiRequestMethod::Get,
                    "https://api.example.com/dream-machine/v1/generations/generation-123",
                ) => json_response(json!({
                    "id": "generation-123",
                    "state": "completed",
                    "assets": {
                        "image": "https://api.example.com/image.png"
                    }
                })),
                (ProviderApiRequestMethod::Get, "https://api.example.com/image.png") => {
                    ProviderApiResponse::bytes(200, "OK", vec![1, 2, 3])
                }
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"detail": [{"msg": "unexpected request"}]}).to_string(),
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
    fn luma_provider_creates_image_model_with_headers_body_and_metadata() {
        let (requests, transport) = luma_success_transport();
        let provider = create_luma(
            LumaProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/")
                .with_header("x-provider-header", "provider"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "luma".to_string(),
            serde_json::from_value(json!({
                "additional_param": "value",
                "pollIntervalMillis": 1,
                "maxPollAttempts": 1
            }))
            .expect("object provider options deserialize"),
        );

        let result = poll_ready(
            provider.image("photon-1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A serene mountain lake")
                    .with_aspect_ratio("16:9")
                    .with_provider_options(provider_options)
                    .with_header("x-request-header", "request"),
            ),
        );

        assert_eq!(result.images, vec![FileDataContent::Bytes(vec![1, 2, 3])]);
        assert_eq!(result.response.model_id, "photon-1");
        assert_eq!(result.response.timestamp, fixed_timestamp());
        assert_eq!(
            result
                .response
                .headers
                .expect("generation response headers")
                .get("x-generation-id"),
            Some(&"generation-123".to_string())
        );

        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method, ProviderApiRequestMethod::Post);
        assert_eq!(
            requests[0].url,
            "https://api.example.com/dream-machine/v1/generations/image"
        );
        assert_eq!(
            requests[0].headers.get("authorization"),
            Some(&"Bearer test-api-key".to_string())
        );
        assert_eq!(
            requests[0].headers.get("x-provider-header"),
            Some(&"provider".to_string())
        );
        assert_eq!(
            requests[0].headers.get("x-request-header"),
            Some(&"request".to_string())
        );
        assert!(
            requests[0]
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/luma/"))
        );
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "additional_param": "value",
                "aspect_ratio": "16:9",
                "model": "photon-1",
                "prompt": "A serene mountain lake"
            })
        );
        assert_eq!(requests[1].method, ProviderApiRequestMethod::Get);
        assert_eq!(
            requests[1].url,
            "https://api.example.com/dream-machine/v1/generations/generation-123"
        );
        assert_eq!(requests[2].method, ProviderApiRequestMethod::Get);
        assert_eq!(requests[2].url, "https://api.example.com/image.png");
        assert!(!requests[2].headers.contains_key("authorization"));
        assert!(!requests[2].headers.contains_key("x-provider-header"));
        assert!(!requests[2].headers.contains_key("x-request-header"));
    }

    #[test]
    fn luma_image_model_maps_reference_images_and_warnings() {
        let (requests, transport) = luma_success_transport();
        let provider = create_luma(
            LumaProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com"),
        )
        .with_transport(transport);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "luma".to_string(),
            serde_json::from_value(json!({
                "referenceType": "character",
                "images": [
                    {"id": "identity0"},
                    {"id": "identity1"}
                ],
                "pollIntervalMillis": 1,
                "maxPollAttempts": 1
            }))
            .expect("object provider options deserialize"),
        );

        let result = poll_ready(
            provider.image("photon-1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("Two people talking")
                    .with_size("1024x1024")
                    .with_seed(42)
                    .with_files(vec![
                        ImageModelFile::url(
                            Url::parse("https://example.com/person1.jpg").expect("valid URL"),
                        ),
                        ImageModelFile::url(
                            Url::parse("https://example.com/person2.jpg").expect("valid URL"),
                        ),
                    ])
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(
            result.warnings,
            vec![
                Warning::Unsupported {
                    feature: "seed".to_string(),
                    details: Some("This model does not support the `seed` option.".to_string()),
                },
                Warning::Unsupported {
                    feature: "size".to_string(),
                    details: Some(
                        "This model does not support the `size` option. Use `aspectRatio` instead."
                            .to_string(),
                    ),
                },
            ]
        );
        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "character": {
                    "identity0": {
                        "images": ["https://example.com/person1.jpg"]
                    },
                    "identity1": {
                        "images": ["https://example.com/person2.jpg"]
                    }
                },
                "model": "photon-1",
                "prompt": "Two people talking"
            })
        );
    }

    #[test]
    fn luma_image_model_reports_editing_validation_errors_to_metadata() {
        let (_requests, transport) = luma_success_transport();
        let provider = create_luma(
            LumaProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com"),
        )
        .with_transport(transport);

        let result = poll_ready(
            provider.image("photon-1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("Edit this")
                    .with_files(vec![ImageModelFile::file(
                        "image/png",
                        FileDataContent::Bytes(vec![137, 80, 78, 71]),
                    )]),
            ),
        );

        assert!(result.images.is_empty());
        let metadata = result
            .provider_metadata
            .expect("provider metadata")
            .remove("luma")
            .expect("Luma metadata");
        assert_eq!(metadata.images, Vec::<ai_sdk_rust::JsonValue>::new());
        assert!(metadata.extra.get("errorMessage").is_some_and(|message| {
            message
                .as_str()
                .is_some_and(|message| message.contains("Luma AI only supports URL-based images"))
        }));
    }

    #[test]
    fn luma_image_model_maps_api_and_status_errors_to_metadata() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: LumaTransport = Arc::new(move |request| -> LumaTransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());

            let response = match (request.method, request.url.as_str()) {
                (
                    ProviderApiRequestMethod::Post,
                    "https://api.example.com/dream-machine/v1/generations/image",
                ) => json_response(json!({
                    "id": "failed-generation",
                    "state": "queued"
                })),
                (
                    ProviderApiRequestMethod::Get,
                    "https://api.example.com/dream-machine/v1/generations/failed-generation",
                ) => json_response(json!({
                    "id": "failed-generation",
                    "state": "failed",
                    "failure_reason": "bad prompt"
                })),
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"detail": [{"msg": "unexpected request"}]}).to_string(),
                ),
            };

            Box::pin(ready(Ok(response)))
        });
        let provider = create_luma(
            LumaProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com"),
        )
        .with_transport(transport);

        let result = poll_ready(
            provider
                .image("photon-1")
                .do_generate(ImageModelCallOptions::new(1).with_prompt("Invalid prompt")),
        );

        assert!(result.images.is_empty());
        let metadata = result
            .provider_metadata
            .expect("provider metadata")
            .remove("luma")
            .expect("Luma metadata");
        assert_eq!(
            metadata.extra.get("errorMessage"),
            Some(&json!("Image generation failed."))
        );
        assert_eq!(
            requests
                .lock()
                .expect("request list mutex is not poisoned")
                .len(),
            2
        );
    }

    #[test]
    fn luma_provider_reports_unsupported_model_families_and_trait_image() {
        let provider = luma();
        let language_error = match provider.language_model("some-model") {
            Ok(_) => panic!("language models are unsupported"),
            Err(error) => error,
        };
        let embedding_error = match provider.embedding_model("some-model") {
            Ok(_) => panic!("embedding models are unsupported"),
            Err(error) => error,
        };

        assert_eq!(language_error.model_type(), ModelType::LanguageModel);
        assert_eq!(embedding_error.model_type(), ModelType::EmbeddingModel);
        assert_eq!(provider.specification_version().as_str(), "v4");
        assert_eq!(provider.image("photon-1").provider(), "luma.image");

        let trait_image_model = Provider::image_model(&provider, "photon-1")
            .expect("Provider trait creates image model");
        assert_eq!(trait_image_model.model_id(), "photon-1");
    }

    #[test]
    fn luma_provider_settings_serde_accepts_upstream_shape() {
        let settings: LumaProviderSettings = serde_json::from_value(json!({
            "apiKey": "key",
            "baseURL": "https://api.example.com/",
            "headers": {
                "x-extra": "1"
            }
        }))
        .expect("settings deserialize");
        let provider = create_luma(settings.clone());

        assert_eq!(settings.api_key.as_deref(), Some("key"));
        assert_eq!(
            settings.base_url.as_deref(),
            Some("https://api.example.com/")
        );
        assert_eq!(settings.headers.get("x-extra"), Some(&"1".to_string()));
        assert_eq!(provider.base_url, "https://api.example.com");
        assert_eq!(DEFAULT_LUMA_BASE_URL, "https://api.lumalabs.ai");
    }
}
