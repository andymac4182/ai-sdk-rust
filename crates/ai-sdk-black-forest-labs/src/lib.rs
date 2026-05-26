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
    convert_to_base64, create_binary_response_handler, create_json_error_response_handler,
    create_json_response_handler, create_status_code_error_response_handler, delay, get_from_api,
    load_api_key, parse_provider_options, post_json_to_api, with_user_agent_suffix,
    without_trailing_slash,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use url::Url;

/// Default base URL for upstream `@ai-sdk/black-forest-labs` API calls.
pub const DEFAULT_BLACK_FOREST_LABS_BASE_URL: &str = "https://api.bfl.ai/v1";

/// Default polling interval used by upstream Black Forest Labs image generation.
pub const DEFAULT_BLACK_FOREST_LABS_POLL_INTERVAL_MILLIS: u64 = 500;

/// Default polling timeout used by upstream Black Forest Labs image generation.
pub const DEFAULT_BLACK_FOREST_LABS_POLL_TIMEOUT_MILLIS: u64 = 60_000;

/// Settings for the upstream Black Forest Labs provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlackForestLabsProviderSettings {
    /// Black Forest Labs API key. When omitted, `BFL_API_KEY` is read at request time.
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

    /// Poll interval in milliseconds between status checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_interval_millis: Option<u64>,

    /// Overall timeout in milliseconds before polling gives up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_timeout_millis: Option<u64>,
}

impl BlackForestLabsProviderSettings {
    /// Creates empty Black Forest Labs provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Black Forest Labs API key.
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

    /// Sets the polling interval in milliseconds.
    pub fn with_poll_interval_millis(mut self, poll_interval_millis: u64) -> Self {
        self.poll_interval_millis = Some(poll_interval_millis);
        self
    }

    /// Sets the polling timeout in milliseconds.
    pub fn with_poll_timeout_millis(mut self, poll_timeout_millis: u64) -> Self {
        self.poll_timeout_millis = Some(poll_timeout_millis);
        self
    }
}

/// Upstream Black Forest Labs provider foundation.
#[derive(Clone)]
pub struct BlackForestLabsProvider {
    base_url: String,
    settings: BlackForestLabsProviderSettings,
    transport: BlackForestLabsTransport,
    current_date: BlackForestLabsDateProvider,
}

/// Black Forest Labs image model.
#[derive(Clone)]
pub struct BlackForestLabsImageModel {
    model_id: String,
    base_url: String,
    settings: BlackForestLabsProviderSettings,
    transport: BlackForestLabsTransport,
    current_date: BlackForestLabsDateProvider,
}

/// Future returned by an injected Black Forest Labs HTTP transport.
pub type BlackForestLabsTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by Black Forest Labs provider models.
pub type BlackForestLabsTransport =
    Arc<dyn Fn(ProviderApiRequest) -> BlackForestLabsTransportFuture + Send + Sync>;

type BlackForestLabsDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type BlackForestLabsImageGenerateFuture<'a> =
    Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>;

impl BlackForestLabsProvider {
    /// Creates a Black Forest Labs provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(BlackForestLabsProviderSettings::new())
    }

    /// Creates a provider from explicit Black Forest Labs settings.
    pub fn from_settings(settings: BlackForestLabsProviderSettings) -> Self {
        let base_url = without_trailing_slash(
            settings
                .base_url
                .as_deref()
                .or(Some(DEFAULT_BLACK_FOREST_LABS_BASE_URL)),
        )
        .expect("default Black Forest Labs base URL is present")
        .to_string();

        Self {
            base_url,
            settings,
            transport: default_black_forest_labs_transport(),
            current_date: default_black_forest_labs_date_provider(),
        }
    }

    /// Sets the Black Forest Labs API key for this provider.
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
    pub fn with_transport(mut self, transport: BlackForestLabsTransport) -> Self {
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
    pub fn image(&self, model_id: impl Into<String>) -> BlackForestLabsImageModel {
        self.image_model(model_id)
            .expect("Black Forest Labs image models are supported")
    }

    /// Creates an image model.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<BlackForestLabsImageModel, NoSuchModelError> {
        Ok(BlackForestLabsImageModel::new(
            model_id,
            self.base_url.clone(),
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Reports that Black Forest Labs does not expose language models through this provider.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::LanguageModel))
    }

    /// Reports that Black Forest Labs does not expose embedding models through this provider.
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

impl Default for BlackForestLabsProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for BlackForestLabsProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = BlackForestLabsImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        BlackForestLabsProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        BlackForestLabsProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        BlackForestLabsProvider::image_model(self, model_id)
    }
}

impl BlackForestLabsImageModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: BlackForestLabsProviderSettings,
        transport: BlackForestLabsTransport,
        current_date: BlackForestLabsDateProvider,
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
        "black-forest-labs.image"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: BlackForestLabsTransport) -> Self {
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
            match black_forest_labs_image_request_body(&options) {
                Ok(args) => args,
                Err(error) => {
                    return black_forest_labs_image_result_from_error(
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
                return black_forest_labs_image_result_from_error(
                    &self.model_id,
                    error.to_string(),
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let submit_options = PostJsonToApiOptions::new(self.image_model_url(), request_body)
            .with_headers(request_headers.clone())
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        let submit = match post_json_to_api(
            submit_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    black_forest_labs_submit_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    black_forest_labs_error_response,
                    black_forest_labs_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                let (message, headers) = black_forest_labs_handled_error_parts(error);
                return black_forest_labs_image_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    warnings,
                    timestamp,
                );
            }
        };

        let poll = match self
            .poll_for_image_url(
                &submit.value.polling_url,
                &submit.value.id,
                request_headers.clone(),
                poll_overrides,
            )
            .await
        {
            Ok(poll) => poll,
            Err(error) => {
                return black_forest_labs_image_result_from_error(
                    &self.model_id,
                    error,
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let transport = Arc::clone(&self.transport);
        let image_options = GetFromApiOptions::new(poll.image_url.clone())
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let image = match get_from_api(
            image_options,
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
                let (message, headers) = black_forest_labs_handled_error_parts(error);
                return black_forest_labs_image_result_from_error(
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
            black_forest_labs_image_response_metadata(
                &self.model_id,
                image.response_headers,
                timestamp,
            ),
        )
        .with_provider_metadata(black_forest_labs_image_provider_metadata(
            &submit.value,
            &poll,
        ));

        for warning in warnings {
            result = result.with_warning(warning);
        }

        result
    }

    async fn poll_for_image_url(
        &self,
        poll_url: &str,
        request_id: &str,
        headers: BTreeMap<String, Option<String>>,
        overrides: BlackForestLabsPollOverrides,
    ) -> Result<BlackForestLabsPollReady, String> {
        let poll_interval_millis = overrides
            .poll_interval_millis
            .or(self.settings.poll_interval_millis)
            .unwrap_or(DEFAULT_BLACK_FOREST_LABS_POLL_INTERVAL_MILLIS);
        let poll_timeout_millis = overrides
            .poll_timeout_millis
            .or(self.settings.poll_timeout_millis)
            .unwrap_or(DEFAULT_BLACK_FOREST_LABS_POLL_TIMEOUT_MILLIS);
        let max_poll_attempts = (poll_timeout_millis / poll_interval_millis.max(1))
            + u64::from(poll_timeout_millis % poll_interval_millis.max(1) != 0);
        let mut url = Url::parse(poll_url)
            .map_err(|error| format!("Invalid Black Forest Labs polling URL: {error}"))?;

        if !url.query_pairs().any(|(name, _)| name == "id") {
            url.query_pairs_mut().append_pair("id", request_id);
        }

        for attempt in 0..max_poll_attempts {
            let transport = Arc::clone(&self.transport);
            let options = GetFromApiOptions::new(url.as_str())
                .with_headers(headers.clone())
                .with_environment(RuntimeEnvironment::unknown());
            let response = get_from_api(
                options,
                move |request| (transport)(request),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        black_forest_labs_poll_response,
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| {
                    Ok(create_json_error_response_handler(
                        response.json_error_response_handler_options(request),
                        black_forest_labs_error_response,
                        black_forest_labs_error_message,
                        |_, _| None,
                    ))
                },
            )
            .await
            .map_err(|error| black_forest_labs_handled_error_parts(error).0)?;

            match response.value.into_ready() {
                Ok(ready) => return Ok(ready),
                Err(BlackForestLabsPollStatus::Pending) => {
                    if attempt + 1 < max_poll_attempts {
                        delay(Some(poll_interval_millis as i64)).await;
                    }
                }
                Err(BlackForestLabsPollStatus::Failed) => {
                    return Err("Black Forest Labs generation failed.".to_string());
                }
                Err(BlackForestLabsPollStatus::MissingSample) => {
                    return Err(
                        "Black Forest Labs poll response is Ready but missing result.sample"
                            .to_string(),
                    );
                }
                Err(BlackForestLabsPollStatus::MissingStatus) => {
                    return Err("Missing status in Black Forest Labs poll response".to_string());
                }
            }
        }

        Err("Black Forest Labs generation timed out.".to_string())
    }

    fn image_model_url(&self) -> String {
        format!("{}/{}", self.base_url, self.model_id)
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<BTreeMap<String, Option<String>>, LoadApiKeyError> {
        Ok(combine_headers([
            Some(black_forest_labs_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
        ]))
    }
}

impl ImageModel for BlackForestLabsImageModel {
    type MaxImagesPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = BlackForestLabsImageGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        BlackForestLabsImageModel::provider(self)
    }

    fn model_id(&self) -> &str {
        BlackForestLabsImageModel::model_id(self)
    }

    fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
        ready(Some(1))
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates a Black Forest Labs provider with explicit settings.
pub fn create_black_forest_labs(
    settings: BlackForestLabsProviderSettings,
) -> BlackForestLabsProvider {
    BlackForestLabsProvider::from_settings(settings)
}

/// Creates a Black Forest Labs provider with default settings.
pub fn black_forest_labs() -> BlackForestLabsProvider {
    BlackForestLabsProvider::new()
}

/// Provider-specific image options accepted by upstream Black Forest Labs.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlackForestLabsImageModelOptions {
    /// Deprecated upstream image prompt option.
    #[serde(default)]
    pub image_prompt: Option<String>,

    /// Deprecated upstream image prompt strength.
    #[serde(default)]
    pub image_prompt_strength: Option<f64>,

    /// Number of generation steps.
    #[serde(default)]
    pub steps: Option<u64>,

    /// Guidance strength.
    #[serde(default)]
    pub guidance: Option<f64>,

    /// Explicit image width.
    #[serde(default)]
    pub width: Option<u64>,

    /// Explicit image height.
    #[serde(default)]
    pub height: Option<u64>,

    /// Output image format.
    #[serde(default)]
    pub output_format: Option<String>,

    /// Whether to enable prompt upsampling.
    #[serde(default)]
    pub prompt_upsampling: Option<bool>,

    /// Whether to use raw mode.
    #[serde(default)]
    pub raw: Option<bool>,

    /// Safety tolerance.
    #[serde(default)]
    pub safety_tolerance: Option<u64>,

    /// Webhook secret.
    #[serde(default)]
    pub webhook_secret: Option<String>,

    /// Webhook URL.
    #[serde(default)]
    pub webhook_url: Option<String>,

    /// Per-call polling interval override.
    #[serde(default)]
    pub poll_interval_millis: Option<u64>,

    /// Per-call polling timeout override.
    #[serde(default)]
    pub poll_timeout_millis: Option<u64>,
}

impl BlackForestLabsImageModelOptions {
    fn validate(&self) -> Result<(), &'static str> {
        if self
            .image_prompt_strength
            .is_some_and(|value| !(0.0..=1.0).contains(&value))
        {
            return Err("imagePromptStrength must be between 0 and 1");
        }

        if self.steps.is_some_and(|value| value == 0) {
            return Err("steps must be positive");
        }

        if self.guidance.is_some_and(|value| value < 0.0) {
            return Err("guidance must be greater than or equal to 0");
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

        if let Some(output_format) = self.output_format.as_deref() {
            if !matches!(output_format, "jpeg" | "png") {
                return Err("outputFormat must be jpeg or png");
            }
        }

        if self
            .safety_tolerance
            .is_some_and(|value| !(0..=6).contains(&value))
        {
            return Err("safetyTolerance must be between 0 and 6");
        }

        if self.poll_interval_millis.is_some_and(|value| value == 0) {
            return Err("pollIntervalMillis must be positive");
        }

        if self.poll_timeout_millis.is_some_and(|value| value == 0) {
            return Err("pollTimeoutMillis must be positive");
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct BlackForestLabsPollOverrides {
    poll_interval_millis: Option<u64>,
    poll_timeout_millis: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct BlackForestLabsSubmitResponse {
    id: String,
    polling_url: String,
    #[serde(default)]
    cost: Option<f64>,
    #[serde(default)]
    input_mp: Option<f64>,
    #[serde(default)]
    output_mp: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct BlackForestLabsPollResponse {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    result: Option<BlackForestLabsPollResult>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct BlackForestLabsPollResult {
    sample: Option<String>,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default)]
    start_time: Option<f64>,
    #[serde(default)]
    end_time: Option<f64>,
    #[serde(default)]
    duration: Option<f64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum BlackForestLabsPollStatus {
    Pending,
    Failed,
    MissingSample,
    MissingStatus,
}

#[derive(Clone, Debug, PartialEq)]
struct BlackForestLabsPollReady {
    image_url: String,
    seed: Option<u64>,
    start_time: Option<f64>,
    end_time: Option<f64>,
    duration: Option<f64>,
}

impl BlackForestLabsPollResponse {
    fn into_ready(self) -> Result<BlackForestLabsPollReady, BlackForestLabsPollStatus> {
        match self.status.or(self.state).as_deref() {
            Some("Ready") => {
                let Some(result) = self.result else {
                    return Err(BlackForestLabsPollStatus::MissingSample);
                };
                let Some(image_url) = result.sample else {
                    return Err(BlackForestLabsPollStatus::MissingSample);
                };

                Ok(BlackForestLabsPollReady {
                    image_url,
                    seed: result.seed,
                    start_time: result.start_time,
                    end_time: result.end_time,
                    duration: result.duration,
                })
            }
            Some("Error" | "Failed") => Err(BlackForestLabsPollStatus::Failed),
            Some(_) => Err(BlackForestLabsPollStatus::Pending),
            None => Err(BlackForestLabsPollStatus::MissingStatus),
        }
    }
}

fn black_forest_labs_image_request_body(
    options: &ImageModelCallOptions,
) -> Result<
    (
        ai_sdk_rust::JsonValue,
        Vec<Warning>,
        BlackForestLabsPollOverrides,
    ),
    String,
> {
    let mut warnings = Vec::new();
    let provider_options = parse_provider_options(
        "blackForestLabs",
        Some(&options.provider_options),
        black_forest_labs_image_model_options,
    )
    .map_err(|error| error.to_string())?;
    let provider_options = provider_options.unwrap_or_default();
    let mut body = ai_sdk_rust::JsonObject::new();

    if let Some(prompt) = options.prompt.as_ref() {
        body.insert(
            "prompt".to_string(),
            ai_sdk_rust::JsonValue::String(prompt.clone()),
        );
    }

    if let Some(seed) = options.seed {
        body.insert("seed".to_string(), ai_sdk_rust::JsonValue::from(seed));
    }

    let final_aspect_ratio = options.aspect_ratio.clone().or_else(|| {
        options
            .size
            .as_deref()
            .and_then(convert_size_to_aspect_ratio)
    });

    if options.size.is_some() && options.aspect_ratio.is_none() {
        warnings.push(Warning::Unsupported {
            feature: "size".to_string(),
            details: Some("Deriving aspect_ratio from size. Use the width and height provider options to specify dimensions for models that support them.".to_string()),
        });
    } else if options.size.is_some() && options.aspect_ratio.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "size".to_string(),
            details: Some("Black Forest Labs ignores size when aspectRatio is provided. Use the width and height provider options to specify dimensions for models that support them".to_string()),
        });
    }

    if let Some(aspect_ratio) = final_aspect_ratio {
        body.insert(
            "aspect_ratio".to_string(),
            ai_sdk_rust::JsonValue::String(aspect_ratio),
        );
    }

    let (size_width, size_height) = if options.aspect_ratio.is_some() {
        (None, None)
    } else {
        options
            .size
            .as_deref()
            .and_then(parse_size)
            .unwrap_or((None, None))
    };
    insert_option_u64(&mut body, "width", provider_options.width.or(size_width));
    insert_option_u64(&mut body, "height", provider_options.height.or(size_height));
    insert_option_u64(&mut body, "steps", provider_options.steps);
    insert_option_f64(&mut body, "guidance", provider_options.guidance);
    insert_option_f64(
        &mut body,
        "image_prompt_strength",
        provider_options.image_prompt_strength,
    );
    insert_option_string(&mut body, "image_prompt", provider_options.image_prompt);

    if let Some(files) = options.files.as_ref() {
        if files.len() > 10 {
            return Err("Black Forest Labs supports up to 10 input images.".to_string());
        }

        for (index, file) in files.iter().enumerate() {
            let name = if index == 0 {
                "input_image".to_string()
            } else {
                format!("input_image_{}", index + 1)
            };
            body.insert(
                name,
                ai_sdk_rust::JsonValue::String(black_forest_labs_file_input(file)),
            );
        }
    }

    if let Some(mask) = options.mask.as_ref() {
        body.insert(
            "mask".to_string(),
            ai_sdk_rust::JsonValue::String(black_forest_labs_file_input(mask)),
        );
    }

    insert_option_string(&mut body, "output_format", provider_options.output_format);
    insert_option_bool(
        &mut body,
        "prompt_upsampling",
        provider_options.prompt_upsampling,
    );
    insert_option_bool(&mut body, "raw", provider_options.raw);
    insert_option_u64(
        &mut body,
        "safety_tolerance",
        provider_options.safety_tolerance,
    );
    insert_option_string(&mut body, "webhook_secret", provider_options.webhook_secret);
    insert_option_string(&mut body, "webhook_url", provider_options.webhook_url);

    Ok((
        ai_sdk_rust::JsonValue::Object(body),
        warnings,
        BlackForestLabsPollOverrides {
            poll_interval_millis: provider_options.poll_interval_millis,
            poll_timeout_millis: provider_options.poll_timeout_millis,
        },
    ))
}

fn black_forest_labs_image_model_options(
    value: &ai_sdk_rust::JsonValue,
) -> Result<BlackForestLabsImageModelOptions, String> {
    let options = serde_json::from_value::<BlackForestLabsImageModelOptions>(value.clone())
        .map_err(|error| error.to_string())?;
    options.validate().map_err(str::to_string)?;
    Ok(options)
}

fn black_forest_labs_file_input(file: &ImageModelFile) -> String {
    match file {
        ImageModelFile::Url { url, .. } => url.as_str().to_string(),
        ImageModelFile::File { data, .. } => convert_to_base64(data),
    }
}

fn convert_size_to_aspect_ratio(size: &str) -> Option<String> {
    let Some((Some(width), Some(height))) = parse_size(size) else {
        return None;
    };

    if width == 0 || height == 0 {
        return None;
    }

    let divisor = gcd(width, height);
    Some(format!("{}:{}", width / divisor, height / divisor))
}

fn parse_size(size: &str) -> Option<(Option<u64>, Option<u64>)> {
    let (width, height) = size.split_once('x')?;
    Some((width.parse().ok(), height.parse().ok()))
}

fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let next = right;
        right = left % right;
        left = next;
    }

    left
}

fn insert_option_bool(body: &mut ai_sdk_rust::JsonObject, name: &str, value: Option<bool>) {
    if let Some(value) = value {
        body.insert(name.to_string(), ai_sdk_rust::JsonValue::Bool(value));
    }
}

fn insert_option_f64(body: &mut ai_sdk_rust::JsonObject, name: &str, value: Option<f64>) {
    if let Some(value) = value {
        body.insert(name.to_string(), ai_sdk_rust::JsonValue::from(value));
    }
}

fn insert_option_string(body: &mut ai_sdk_rust::JsonObject, name: &str, value: Option<String>) {
    if let Some(value) = value {
        body.insert(name.to_string(), ai_sdk_rust::JsonValue::String(value));
    }
}

fn insert_option_u64(body: &mut ai_sdk_rust::JsonObject, name: &str, value: Option<u64>) {
    if let Some(value) = value {
        body.insert(name.to_string(), ai_sdk_rust::JsonValue::from(value));
    }
}

fn black_forest_labs_provider_header_entries(
    settings: &BlackForestLabsProviderSettings,
) -> Result<Vec<(String, Option<String>)>, LoadApiKeyError> {
    let mut headers = vec![(
        "x-key".to_string(),
        Some(black_forest_labs_api_key(settings.api_key.as_ref())?),
    )];

    headers.extend(
        settings
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    );

    Ok(with_user_agent_suffix(
        Some(headers),
        [format!("ai-sdk/black-forest-labs/{}", ai_sdk_rust::VERSION)],
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

fn black_forest_labs_api_key(explicit_api_key: Option<&String>) -> Result<String, LoadApiKeyError> {
    let mut options = LoadApiKeyOptions::new("BFL_API_KEY", "Black Forest Labs");

    if let Some(api_key) = explicit_api_key {
        options = options.with_api_key(api_key.clone());
    }

    load_api_key(options)
}

fn black_forest_labs_submit_response(
    value: &ai_sdk_rust::JsonValue,
) -> Result<BlackForestLabsSubmitResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn black_forest_labs_poll_response(
    value: &ai_sdk_rust::JsonValue,
) -> Result<BlackForestLabsPollResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn black_forest_labs_error_response(
    value: &ai_sdk_rust::JsonValue,
) -> Result<ai_sdk_rust::JsonValue, serde_json::Error> {
    Ok(value.clone())
}

fn black_forest_labs_error_message(value: &ai_sdk_rust::JsonValue) -> String {
    if let Some(detail) = value.get("detail") {
        if let Some(detail) = detail.as_str() {
            return detail.to_string();
        }

        if !detail.is_null() {
            return serde_json::to_string(detail)
                .unwrap_or_else(|_| "Unknown Black Forest Labs error".to_string());
        }
    }

    value
        .get("message")
        .and_then(ai_sdk_rust::JsonValue::as_str)
        .unwrap_or("Unknown Black Forest Labs error")
        .to_string()
}

fn black_forest_labs_handled_error_parts(error: HandledFetchError) -> (String, Option<Headers>) {
    match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
        ),
    }
}

fn black_forest_labs_image_result_from_error(
    model_id: &str,
    message: String,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> ImageModelResult {
    let mut result = ImageModelResult::new(
        Vec::new(),
        black_forest_labs_image_response_metadata(model_id, headers, timestamp),
    )
    .with_provider_metadata(black_forest_labs_image_error_metadata(message));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn black_forest_labs_image_response_metadata(
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

fn black_forest_labs_image_provider_metadata(
    submit: &BlackForestLabsSubmitResponse,
    poll: &BlackForestLabsPollReady,
) -> ImageModelProviderMetadata {
    let mut image = ai_sdk_rust::JsonObject::new();

    insert_option_json_u64(&mut image, "seed", poll.seed);
    insert_option_json_f64(&mut image, "start_time", poll.start_time);
    insert_option_json_f64(&mut image, "end_time", poll.end_time);
    insert_option_json_f64(&mut image, "duration", poll.duration);
    insert_option_json_f64(&mut image, "cost", submit.cost);
    insert_option_json_f64(&mut image, "inputMegapixels", submit.input_mp);
    insert_option_json_f64(&mut image, "outputMegapixels", submit.output_mp);

    ImageModelProviderMetadata::from([(
        "blackForestLabs".to_string(),
        ImageModelProviderMetadataEntry::new(vec![ai_sdk_rust::JsonValue::Object(image)]),
    )])
}

fn black_forest_labs_image_error_metadata(message: String) -> ImageModelProviderMetadata {
    let mut extra = ai_sdk_rust::JsonObject::new();
    extra.insert(
        "errorMessage".to_string(),
        ai_sdk_rust::JsonValue::String(message),
    );

    ImageModelProviderMetadata::from([(
        "blackForestLabs".to_string(),
        ImageModelProviderMetadataEntry {
            images: Vec::new(),
            extra,
        },
    )])
}

fn insert_option_json_f64(body: &mut ai_sdk_rust::JsonObject, name: &str, value: Option<f64>) {
    if let Some(value) = value {
        body.insert(name.to_string(), ai_sdk_rust::JsonValue::from(value));
    }
}

fn insert_option_json_u64(body: &mut ai_sdk_rust::JsonObject, name: &str, value: Option<u64>) {
    if let Some(value) = value {
        body.insert(name.to_string(), ai_sdk_rust::JsonValue::from(value));
    }
}

fn default_black_forest_labs_date_provider() -> BlackForestLabsDateProvider {
    Arc::new(OffsetDateTime::now_utc)
}

fn default_black_forest_labs_transport() -> BlackForestLabsTransport {
    Arc::new(|request| Box::pin(ready(execute_black_forest_labs_request(request))))
}

fn execute_black_forest_labs_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_black_forest_labs_get_request(request),
        ProviderApiRequestMethod::Post => execute_black_forest_labs_post_request(request),
    }
}

fn execute_black_forest_labs_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    black_forest_labs_provider_api_response(response)
}

fn execute_black_forest_labs_post_request(
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
                "multipart form data is not supported by the Black Forest Labs transport",
            ));
        }
        None => builder.send_empty(),
    };

    black_forest_labs_provider_api_response(response)
}

fn black_forest_labs_provider_api_response(
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
        BlackForestLabsProvider, BlackForestLabsProviderSettings, BlackForestLabsTransport,
        BlackForestLabsTransportFuture, DEFAULT_BLACK_FOREST_LABS_BASE_URL, black_forest_labs,
        create_black_forest_labs,
    };
    use ai_sdk_rust::{
        FileDataContent, ImageModel, ImageModelCallOptions, ImageModelFile, ModelType, Provider,
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
        ProviderOptions, Warning,
    };
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
    use std::pin::Pin;
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

    fn poll_once<F: Future>(mut future: Pin<&mut F>) -> Poll<F::Output> {
        let waker = test_waker();
        let mut context = Context::from_waker(&waker);
        future.as_mut().poll(&mut context)
    }

    fn poll_pinned_until_ready<F: Future>(mut future: Pin<&mut F>, timeout: Duration) -> F::Output {
        let start = Instant::now();

        loop {
            match poll_once(future.as_mut()) {
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

    fn bfl_success_transport() -> (
        Arc<Mutex<Vec<ProviderApiRequest>>>,
        BlackForestLabsTransport,
    ) {
        recorded_transport(|request| match (request.method, request.url.as_str()) {
            (ProviderApiRequestMethod::Post, "https://api.example.com/v1/flux-pro-1.1") => {
                json_response(json!({
                    "id": "req-123",
                    "polling_url": "https://api.example.com/poll",
                    "cost": 0.08,
                    "input_mp": 1.5,
                    "output_mp": 2.0
                }))
            }
            (ProviderApiRequestMethod::Get, "https://api.example.com/poll?id=req-123") => {
                json_response(json!({
                    "status": "Ready",
                    "result": {
                        "sample": "https://api.example.com/image.png",
                        "seed": 12345,
                        "start_time": 10.0,
                        "end_time": 12.5,
                        "duration": 2.5
                    }
                }))
            }
            (ProviderApiRequestMethod::Get, "https://api.example.com/image.png") => {
                ProviderApiResponse::bytes(200, "OK", vec![1, 2, 3]).with_headers(
                    [("x-image-id".to_string(), "img-123".to_string())]
                        .into_iter()
                        .collect(),
                )
            }
            _ => ProviderApiResponse::text(
                404,
                "Not Found",
                json!({"message": "unexpected request"}).to_string(),
            ),
        })
    }

    fn recorded_transport<F>(
        handler: F,
    ) -> (
        Arc<Mutex<Vec<ProviderApiRequest>>>,
        BlackForestLabsTransport,
    )
    where
        F: Fn(&ProviderApiRequest) -> ProviderApiResponse + Send + Sync + 'static,
    {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: BlackForestLabsTransport =
            Arc::new(move |request| -> BlackForestLabsTransportFuture {
                requests_for_transport
                    .lock()
                    .expect("request list mutex is not poisoned")
                    .push(request.clone());

                Box::pin(ready(Ok(handler(&request))))
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
    fn black_forest_labs_provider_creates_image_model_with_headers_body_and_metadata() {
        let (requests, transport) = bfl_success_transport();
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1/")
                .with_header("x-extra-header", "extra"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "blackForestLabs".to_string(),
            serde_json::from_value(json!({
                "promptUpsampling": true,
                "unsupportedProperty": "ignored"
            }))
            .expect("object provider options deserialize"),
        );

        let result = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A serene mountain landscape at sunset")
                    .with_aspect_ratio("1:1")
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.images, vec![FileDataContent::Bytes(vec![1, 2, 3])]);
        assert_eq!(result.response.model_id, "flux-pro-1.1");
        assert_eq!(result.response.timestamp, fixed_timestamp());
        assert_eq!(
            result
                .response
                .headers
                .expect("image response headers")
                .get("x-image-id"),
            Some(&"img-123".to_string())
        );
        let metadata = result
            .provider_metadata
            .expect("provider metadata")
            .remove("blackForestLabs")
            .expect("Black Forest Labs metadata");
        assert_eq!(
            metadata.images[0],
            json!({
                "cost": 0.08,
                "duration": 2.5,
                "end_time": 12.5,
                "inputMegapixels": 1.5,
                "outputMegapixels": 2.0,
                "seed": 12345,
                "start_time": 10.0
            })
        );

        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method, ProviderApiRequestMethod::Post);
        assert_eq!(requests[0].url, "https://api.example.com/v1/flux-pro-1.1");
        assert_eq!(
            requests[0].headers.get("x-key"),
            Some(&"test-api-key".to_string())
        );
        assert_eq!(
            requests[0].headers.get("x-extra-header"),
            Some(&"extra".to_string())
        );
        assert!(
            requests[0]
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/black-forest-labs/"))
        );
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "aspect_ratio": "1:1",
                "prompt": "A serene mountain landscape at sunset",
                "prompt_upsampling": true
            })
        );
        assert_eq!(requests[1].method, ProviderApiRequestMethod::Get);
        assert_eq!(requests[1].url, "https://api.example.com/poll?id=req-123");
        assert_eq!(requests[2].method, ProviderApiRequestMethod::Get);
        assert_eq!(requests[2].url, "https://api.example.com/image.png");
    }

    #[test]
    fn black_forest_labs_image_model_derives_aspect_ratio_and_passes_files_mask_and_options() {
        let (requests, transport) = bfl_success_transport();
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "blackForestLabs".to_string(),
            serde_json::from_value(json!({
                "height": 512,
                "imagePrompt": "style ref",
                "imagePromptStrength": 0.7,
                "outputFormat": "png",
                "pollIntervalMillis": 1,
                "pollTimeoutMillis": 10,
                "raw": true,
                "safetyTolerance": 3,
                "steps": 12,
                "webhookUrl": "https://example.com/webhook"
            }))
            .expect("object provider options deserialize"),
        );

        let result = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A generated image")
                    .with_size("1024x512")
                    .with_seed(7)
                    .with_files(vec![
                        ImageModelFile::file("image/png", FileDataContent::Bytes(vec![1, 2, 3])),
                        ImageModelFile::url(
                            Url::parse("https://example.com/input.png").expect("valid URL"),
                        ),
                    ])
                    .with_mask(ImageModelFile::file(
                        "image/png",
                        FileDataContent::Base64("bWFzaw==".to_string()),
                    ))
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(
            result.warnings,
            vec![Warning::Unsupported {
                feature: "size".to_string(),
                details: Some("Deriving aspect_ratio from size. Use the width and height provider options to specify dimensions for models that support them.".to_string()),
            }]
        );
        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "aspect_ratio": "2:1",
                "height": 512,
                "image_prompt": "style ref",
                "image_prompt_strength": 0.7,
                "input_image": "AQID",
                "input_image_2": "https://example.com/input.png",
                "mask": "bWFzaw==",
                "output_format": "png",
                "prompt": "A generated image",
                "raw": true,
                "safety_tolerance": 3,
                "seed": 7,
                "steps": 12,
                "webhook_url": "https://example.com/webhook",
                "width": 1024
            })
        );
    }

    #[test]
    fn black_forest_labs_image_model_maps_api_and_poll_errors_to_metadata() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: BlackForestLabsTransport =
            Arc::new(move |request| -> BlackForestLabsTransportFuture {
                requests_for_transport
                    .lock()
                    .expect("request list mutex is not poisoned")
                    .push(request);

                Box::pin(ready(Ok(ProviderApiResponse::text(
                    400,
                    "Bad Request",
                    json!({
                        "message": "Top-level message",
                        "detail": {"error": "Invalid prompt"}
                    })
                    .to_string(),
                ))))
            });
        let provider = BlackForestLabsProvider::from_settings(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport);

        let result = poll_ready(
            provider
                .image("flux-pro-1.1")
                .do_generate(ImageModelCallOptions::new(1).with_prompt("Invalid prompt")),
        );

        assert!(result.images.is_empty());
        let metadata = result
            .provider_metadata
            .expect("provider metadata")
            .remove("blackForestLabs")
            .expect("Black Forest Labs metadata");
        assert_eq!(metadata.images, Vec::<ai_sdk_rust::JsonValue>::new());
        assert_eq!(
            metadata.extra.get("errorMessage"),
            Some(&json!("{\"error\":\"Invalid prompt\"}"))
        );
        assert_eq!(
            requests
                .lock()
                .expect("request list mutex is not poisoned")
                .len(),
            1
        );
    }

    #[test]
    fn black_forest_labs_provider_reports_unsupported_model_families_and_trait_image() {
        let provider = black_forest_labs();
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
        assert_eq!(
            provider.image("flux-pro-1.1").provider(),
            "black-forest-labs.image"
        );

        let trait_image_model = Provider::image_model(&provider, "flux-pro-1.1")
            .expect("Provider trait creates image model");
        assert_eq!(trait_image_model.model_id(), "flux-pro-1.1");
    }

    #[test]
    fn black_forest_labs_provider_settings_serde_accepts_upstream_shape() {
        let settings: BlackForestLabsProviderSettings = serde_json::from_value(json!({
            "apiKey": "key",
            "baseURL": "https://api.example.com/v1/",
            "headers": {
                "x-extra": "1"
            },
            "pollIntervalMillis": 10,
            "pollTimeoutMillis": 30
        }))
        .expect("settings deserialize");
        let provider = create_black_forest_labs(settings.clone());

        assert_eq!(settings.api_key.as_deref(), Some("key"));
        assert_eq!(
            settings.base_url.as_deref(),
            Some("https://api.example.com/v1/")
        );
        assert_eq!(settings.headers.get("x-extra"), Some(&"1".to_string()));
        assert_eq!(settings.poll_interval_millis, Some(10));
        assert_eq!(settings.poll_timeout_millis, Some(30));
        assert_eq!(provider.base_url, "https://api.example.com/v1");
        assert_eq!(DEFAULT_BLACK_FOREST_LABS_BASE_URL, "https://api.bfl.ai/v1");
    }

    #[test]
    fn black_forest_labs_provider_creates_image_models_via_image_and_image_model() {
        let provider = black_forest_labs();

        let image_model = provider.image("flux-pro-1.1");
        let image_model2 = provider
            .image_model("flux-pro-1.1-ultra")
            .expect("image model is supported");

        assert_eq!(image_model.provider(), "black-forest-labs.image");
        assert_eq!(image_model.model_id(), "flux-pro-1.1");
        assert_eq!(image_model2.model_id(), "flux-pro-1.1-ultra");
        assert_eq!(image_model2.provider(), "black-forest-labs.image");
        assert_eq!(image_model2.specification_version().as_str(), "v4");
    }

    #[test]
    fn black_forest_labs_provider_configures_base_url_and_headers_correctly() {
        let (requests, transport) = bfl_success_transport();
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1")
                .with_header("x-extra-header", "extra"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let model = provider.image("flux-pro-1.1");

        let result = poll_ready(model.do_generate(
            ImageModelCallOptions::new(1).with_prompt("A serene mountain landscape at sunset"),
        ));

        assert_eq!(result.images, vec![FileDataContent::Bytes(vec![1, 2, 3])]);
        assert_eq!(result.response.model_id, "flux-pro-1.1");
        assert_eq!(result.response.timestamp, fixed_timestamp());
        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(requests[0].method, ProviderApiRequestMethod::Post);
        assert_eq!(requests[0].url, "https://api.example.com/v1/flux-pro-1.1");
        assert_eq!(
            requests[0].headers.get("x-key"),
            Some(&"test-api-key".to_string())
        );
        assert_eq!(
            requests[0].headers.get("x-extra-header"),
            Some(&"extra".to_string())
        );
        assert!(
            requests[0]
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/black-forest-labs/"))
        );
    }

    #[test]
    fn black_forest_labs_provider_uses_provider_polling_options_for_timeout_behavior() {
        let (requests, transport) =
            recorded_transport(|request| match (request.method, request.url.as_str()) {
                (ProviderApiRequestMethod::Post, "https://api.example.com/v1/flux-pro-1.1") => {
                    json_response(json!({
                        "id": "req-123",
                        "polling_url": "https://api.example.com/poll"
                    }))
                }
                (ProviderApiRequestMethod::Get, "https://api.example.com/poll?id=req-123") => {
                    json_response(json!({
                        "status": "Pending"
                    }))
                }
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"message": "unexpected request"}).to_string(),
                ),
            });
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1")
                .with_poll_interval_millis(10)
                .with_poll_timeout_millis(25),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let model = provider.image("flux-pro-1.1");
        let mut future = Box::pin(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("Timeout test")
                    .with_aspect_ratio("1:1"),
            ),
        );
        let result = poll_pinned_until_ready(future.as_mut(), Duration::from_millis(200));

        assert!(result.images.is_empty());
        let metadata = result
            .provider_metadata
            .expect("provider metadata")
            .remove("blackForestLabs")
            .expect("Black Forest Labs metadata");
        assert_eq!(
            metadata.extra.get("errorMessage"),
            Some(&json!("Black Forest Labs generation timed out."))
        );
        let poll_calls = requests
            .lock()
            .expect("request list mutex is not poisoned")
            .iter()
            .filter(|request| {
                request.method == ProviderApiRequestMethod::Get
                    && request.url.starts_with("https://api.example.com/poll")
            })
            .count();
        assert_eq!(poll_calls, 3);
    }

    #[test]
    fn black_forest_labs_provider_throws_nosuchmodelerror_for_unsupported_model_types() {
        let provider = black_forest_labs();

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
    }

    #[test]
    fn black_forest_labs_image_model_passes_correct_parameters_including_aspect_ratio_and_provider_options()
     {
        let (requests, transport) = bfl_success_transport();
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "blackForestLabs".to_string(),
            serde_json::from_value(json!({
                "promptUpsampling": true,
                "unsupportedProperty": "value"
            }))
            .expect("object provider options deserialize"),
        );

        let result = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A cute baby sea otter")
                    .with_aspect_ratio("16:9")
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.images, vec![FileDataContent::Bytes(vec![1, 2, 3])]);
        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "aspect_ratio": "16:9",
                "prompt": "A cute baby sea otter",
                "prompt_upsampling": true
            })
        );
    }

    #[test]
    fn black_forest_labs_image_model_includes_seed_in_provider_metadata_images_when_provided_by_api()
     {
        let (requests, transport) = bfl_success_transport();
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A cute baby sea otter")
                    .with_aspect_ratio("1:1"),
            ),
        );

        let metadata = result
            .provider_metadata
            .expect("provider metadata")
            .remove("blackForestLabs")
            .expect("Black Forest Labs metadata");
        assert_eq!(
            metadata.images[0],
            json!({"seed": 12345, "start_time": 10.0, "end_time": 12.5, "duration": 2.5, "cost": 0.08, "inputMegapixels": 1.5, "outputMegapixels": 2.0})
        );
        assert_eq!(
            requests
                .lock()
                .expect("request list mutex is not poisoned")
                .len(),
            3
        );
    }

    #[test]
    fn black_forest_labs_image_model_includes_all_cost_and_megapixel_fields_when_provided_by_submit_api()
     {
        let (requests, transport) = bfl_success_transport();
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A cute baby sea otter")
                    .with_aspect_ratio("1:1"),
            ),
        );

        let metadata = result
            .provider_metadata
            .expect("provider metadata")
            .remove("blackForestLabs")
            .expect("Black Forest Labs metadata");
        assert_eq!(metadata.images[0]["cost"], json!(0.08));
        assert_eq!(metadata.images[0]["inputMegapixels"], json!(1.5));
        assert_eq!(metadata.images[0]["outputMegapixels"], json!(2.0));
        assert_eq!(
            requests
                .lock()
                .expect("request list mutex is not poisoned")
                .len(),
            3
        );
    }

    #[test]
    fn black_forest_labs_image_model_omits_cost_and_megapixel_fields_from_provider_metadata_when_not_provided_by_submit_api()
     {
        let (requests, transport) =
            recorded_transport(|request| match (request.method, request.url.as_str()) {
                (ProviderApiRequestMethod::Post, "https://api.example.com/v1/flux-pro-1.1") => {
                    json_response(json!({
                        "id": "req-123",
                        "polling_url": "https://api.example.com/poll"
                    }))
                }
                (ProviderApiRequestMethod::Get, "https://api.example.com/poll?id=req-123") => {
                    json_response(json!({
                        "status": "Ready",
                        "result": {
                            "sample": "https://api.example.com/image.png"
                        }
                    }))
                }
                (ProviderApiRequestMethod::Get, "https://api.example.com/image.png") => {
                    ProviderApiResponse::bytes(200, "OK", vec![1, 2, 3])
                }
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"message": "unexpected request"}).to_string(),
                ),
            });
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A cute baby sea otter")
                    .with_aspect_ratio("1:1"),
            ),
        );

        let metadata = result
            .provider_metadata
            .expect("provider metadata")
            .remove("blackForestLabs")
            .expect("Black Forest Labs metadata");
        assert!(
            !metadata.images[0]
                .as_object()
                .expect("image metadata object")
                .contains_key("cost")
        );
        assert!(
            !metadata.images[0]
                .as_object()
                .expect("image metadata object")
                .contains_key("inputMegapixels")
        );
        assert!(
            !metadata.images[0]
                .as_object()
                .expect("image metadata object")
                .contains_key("outputMegapixels")
        );
        assert_eq!(
            requests
                .lock()
                .expect("request list mutex is not poisoned")
                .len(),
            3
        );
    }

    #[test]
    fn black_forest_labs_image_model_handles_null_cost_and_megapixel_fields_from_submit_api() {
        let (requests, transport) =
            recorded_transport(|request| match (request.method, request.url.as_str()) {
                (ProviderApiRequestMethod::Post, "https://api.example.com/v1/flux-pro-1.1") => {
                    json_response(json!({
                        "id": "req-123",
                        "polling_url": "https://api.example.com/poll",
                        "cost": null,
                        "input_mp": null,
                        "output_mp": null
                    }))
                }
                (ProviderApiRequestMethod::Get, "https://api.example.com/poll?id=req-123") => {
                    json_response(json!({
                        "status": "Ready",
                        "result": {
                            "sample": "https://api.example.com/image.png"
                        }
                    }))
                }
                (ProviderApiRequestMethod::Get, "https://api.example.com/image.png") => {
                    ProviderApiResponse::bytes(200, "OK", vec![1, 2, 3])
                }
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"message": "unexpected request"}).to_string(),
                ),
            });
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A cute baby sea otter")
                    .with_aspect_ratio("1:1"),
            ),
        );

        let metadata = result
            .provider_metadata
            .expect("provider metadata")
            .remove("blackForestLabs")
            .expect("Black Forest Labs metadata");
        assert!(
            !metadata.images[0]
                .as_object()
                .expect("image metadata object")
                .contains_key("cost")
        );
        assert!(
            !metadata.images[0]
                .as_object()
                .expect("image metadata object")
                .contains_key("inputMegapixels")
        );
        assert!(
            !metadata.images[0]
                .as_object()
                .expect("image metadata object")
                .contains_key("outputMegapixels")
        );
        assert_eq!(
            requests
                .lock()
                .expect("request list mutex is not poisoned")
                .len(),
            3
        );
    }

    #[test]
    fn black_forest_labs_image_model_calls_the_expected_urls_in_sequence() {
        let (requests, transport) = bfl_success_transport();
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let _ = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A cute baby sea otter")
                    .with_aspect_ratio("1:1"),
            ),
        );

        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(requests[0].method, ProviderApiRequestMethod::Post);
        assert_eq!(requests[0].url, "https://api.example.com/v1/flux-pro-1.1");
        assert_eq!(requests[1].method, ProviderApiRequestMethod::Get);
        assert_eq!(requests[1].url, "https://api.example.com/poll?id=req-123");
        assert_eq!(requests[2].method, ProviderApiRequestMethod::Get);
        assert_eq!(requests[2].url, "https://api.example.com/image.png");
    }

    #[test]
    fn black_forest_labs_image_model_merges_provider_and_request_headers_for_submit_call() {
        let (requests, transport) = bfl_success_transport();
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1")
                .with_header("x-custom-provider-header", "provider-header-value"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let _ = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A cute baby sea otter")
                    .with_aspect_ratio("1:1")
                    .with_header("x-custom-request-header", "request-header-value"),
            ),
        );

        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(
            requests[0].headers.get("x-custom-provider-header"),
            Some(&"provider-header-value".to_string())
        );
        assert_eq!(
            requests[0].headers.get("x-custom-request-header"),
            Some(&"request-header-value".to_string())
        );
    }

    #[test]
    fn black_forest_labs_image_model_passes_merged_headers_to_polling_requests() {
        let (requests, transport) = bfl_success_transport();
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1")
                .with_header("x-custom-provider-header", "provider-header-value"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let _ = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A cute baby sea otter")
                    .with_aspect_ratio("1:1")
                    .with_header("x-custom-request-header", "request-header-value"),
            ),
        );

        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(
            requests[1].headers.get("x-custom-provider-header"),
            Some(&"provider-header-value".to_string())
        );
        assert_eq!(
            requests[1].headers.get("x-custom-request-header"),
            Some(&"request-header-value".to_string())
        );
        assert_eq!(
            requests[2].headers.get("x-custom-provider-header"),
            Some(&"provider-header-value".to_string())
        );
        assert_eq!(
            requests[2].headers.get("x-custom-request-header"),
            Some(&"request-header-value".to_string())
        );
    }

    #[test]
    fn black_forest_labs_image_model_warns_and_derives_aspect_ratio_when_size_is_provided() {
        let (requests, transport) = bfl_success_transport();
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "blackForestLabs".to_string(),
            serde_json::from_value(json!({
                "height": 512,
                "imagePrompt": "style ref",
                "imagePromptStrength": 0.7,
                "outputFormat": "png",
                "pollIntervalMillis": 1,
                "pollTimeoutMillis": 10,
                "raw": true,
                "safetyTolerance": 3,
                "steps": 12,
                "webhookUrl": "https://example.com/webhook"
            }))
            .expect("object provider options deserialize"),
        );

        let result = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A generated image")
                    .with_size("1024x512")
                    .with_seed(7)
                    .with_files(vec![
                        ImageModelFile::file("image/png", FileDataContent::Bytes(vec![1, 2, 3])),
                        ImageModelFile::url(
                            Url::parse("https://example.com/input.png").expect("valid URL"),
                        ),
                    ])
                    .with_mask(ImageModelFile::file(
                        "image/png",
                        FileDataContent::Base64("bWFzaw==".to_string()),
                    ))
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(
            result.warnings,
            vec![Warning::Unsupported {
                feature: "size".to_string(),
                details: Some("Deriving aspect_ratio from size. Use the width and height provider options to specify dimensions for models that support them.".to_string()),
            }]
        );
        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "aspect_ratio": "2:1",
                "height": 512,
                "image_prompt": "style ref",
                "image_prompt_strength": 0.7,
                "input_image": "AQID",
                "input_image_2": "https://example.com/input.png",
                "mask": "bWFzaw==",
                "output_format": "png",
                "prompt": "A generated image",
                "raw": true,
                "safety_tolerance": 3,
                "seed": 7,
                "steps": 12,
                "webhook_url": "https://example.com/webhook",
                "width": 1024
            })
        );
    }

    #[test]
    fn black_forest_labs_image_model_warns_and_ignores_size_when_both_size_and_aspect_ratio_are_provided()
     {
        let (requests, transport) = bfl_success_transport();
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport);

        let result = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A generated image")
                    .with_size("1024x512")
                    .with_aspect_ratio("1:1"),
            ),
        );

        assert_eq!(
            result.warnings,
            vec![Warning::Unsupported {
                feature: "size".to_string(),
                details: Some("Black Forest Labs ignores size when aspectRatio is provided. Use the width and height provider options to specify dimensions for models that support them".to_string()),
            }]
        );
        assert_eq!(
            request_body_json(&requests.lock().expect("request list mutex is not poisoned")[0]),
            json!({
                "aspect_ratio": "1:1",
                "prompt": "A generated image"
            })
        );
    }

    #[test]
    fn black_forest_labs_image_model_handles_api_errors_with_message_and_detail() {
        let (requests, transport) =
            recorded_transport(|request| match (request.method, request.url.as_str()) {
                (ProviderApiRequestMethod::Post, "https://api.example.com/v1/flux-pro-1.1") => {
                    ProviderApiResponse::text(
                        400,
                        "Bad Request",
                        json!({
                            "message": "Top-level message",
                            "detail": {"error": "Invalid prompt"}
                        })
                        .to_string(),
                    )
                }
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"message": "unexpected request"}).to_string(),
                ),
            });
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("Invalid prompt")
                    .with_aspect_ratio("1:1"),
            ),
        );

        assert!(result.images.is_empty());
        let metadata = result
            .provider_metadata
            .expect("provider metadata")
            .remove("blackForestLabs")
            .expect("Black Forest Labs metadata");
        assert_eq!(
            metadata.extra.get("errorMessage"),
            Some(&json!("{\"error\":\"Invalid prompt\"}"))
        );
        assert_eq!(
            requests
                .lock()
                .expect("request list mutex is not poisoned")
                .len(),
            1
        );
    }

    #[test]
    fn black_forest_labs_image_model_handles_poll_responses_with_state_instead_of_status() {
        let (requests, transport) =
            recorded_transport(|request| match (request.method, request.url.as_str()) {
                (ProviderApiRequestMethod::Post, "https://api.example.com/v1/flux-pro-1.1") => {
                    json_response(json!({
                        "id": "req-123",
                        "polling_url": "https://api.example.com/poll"
                    }))
                }
                (ProviderApiRequestMethod::Get, "https://api.example.com/poll?id=req-123") => {
                    json_response(json!({
                        "state": "Ready",
                        "result": {
                            "sample": "https://api.example.com/image.png"
                        }
                    }))
                }
                (ProviderApiRequestMethod::Get, "https://api.example.com/image.png") => {
                    ProviderApiResponse::bytes(200, "OK", vec![1, 2, 3])
                }
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"message": "unexpected request"}).to_string(),
                ),
            });
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A cute baby sea otter")
                    .with_aspect_ratio("1:1"),
            ),
        );

        assert_eq!(result.images, vec![FileDataContent::Bytes(vec![1, 2, 3])]);
        assert_eq!(
            requests
                .lock()
                .expect("request list mutex is not poisoned")
                .len(),
            3
        );
    }

    #[test]
    fn black_forest_labs_image_model_polls_multiple_times_using_configured_interval_until_ready() {
        let poll_calls = Arc::new(Mutex::new(0usize));
        let poll_calls_for_transport = Arc::clone(&poll_calls);
        let (requests, transport) =
            recorded_transport(
                move |request| match (request.method, request.url.as_str()) {
                    (ProviderApiRequestMethod::Post, "https://api.example.com/v1/flux-pro-1.1") => {
                        json_response(json!({
                            "id": "req-123",
                            "polling_url": "https://api.example.com/poll"
                        }))
                    }
                    (ProviderApiRequestMethod::Get, "https://api.example.com/poll?id=req-123") => {
                        let mut calls = poll_calls_for_transport
                            .lock()
                            .expect("poll counter mutex is not poisoned");
                        *calls += 1;
                        match *calls {
                            1 | 2 => json_response(json!({
                                "status": "Pending"
                            })),
                            _ => json_response(json!({
                                "status": "Ready",
                                "result": {
                                    "sample": "https://api.example.com/image.png"
                                }
                            })),
                        }
                    }
                    (ProviderApiRequestMethod::Get, "https://api.example.com/image.png") => {
                        ProviderApiResponse::bytes(200, "OK", vec![1, 2, 3])
                    }
                    _ => ProviderApiResponse::text(
                        404,
                        "Not Found",
                        json!({"message": "unexpected request"}).to_string(),
                    ),
                },
            );
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1")
                .with_poll_interval_millis(10)
                .with_poll_timeout_millis(100),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let model = provider.image("flux-pro-1.1");
        let mut future = Box::pin(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A cute baby sea otter")
                    .with_aspect_ratio("1:1"),
            ),
        );
        let result = poll_pinned_until_ready(future.as_mut(), Duration::from_millis(200));

        assert_eq!(result.images, vec![FileDataContent::Bytes(vec![1, 2, 3])]);
        assert_eq!(
            *poll_calls
                .lock()
                .expect("poll counter mutex is not poisoned"),
            3
        );
        assert_eq!(
            requests
                .lock()
                .expect("request list mutex is not poisoned")
                .len(),
            5
        );
    }

    #[test]
    fn black_forest_labs_image_model_uses_configured_poll_timeout_millis_and_poll_interval_millis_to_time_out()
     {
        let (requests, transport) =
            recorded_transport(|request| match (request.method, request.url.as_str()) {
                (ProviderApiRequestMethod::Post, "https://api.example.com/v1/flux-pro-1.1") => {
                    json_response(json!({
                        "id": "req-123",
                        "polling_url": "https://api.example.com/poll"
                    }))
                }
                (ProviderApiRequestMethod::Get, "https://api.example.com/poll?id=req-123") => {
                    json_response(json!({
                        "status": "Pending"
                    }))
                }
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"message": "unexpected request"}).to_string(),
                ),
            });
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1")
                .with_poll_interval_millis(10)
                .with_poll_timeout_millis(25),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let model = provider.image("flux-pro-1.1");
        let mut future = Box::pin(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("Timeout test")
                    .with_aspect_ratio("1:1"),
            ),
        );
        let result = poll_pinned_until_ready(future.as_mut(), Duration::from_millis(200));

        assert!(result.images.is_empty());
        let metadata = result
            .provider_metadata
            .expect("provider metadata")
            .remove("blackForestLabs")
            .expect("Black Forest Labs metadata");
        assert_eq!(
            metadata.extra.get("errorMessage"),
            Some(&json!("Black Forest Labs generation timed out."))
        );
        let poll_calls = requests
            .lock()
            .expect("request list mutex is not poisoned")
            .iter()
            .filter(|request| {
                request.method == ProviderApiRequestMethod::Get
                    && request.url.starts_with("https://api.example.com/poll")
            })
            .count();
        assert_eq!(poll_calls, 3);
    }

    #[test]
    fn black_forest_labs_image_model_throws_when_poll_is_ready_but_sample_is_missing() {
        let (requests, transport) =
            recorded_transport(|request| match (request.method, request.url.as_str()) {
                (ProviderApiRequestMethod::Post, "https://api.example.com/v1/flux-pro-1.1") => {
                    json_response(json!({
                        "id": "req-123",
                        "polling_url": "https://api.example.com/poll"
                    }))
                }
                (ProviderApiRequestMethod::Get, "https://api.example.com/poll?id=req-123") => {
                    json_response(json!({
                        "status": "Ready",
                        "result": {}
                    }))
                }
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"message": "unexpected request"}).to_string(),
                ),
            });
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A cute baby sea otter")
                    .with_aspect_ratio("1:1"),
            ),
        );

        assert!(result.images.is_empty());
        let metadata = result
            .provider_metadata
            .expect("provider metadata")
            .remove("blackForestLabs")
            .expect("Black Forest Labs metadata");
        assert_eq!(
            metadata.extra.get("errorMessage"),
            Some(&json!(
                "Black Forest Labs poll response is Ready but missing result.sample"
            ))
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
    fn black_forest_labs_image_model_throws_when_poll_returns_error_or_failed() {
        for status in ["Error", "Failed"] {
            let (requests, transport) =
                recorded_transport(
                    move |request| match (request.method, request.url.as_str()) {
                        (
                            ProviderApiRequestMethod::Post,
                            "https://api.example.com/v1/flux-pro-1.1",
                        ) => json_response(json!({
                            "id": "req-123",
                            "polling_url": "https://api.example.com/poll"
                        })),
                        (
                            ProviderApiRequestMethod::Get,
                            "https://api.example.com/poll?id=req-123",
                        ) => json_response(json!({
                            "status": status
                        })),
                        _ => ProviderApiResponse::text(
                            404,
                            "Not Found",
                            json!({"message": "unexpected request"}).to_string(),
                        ),
                    },
                );
            let provider = create_black_forest_labs(
                BlackForestLabsProviderSettings::new()
                    .with_api_key("test-api-key")
                    .with_base_url("https://api.example.com/v1"),
            )
            .with_transport(transport)
            .with_current_date(fixed_timestamp);

            let result = poll_ready(
                provider.image("flux-pro-1.1").do_generate(
                    ImageModelCallOptions::new(1)
                        .with_prompt("A cute baby sea otter")
                        .with_aspect_ratio("1:1"),
                ),
            );

            assert!(result.images.is_empty());
            let metadata = result
                .provider_metadata
                .expect("provider metadata")
                .remove("blackForestLabs")
                .expect("Black Forest Labs metadata");
            assert_eq!(
                metadata.extra.get("errorMessage"),
                Some(&json!("Black Forest Labs generation failed."))
            );
            assert_eq!(
                requests
                    .lock()
                    .expect("request list mutex is not poisoned")
                    .len(),
                2
            );
        }
    }

    #[test]
    fn black_forest_labs_image_model_includes_timestamp_headers_and_model_id_in_response_metadata()
    {
        let (requests, transport) = bfl_success_transport();
        let provider = create_black_forest_labs(
            BlackForestLabsProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.example.com/v1"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let result = poll_ready(
            provider.image("flux-pro-1.1").do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A cute baby sea otter")
                    .with_aspect_ratio("1:1"),
            ),
        );

        assert_eq!(result.response.model_id, "flux-pro-1.1");
        assert_eq!(result.response.timestamp, fixed_timestamp());
        assert_eq!(
            result
                .response
                .headers
                .expect("image response headers")
                .get("x-image-id"),
            Some(&"img-123".to_string())
        );
        assert_eq!(
            requests
                .lock()
                .expect("request list mutex is not poisoned")
                .len(),
            3
        );
    }

    #[test]
    fn black_forest_labs_image_model_exposes_correct_provider_and_model_information() {
        let provider = black_forest_labs();
        let image_model = provider.image("flux-pro-1.1");

        assert_eq!(image_model.provider(), "black-forest-labs.image");
        assert_eq!(image_model.model_id(), "flux-pro-1.1");
        assert_eq!(image_model.specification_version().as_str(), "v4");
    }
}
