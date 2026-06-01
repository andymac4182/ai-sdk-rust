use std::collections::{BTreeMap, BTreeSet};
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use ai_sdk_rust::{
    DelayOptions, FetchErrorInfo, GetFromApiOptions, HandledFetchError, Headers, JsonObject,
    JsonValue, LoadSettingError, LoadSettingOptions, ModelType, NoSuchModelError,
    OpenAICompatibleChatLanguageModel, OpenAICompatibleEmbeddingModel, OpenAICompatibleImageModel,
    PostJsonToApiOptions, Provider, ProviderAbortSignal, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, ProviderMetadata, ProviderWithVideoModel, RuntimeEnvironment,
    VideoModel, VideoModelCallOptions, VideoModelFile, VideoModelResponse, VideoModelResult,
    VideoModelVideoData, Warning, combine_headers, convert_to_base64,
    create_json_error_response_handler, create_json_response_handler, delay_with_options,
    get_from_api, load_setting, parse_provider_options, post_json_to_api, with_user_agent_suffix,
    without_trailing_slash,
};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use time::OffsetDateTime;
use url::Url;

/// Default base URL for upstream `@ai-sdk/klingai` API calls.
pub const DEFAULT_KLINGAI_BASE_URL: &str = "https://api-singapore.klingai.com";

const DEFAULT_KLINGAI_POLL_INTERVAL_MILLIS: u64 = 5_000;
const DEFAULT_KLINGAI_POLL_TIMEOUT_MILLIS: u64 = 600_000;

type HmacSha256 = Hmac<Sha256>;

/// Settings for the upstream KlingAI provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KlingAIProviderSettings {
    /// KlingAI access key. When omitted, `KLINGAI_ACCESS_KEY` is read at request time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_key: Option<String>,

    /// KlingAI secret key. When omitted, `KLINGAI_SECRET_KEY` is read at request time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_key: Option<String>,

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

impl KlingAIProviderSettings {
    /// Creates empty KlingAI provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the KlingAI access key.
    pub fn with_access_key(mut self, access_key: impl Into<String>) -> Self {
        self.access_key = Some(access_key.into());
        self
    }

    /// Sets the KlingAI secret key.
    pub fn with_secret_key(mut self, secret_key: impl Into<String>) -> Self {
        self.secret_key = Some(secret_key.into());
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

/// Upstream KlingAI provider foundation.
#[derive(Clone)]
pub struct KlingAIProvider {
    base_url: String,
    settings: KlingAIProviderSettings,
    transport: KlingAITransport,
    current_date: KlingAIDateProvider,
}

/// KlingAI video model.
#[derive(Clone)]
pub struct KlingAIVideoModel {
    model_id: String,
    base_url: String,
    settings: KlingAIProviderSettings,
    transport: KlingAITransport,
    current_date: KlingAIDateProvider,
}

/// Future returned by an injected KlingAI HTTP transport.
pub type KlingAITransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by KlingAI provider models.
pub type KlingAITransport = Arc<dyn Fn(ProviderApiRequest) -> KlingAITransportFuture + Send + Sync>;

type KlingAIDateProvider = Arc<dyn Fn() -> OffsetDateTime + Send + Sync>;
type KlingAIVideoMaxVideosFuture<'a> = Ready<Option<usize>>;
type KlingAIVideoGenerateFuture<'a> = Pin<Box<dyn Future<Output = VideoModelResult> + Send + 'a>>;

impl KlingAIProvider {
    /// Creates a KlingAI provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(KlingAIProviderSettings::new())
    }

    /// Creates a provider from explicit KlingAI settings.
    pub fn from_settings(settings: KlingAIProviderSettings) -> Self {
        let base_url = without_trailing_slash(
            settings
                .base_url
                .as_deref()
                .or(Some(DEFAULT_KLINGAI_BASE_URL)),
        )
        .expect("default KlingAI base URL is present")
        .to_string();

        Self {
            base_url,
            settings,
            transport: default_klingai_transport(),
            current_date: Arc::new(OffsetDateTime::now_utc),
        }
    }

    /// Sets the KlingAI access key for this provider.
    pub fn with_access_key(mut self, access_key: impl Into<String>) -> Self {
        self.settings.access_key = Some(access_key.into());
        self
    }

    /// Sets the KlingAI secret key for this provider.
    pub fn with_secret_key(mut self, secret_key: impl Into<String>) -> Self {
        self.settings.secret_key = Some(secret_key.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: KlingAITransport) -> Self {
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
    pub fn video(&self, model_id: impl Into<String>) -> KlingAIVideoModel {
        self.video_model(model_id)
            .expect("KlingAI video models are supported")
    }

    /// Creates a video model.
    pub fn video_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<KlingAIVideoModel, NoSuchModelError> {
        let model_id = model_id.into();
        detect_klingai_mode(&model_id)?;
        Ok(KlingAIVideoModel::new(
            model_id,
            self.base_url.clone(),
            self.settings.clone(),
            Arc::clone(&self.transport),
            Arc::clone(&self.current_date),
        ))
    }

    /// Reports that KlingAI does not expose language models through this provider.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::LanguageModel))
    }

    /// Reports that KlingAI does not expose embedding models through this provider.
    pub fn embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleEmbeddingModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::EmbeddingModel))
    }

    /// Reports that KlingAI does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }
}

impl Default for KlingAIProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for KlingAIProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        KlingAIProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        KlingAIProvider::embedding_model(self, model_id)
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        KlingAIProvider::image_model(self, model_id)
    }
}

impl ProviderWithVideoModel for KlingAIProvider {
    type VideoModel = KlingAIVideoModel;

    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
        KlingAIProvider::video_model(self, model_id)
    }
}

impl KlingAIVideoModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: KlingAIProviderSettings,
        transport: KlingAITransport,
        current_date: KlingAIDateProvider,
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
        "klingai.video"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: KlingAITransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_generate_result(&self, options: VideoModelCallOptions) -> VideoModelResult {
        let timestamp = (self.current_date)();
        let abort_signal = options.abort_signal.clone();
        let (endpoint_path, request_body, warnings, poll_overrides) =
            match klingai_video_request_body(&self.model_id, &options) {
                Ok(args) => args,
                Err(error) => {
                    return klingai_video_result_from_error(
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
                return klingai_video_result_from_error(
                    &self.model_id,
                    error,
                    None,
                    warnings,
                    timestamp,
                );
            }
        };
        let create_url = format!("{}{}", self.base_url, endpoint_path);
        let transport = Arc::clone(&self.transport);
        let create_response = match post_json_to_api(
            PostJsonToApiOptions::new(create_url, request_body)
                .with_headers(request_headers.clone())
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(abort_signal.clone()),
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    klingai_create_task_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    klingai_error_response,
                    klingai_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                let (message, headers) = klingai_handled_error_parts(error);
                return klingai_video_result_from_error(
                    &self.model_id,
                    message,
                    headers,
                    warnings,
                    timestamp,
                );
            }
        };
        let Some(task_id) = create_response.value.data.and_then(|data| data.task_id) else {
            return klingai_video_result_from_error(
                &self.model_id,
                "No task_id returned from KlingAI API.".to_string(),
                create_response.response_headers,
                warnings,
                timestamp,
            );
        };

        let final_response = match self
            .poll_task_status(
                &endpoint_path,
                &task_id,
                request_headers,
                poll_overrides,
                abort_signal,
            )
            .await
        {
            Ok(response) => response,
            Err(error) => {
                return klingai_video_result_from_error(
                    &self.model_id,
                    error,
                    create_response.response_headers,
                    warnings,
                    timestamp,
                );
            }
        };

        klingai_video_result_from_response(
            &self.model_id,
            task_id,
            final_response.value,
            final_response.response_headers,
            warnings,
            timestamp,
        )
    }

    async fn poll_task_status(
        &self,
        endpoint_path: &str,
        task_id: &str,
        headers: BTreeMap<String, Option<String>>,
        overrides: KlingAIPollOverrides,
        abort_signal: Option<ProviderAbortSignal>,
    ) -> Result<ai_sdk_rust::ResponseHandlerResult<KlingAITaskStatusResponse>, String> {
        let poll_interval_millis = overrides
            .poll_interval_millis
            .unwrap_or(DEFAULT_KLINGAI_POLL_INTERVAL_MILLIS);
        let poll_timeout_millis = overrides
            .poll_timeout_millis
            .unwrap_or(DEFAULT_KLINGAI_POLL_TIMEOUT_MILLIS);
        let start = Instant::now();
        let status_url = format!(
            "{}{}{task_id}",
            self.base_url,
            with_trailing_slash(endpoint_path)
        );

        loop {
            let mut delay_options = DelayOptions::new();
            if let Some(abort_signal) = abort_signal.clone() {
                delay_options = delay_options.with_abort_signal(abort_signal);
            }
            delay_with_options(Some(poll_interval_millis as i64), delay_options)
                .await
                .map_err(|error| error.to_string())?;

            if start.elapsed().as_millis() > u128::from(poll_timeout_millis) {
                return Err(format!(
                    "Video generation timed out after {poll_timeout_millis}ms"
                ));
            }

            let transport = Arc::clone(&self.transport);
            let response = get_from_api(
                GetFromApiOptions::new(status_url.clone())
                    .with_headers(headers.clone())
                    .with_environment(RuntimeEnvironment::unknown())
                    .with_optional_abort_signal(abort_signal.clone()),
                move |request| (transport)(request),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        klingai_task_status_response,
                    )
                    .map_err(ProviderApiResponseHandlerError::from)
                },
                |request, response| {
                    Ok(create_json_error_response_handler(
                        response.json_error_response_handler_options(request),
                        klingai_error_response,
                        klingai_error_message,
                        |_, _| None,
                    ))
                },
            )
            .await
            .map_err(|error| klingai_handled_error_parts(error).0)?;

            match response
                .value
                .data
                .as_ref()
                .and_then(|data| data.task_status.as_deref())
            {
                Some("succeed") => return Ok(response),
                Some("failed") => {
                    let message = response
                        .value
                        .data
                        .as_ref()
                        .and_then(|data| data.task_status_msg.clone())
                        .unwrap_or_else(|| "Unknown error".to_string());
                    return Err(format!("Video generation failed: {message}"));
                }
                _ => {}
            }
        }
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
    ) -> Result<BTreeMap<String, Option<String>>, String> {
        Ok(combine_headers([
            Some(klingai_provider_header_entries(&self.settings)?),
            optional_headers(call_headers),
        ]))
    }
}

impl VideoModel for KlingAIVideoModel {
    type MaxVideosPerCallFuture<'a>
        = KlingAIVideoMaxVideosFuture<'a>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = KlingAIVideoGenerateFuture<'a>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        KlingAIVideoModel::provider(self)
    }

    fn model_id(&self) -> &str {
        KlingAIVideoModel::model_id(self)
    }

    fn max_videos_per_call(&self) -> Self::MaxVideosPerCallFuture<'_> {
        ready(Some(1))
    }

    fn do_generate(&self, options: VideoModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates a KlingAI provider with explicit settings.
pub fn create_klingai(settings: KlingAIProviderSettings) -> KlingAIProvider {
    KlingAIProvider::from_settings(settings)
}

/// Creates a KlingAI provider with default settings.
pub fn klingai() -> KlingAIProvider {
    KlingAIProvider::new()
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum KlingAIVideoMode {
    T2v,
    I2v,
    MotionControl,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KlingAIVideoModelOptions {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    poll_interval_ms: Option<u64>,
    #[serde(default)]
    poll_timeout_ms: Option<u64>,
    #[serde(default)]
    negative_prompt: Option<String>,
    #[serde(default)]
    sound: Option<String>,
    #[serde(default)]
    cfg_scale: Option<f64>,
    #[serde(default)]
    camera_control: Option<JsonValue>,
    #[serde(default)]
    multi_shot: Option<bool>,
    #[serde(default)]
    shot_type: Option<String>,
    #[serde(default)]
    multi_prompt: Option<JsonValue>,
    #[serde(default)]
    element_list: Option<JsonValue>,
    #[serde(default)]
    voice_list: Option<JsonValue>,
    #[serde(default)]
    image_tail: Option<String>,
    #[serde(default)]
    static_mask: Option<String>,
    #[serde(default)]
    dynamic_masks: Option<JsonValue>,
    #[serde(default)]
    video_url: Option<String>,
    #[serde(default)]
    character_orientation: Option<String>,
    #[serde(default)]
    keep_original_sound: Option<String>,
    #[serde(default)]
    watermark_enabled: Option<bool>,
    #[serde(flatten)]
    extra: JsonObject,
}

impl KlingAIVideoModelOptions {
    fn validate(&self) -> Result<(), &'static str> {
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
struct KlingAICreateTaskResponse {
    code: i64,
    message: String,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    data: Option<KlingAITaskData>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct KlingAITaskStatusResponse {
    code: i64,
    message: String,
    #[serde(default)]
    request_id: Option<String>,
    #[serde(default)]
    data: Option<KlingAITaskData>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct KlingAITaskData {
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    task_status: Option<String>,
    #[serde(default)]
    task_status_msg: Option<String>,
    #[serde(default)]
    task_result: Option<KlingAITaskResult>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct KlingAITaskResult {
    #[serde(default)]
    videos: Option<Vec<KlingAITaskVideo>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct KlingAITaskVideo {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    watermark_url: Option<String>,
    #[serde(default)]
    duration: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
struct KlingAIErrorResponse {
    code: i64,
    message: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct KlingAIPollOverrides {
    poll_interval_millis: Option<u64>,
    poll_timeout_millis: Option<u64>,
}

fn klingai_video_request_body(
    model_id: &str,
    options: &VideoModelCallOptions,
) -> Result<(String, JsonValue, Vec<Warning>, KlingAIPollOverrides), String> {
    let mode = detect_klingai_mode(model_id).map_err(|error| error.to_string())?;
    let provider_options = parse_provider_options(
        "klingai",
        Some(&options.provider_options),
        klingai_video_model_options,
    )
    .map_err(|error| error.to_string())?
    .unwrap_or_default();
    let mut warnings = Vec::new();
    let mut body = match mode {
        KlingAIVideoMode::T2v => {
            klingai_t2v_body(model_id, options, &provider_options, &mut warnings)
        }
        KlingAIVideoMode::I2v => {
            klingai_i2v_body(model_id, options, &provider_options, &mut warnings)
        }
        KlingAIVideoMode::MotionControl => {
            klingai_motion_control_body(model_id, options, &provider_options, &mut warnings)?
        }
    };

    if options.resolution.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "resolution".to_string(),
            details: Some("KlingAI video models do not support the resolution option.".to_string()),
        });
    }
    if options.seed.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "seed".to_string(),
            details: Some(
                "KlingAI video models do not support seed for deterministic generation."
                    .to_string(),
            ),
        });
    }
    if options.fps.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "fps".to_string(),
            details: Some("KlingAI video models do not support custom FPS.".to_string()),
        });
    }
    if options.n > 1 {
        warnings.push(Warning::Unsupported {
            feature: "n".to_string(),
            details: Some(
                "KlingAI video models do not support generating multiple videos per call. Only 1 video will be generated."
                    .to_string(),
            ),
        });
    }

    add_klingai_passthrough_options(&mut body, &provider_options);

    Ok((
        klingai_endpoint_path(&mode).to_string(),
        JsonValue::Object(body),
        warnings,
        KlingAIPollOverrides {
            poll_interval_millis: provider_options.poll_interval_ms,
            poll_timeout_millis: provider_options.poll_timeout_ms,
        },
    ))
}

fn klingai_t2v_body(
    model_id: &str,
    options: &VideoModelCallOptions,
    provider_options: &KlingAIVideoModelOptions,
    warnings: &mut Vec<Warning>,
) -> JsonObject {
    let mode = KlingAIVideoMode::T2v;
    let mut body = JsonObject::new();
    body.insert(
        "model_name".to_string(),
        JsonValue::String(klingai_api_model_name(model_id, &mode)),
    );
    insert_option_string_ref(&mut body, "prompt", options.prompt.as_ref());
    insert_option_string_ref(
        &mut body,
        "negative_prompt",
        provider_options.negative_prompt.as_ref(),
    );
    insert_option_string_ref(&mut body, "sound", provider_options.sound.as_ref());
    insert_option_f64(&mut body, "cfg_scale", provider_options.cfg_scale);
    insert_option_string_ref(&mut body, "mode", provider_options.mode.as_ref());
    insert_option_json(
        &mut body,
        "camera_control",
        provider_options.camera_control.clone(),
    );
    insert_option_string_ref(&mut body, "aspect_ratio", options.aspect_ratio.as_ref());
    if let Some(duration) = options.duration {
        body.insert(
            "duration".to_string(),
            JsonValue::String(duration.to_string()),
        );
    }
    insert_option_bool(&mut body, "multi_shot", provider_options.multi_shot);
    insert_option_string_ref(&mut body, "shot_type", provider_options.shot_type.as_ref());
    insert_option_json(
        &mut body,
        "multi_prompt",
        provider_options.multi_prompt.clone(),
    );
    insert_option_json(&mut body, "voice_list", provider_options.voice_list.clone());
    insert_watermark_info(&mut body, provider_options.watermark_enabled);

    if options.image.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "image".to_string(),
            details: Some(
                "KlingAI text-to-video does not support image input. Use an image-to-video model instead."
                    .to_string(),
            ),
        });
    }

    body
}

fn klingai_i2v_body(
    model_id: &str,
    options: &VideoModelCallOptions,
    provider_options: &KlingAIVideoModelOptions,
    warnings: &mut Vec<Warning>,
) -> JsonObject {
    let mode = KlingAIVideoMode::I2v;
    let mut body = JsonObject::new();
    body.insert(
        "model_name".to_string(),
        JsonValue::String(klingai_api_model_name(model_id, &mode)),
    );
    insert_option_string_ref(&mut body, "prompt", options.prompt.as_ref());
    if let Some(image) = options.image.as_ref() {
        body.insert(
            "image".to_string(),
            JsonValue::String(klingai_file_input(image)),
        );
    }
    insert_option_string_ref(
        &mut body,
        "image_tail",
        provider_options.image_tail.as_ref(),
    );
    insert_option_string_ref(
        &mut body,
        "negative_prompt",
        provider_options.negative_prompt.as_ref(),
    );
    insert_option_string_ref(&mut body, "sound", provider_options.sound.as_ref());
    insert_option_f64(&mut body, "cfg_scale", provider_options.cfg_scale);
    insert_option_string_ref(&mut body, "mode", provider_options.mode.as_ref());
    insert_option_json(
        &mut body,
        "camera_control",
        provider_options.camera_control.clone(),
    );
    insert_option_string_ref(
        &mut body,
        "static_mask",
        provider_options.static_mask.as_ref(),
    );
    insert_option_json(
        &mut body,
        "dynamic_masks",
        provider_options.dynamic_masks.clone(),
    );
    insert_option_bool(&mut body, "multi_shot", provider_options.multi_shot);
    insert_option_string_ref(&mut body, "shot_type", provider_options.shot_type.as_ref());
    insert_option_json(
        &mut body,
        "multi_prompt",
        provider_options.multi_prompt.clone(),
    );
    insert_option_json(
        &mut body,
        "element_list",
        provider_options.element_list.clone(),
    );
    insert_option_json(&mut body, "voice_list", provider_options.voice_list.clone());
    insert_watermark_info(&mut body, provider_options.watermark_enabled);
    if let Some(duration) = options.duration {
        body.insert(
            "duration".to_string(),
            JsonValue::String(duration.to_string()),
        );
    }
    if options.aspect_ratio.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "aspectRatio".to_string(),
            details: Some(
                "KlingAI image-to-video does not support aspectRatio. The output dimensions are determined by the input image."
                    .to_string(),
            ),
        });
    }

    body
}

fn klingai_motion_control_body(
    model_id: &str,
    options: &VideoModelCallOptions,
    provider_options: &KlingAIVideoModelOptions,
    warnings: &mut Vec<Warning>,
) -> Result<JsonObject, String> {
    let video_url = provider_options.video_url.as_ref().ok_or_else(|| {
        "KlingAI Motion Control requires providerOptions.klingai with videoUrl, characterOrientation, and mode."
            .to_string()
    })?;
    let character_orientation = provider_options.character_orientation.as_ref().ok_or_else(|| {
        "KlingAI Motion Control requires providerOptions.klingai with videoUrl, characterOrientation, and mode."
            .to_string()
    })?;
    let option_mode = provider_options.mode.as_ref().ok_or_else(|| {
        "KlingAI Motion Control requires providerOptions.klingai with videoUrl, characterOrientation, and mode."
            .to_string()
    })?;
    let mode = KlingAIVideoMode::MotionControl;
    let mut body = JsonObject::new();
    body.insert(
        "model_name".to_string(),
        JsonValue::String(klingai_api_model_name(model_id, &mode)),
    );
    body.insert(
        "video_url".to_string(),
        JsonValue::String(video_url.clone()),
    );
    body.insert(
        "character_orientation".to_string(),
        JsonValue::String(character_orientation.clone()),
    );
    body.insert("mode".to_string(), JsonValue::String(option_mode.clone()));
    insert_option_string_ref(&mut body, "prompt", options.prompt.as_ref());
    if let Some(image) = options.image.as_ref() {
        body.insert(
            "image_url".to_string(),
            JsonValue::String(klingai_file_input(image)),
        );
    }
    insert_option_string_ref(
        &mut body,
        "keep_original_sound",
        provider_options.keep_original_sound.as_ref(),
    );
    insert_watermark_info(&mut body, provider_options.watermark_enabled);
    insert_option_json(
        &mut body,
        "element_list",
        provider_options.element_list.clone(),
    );

    if options.aspect_ratio.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "aspectRatio".to_string(),
            details: Some(
                "KlingAI Motion Control does not support aspectRatio. The output dimensions are determined by the reference image/video."
                    .to_string(),
            ),
        });
    }
    if options.duration.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "duration".to_string(),
            details: Some(
                "KlingAI Motion Control does not support custom duration. The output duration matches the reference video duration."
                    .to_string(),
            ),
        });
    }

    Ok(body)
}

fn add_klingai_passthrough_options(body: &mut JsonObject, options: &KlingAIVideoModelOptions) {
    let handled = klingai_handled_options();
    for (key, value) in &options.extra {
        if !handled.contains(key.as_str()) {
            body.insert(key.clone(), value.clone());
        }
    }
}

fn klingai_video_model_options(value: &JsonValue) -> Result<KlingAIVideoModelOptions, String> {
    let options = serde_json::from_value::<KlingAIVideoModelOptions>(value.clone())
        .map_err(|error| error.to_string())?;
    options.validate().map_err(str::to_string)?;
    Ok(options)
}

fn detect_klingai_mode(model_id: &str) -> Result<KlingAIVideoMode, NoSuchModelError> {
    if model_id.ends_with("-t2v") {
        Ok(KlingAIVideoMode::T2v)
    } else if model_id.ends_with("-i2v") {
        Ok(KlingAIVideoMode::I2v)
    } else if model_id.ends_with("-motion-control") {
        Ok(KlingAIVideoMode::MotionControl)
    } else {
        Err(NoSuchModelError::new(model_id, ModelType::VideoModel))
    }
}

fn klingai_endpoint_path(mode: &KlingAIVideoMode) -> &'static str {
    match mode {
        KlingAIVideoMode::T2v => "/v1/videos/text2video",
        KlingAIVideoMode::I2v => "/v1/videos/image2video",
        KlingAIVideoMode::MotionControl => "/v1/videos/motion-control",
    }
}

fn klingai_api_model_name(model_id: &str, mode: &KlingAIVideoMode) -> String {
    let suffix = match mode {
        KlingAIVideoMode::T2v => "-t2v",
        KlingAIVideoMode::I2v => "-i2v",
        KlingAIVideoMode::MotionControl => "-motion-control",
    };
    model_id
        .strip_suffix(suffix)
        .unwrap_or(model_id)
        .strip_suffix(".0")
        .unwrap_or_else(|| model_id.strip_suffix(suffix).unwrap_or(model_id))
        .replace('.', "-")
}

fn klingai_handled_options() -> BTreeSet<&'static str> {
    [
        "mode",
        "pollIntervalMs",
        "pollTimeoutMs",
        "negativePrompt",
        "sound",
        "cfgScale",
        "cameraControl",
        "multiShot",
        "shotType",
        "multiPrompt",
        "elementList",
        "voiceList",
        "imageTail",
        "staticMask",
        "dynamicMasks",
        "videoUrl",
        "characterOrientation",
        "keepOriginalSound",
        "watermarkEnabled",
    ]
    .into_iter()
    .collect()
}

fn klingai_file_input(file: &VideoModelFile) -> String {
    match file {
        VideoModelFile::Url { url, .. } => url.as_str().to_string(),
        VideoModelFile::File { data, .. } => convert_to_base64(data),
    }
}

fn insert_watermark_info(body: &mut JsonObject, enabled: Option<bool>) {
    if let Some(enabled) = enabled {
        let mut watermark = JsonObject::new();
        watermark.insert("enabled".to_string(), JsonValue::Bool(enabled));
        body.insert("watermark_info".to_string(), JsonValue::Object(watermark));
    }
}

fn klingai_provider_header_entries(
    settings: &KlingAIProviderSettings,
) -> Result<Vec<(String, Option<String>)>, String> {
    let token = generate_klingai_auth_token(settings).map_err(|error| error.to_string())?;
    let mut headers = vec![("Authorization".to_string(), Some(format!("Bearer {token}")))];

    headers.extend(
        settings
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    );

    Ok(with_user_agent_suffix(
        Some(headers),
        [format!("ai-sdk/klingai/{}", ai_sdk_rust::VERSION)],
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

fn generate_klingai_auth_token(
    settings: &KlingAIProviderSettings,
) -> Result<String, KlingAIAuthError> {
    let access_key = load_klingai_access_key(settings)?;
    let secret_key = load_klingai_secret_key(settings)?;
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let header = serde_json::to_string(&serde_json::json!({"alg": "HS256", "typ": "JWT"}))
        .expect("JWT header serializes");
    let payload = serde_json::to_string(&serde_json::json!({
        "iss": access_key,
        "exp": now + 1800,
        "nbf": now - 5,
    }))
    .expect("JWT payload serializes");
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.as_bytes()),
        URL_SAFE_NO_PAD.encode(payload.as_bytes())
    );
    let mut mac = HmacSha256::new_from_slice(secret_key.as_bytes())
        .map_err(|error| KlingAIAuthError(error.to_string()))?;
    mac.update(signing_input.as_bytes());
    let signature = mac.finalize().into_bytes();

    Ok(format!(
        "{}.{}",
        signing_input,
        URL_SAFE_NO_PAD.encode(signature)
    ))
}

fn load_klingai_access_key(settings: &KlingAIProviderSettings) -> Result<String, KlingAIAuthError> {
    let mut options =
        LoadSettingOptions::new("KLINGAI_ACCESS_KEY", "accessKey", "KlingAI access key");
    if let Some(access_key) = settings.access_key.as_ref() {
        options = options.with_setting_value(access_key.clone());
    }
    load_setting(options).map_err(KlingAIAuthError::from)
}

fn load_klingai_secret_key(settings: &KlingAIProviderSettings) -> Result<String, KlingAIAuthError> {
    let mut options =
        LoadSettingOptions::new("KLINGAI_SECRET_KEY", "secretKey", "KlingAI secret key");
    if let Some(secret_key) = settings.secret_key.as_ref() {
        options = options.with_setting_value(secret_key.clone());
    }
    load_setting(options).map_err(KlingAIAuthError::from)
}

#[derive(Debug)]
struct KlingAIAuthError(String);

impl std::fmt::Display for KlingAIAuthError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<LoadSettingError> for KlingAIAuthError {
    fn from(error: LoadSettingError) -> Self {
        Self(error.to_string())
    }
}

fn klingai_create_task_response(
    value: &JsonValue,
) -> Result<KlingAICreateTaskResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn klingai_task_status_response(
    value: &JsonValue,
) -> Result<KlingAITaskStatusResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn klingai_error_response(value: &JsonValue) -> Result<KlingAIErrorResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn klingai_error_message(error: &KlingAIErrorResponse) -> String {
    error.message.clone()
}

fn klingai_handled_error_parts(error: HandledFetchError) -> (String, Option<Headers>) {
    match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
        ),
    }
}

fn klingai_video_result_from_response(
    model_id: &str,
    task_id: String,
    response: KlingAITaskStatusResponse,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> VideoModelResult {
    let videos = response
        .data
        .and_then(|data| data.task_result)
        .and_then(|result| result.videos)
        .unwrap_or_default();
    let mut video_data = Vec::new();
    let mut video_metadata = Vec::new();

    for video in videos {
        let Some(video_url) = video.url else {
            continue;
        };
        let Ok(url) = Url::parse(&video_url) else {
            continue;
        };
        video_data.push(VideoModelVideoData::url(url, "video/mp4"));
        let mut metadata = JsonObject::new();
        metadata.insert(
            "id".to_string(),
            JsonValue::String(video.id.unwrap_or_default()),
        );
        metadata.insert("url".to_string(), JsonValue::String(video_url));
        insert_option_json_string(&mut metadata, "watermarkUrl", video.watermark_url);
        insert_option_json_string(&mut metadata, "duration", video.duration);
        video_metadata.push(JsonValue::Object(metadata));
    }

    if video_data.is_empty() {
        return klingai_video_result_from_error(
            model_id,
            "No valid video URLs in response".to_string(),
            headers,
            warnings,
            timestamp,
        );
    }

    let mut provider = JsonObject::new();
    provider.insert("taskId".to_string(), JsonValue::String(task_id));
    provider.insert("videos".to_string(), JsonValue::Array(video_metadata));

    let mut result = VideoModelResult::new(
        video_data,
        klingai_video_response_metadata(model_id, headers, timestamp),
    )
    .with_provider_metadata(ProviderMetadata::from([("klingai".to_string(), provider)]));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn klingai_video_result_from_error(
    model_id: &str,
    message: String,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
    timestamp: OffsetDateTime,
) -> VideoModelResult {
    let mut result = VideoModelResult::new(
        Vec::new(),
        klingai_video_response_metadata(model_id, headers, timestamp),
    )
    .with_provider_metadata(ProviderMetadata::from([(
        "klingai".to_string(),
        object_with_string("errorMessage", message),
    )]));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn klingai_video_response_metadata(
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

fn with_trailing_slash(value: &str) -> String {
    if value.ends_with('/') {
        value.to_string()
    } else {
        format!("{value}/")
    }
}

fn insert_option_string_ref(body: &mut JsonObject, name: &str, value: Option<&String>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::String(value.clone()));
    }
}

fn insert_option_json_string(body: &mut JsonObject, name: &str, value: Option<String>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::String(value));
    }
}

fn insert_option_bool(body: &mut JsonObject, name: &str, value: Option<bool>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::Bool(value));
    }
}

fn insert_option_f64(body: &mut JsonObject, name: &str, value: Option<f64>) {
    if let Some(value) = value {
        body.insert(name.to_string(), JsonValue::from(value));
    }
}

fn insert_option_json(body: &mut JsonObject, name: &str, value: Option<JsonValue>) {
    if let Some(value) = value {
        body.insert(name.to_string(), value);
    }
}

fn object_with_string(name: &str, value: impl Into<String>) -> JsonObject {
    let mut object = JsonObject::new();
    object.insert(name.to_string(), JsonValue::String(value.into()));
    object
}

fn default_klingai_transport() -> KlingAITransport {
    Arc::new(|request| Box::pin(ready(execute_klingai_request(request))))
}

fn execute_klingai_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_klingai_get_request(request),
        ProviderApiRequestMethod::Post => execute_klingai_post_request(request),
    }
}

fn execute_klingai_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    klingai_provider_api_response(builder.config().http_status_as_error(false).build().call())
}

fn execute_klingai_post_request(
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
                "multipart form data is not supported by the KlingAI transport",
            ));
        }
        None => builder.send_empty(),
    };
    klingai_provider_api_response(response)
}

fn klingai_provider_api_response(
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
        KlingAIProviderSettings, KlingAITransport, KlingAITransportFuture, URL_SAFE_NO_PAD,
        create_klingai, generate_klingai_auth_token, klingai_video_request_body,
    };
    use ai_sdk_rust::{
        FileDataContent, ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod,
        ProviderApiResponse, ProviderOptions, VideoModel, VideoModelCallOptions, VideoModelFile,
        Warning,
    };
    use base64::Engine;
    use hmac::Mac;
    use serde_json::json;
    use std::env;
    use std::future::Future;
    use std::future::ready;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use std::thread;
    use std::time::Duration;
    use time::OffsetDateTime;

    struct NoopWake;

    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    fn block_on<F>(future: F) -> F::Output
    where
        F: Future,
    {
        let waker = Waker::from(Arc::new(NoopWake));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => thread::sleep(Duration::from_millis(1)),
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

    fn decode_jwt_part(token: &str, index: usize) -> serde_json::Value {
        let part = token
            .split('.')
            .nth(index)
            .expect("JWT contains requested part");
        let bytes = URL_SAFE_NO_PAD.decode(part).expect("JWT part decodes");
        serde_json::from_slice(&bytes).expect("JWT part is JSON")
    }

    #[test]
    #[ignore = "requires KLINGAI_ACCESS_KEY/KLINGAI_SECRET_KEY and performs live KlingAI video generation"]
    fn live_klingai_video_generation_validates_provider_contract() {
        if env::var("KLINGAI_ACCESS_KEY").is_err() || env::var("KLINGAI_SECRET_KEY").is_err() {
            eprintln!(
                "skipping live KlingAI test: KLINGAI_ACCESS_KEY/KLINGAI_SECRET_KEY is not set"
            );
            return;
        }

        let result = block_on(
            create_klingai(KlingAIProviderSettings::new())
                .video("kling-v2.6-t2v")
                .do_generate(VideoModelCallOptions::new(1).with_prompt("A small blue cube")),
        );

        assert!(!result.videos.is_empty());
        assert_eq!(result.response.model_id, "kling-v2.6-t2v");
    }

    #[test]
    fn klingai_auth_generates_valid_hs256_jwt_structure() {
        let token = generate_klingai_auth_token(
            &KlingAIProviderSettings::new()
                .with_access_key("access-key")
                .with_secret_key("secret-key"),
        )
        .expect("token generated");

        assert_eq!(token.split('.').count(), 3);
        assert_eq!(
            decode_jwt_part(&token, 0),
            json!({
                "alg": "HS256",
                "typ": "JWT"
            })
        );
    }

    #[test]
    fn klingai_auth_includes_issuer_exp_and_nbf_claims() {
        let token = generate_klingai_auth_token(
            &KlingAIProviderSettings::new()
                .with_access_key("access-key")
                .with_secret_key("secret-key"),
        )
        .expect("token generated");
        let payload = decode_jwt_part(&token, 1);

        assert_eq!(payload["iss"], json!("access-key"));
        let exp = payload["exp"].as_i64().expect("exp claim");
        let nbf = payload["nbf"].as_i64().expect("nbf claim");
        assert_eq!(exp - nbf, 1805);
    }

    #[test]
    fn klingai_auth_signs_with_secret_and_changes_for_different_secret() {
        let settings = KlingAIProviderSettings::new()
            .with_access_key("access-key")
            .with_secret_key("secret-one");
        let token = generate_klingai_auth_token(&settings).expect("token generated");
        let mut parts = token.split('.');
        let header = parts.next().expect("header");
        let payload = parts.next().expect("payload");
        let signature = parts.next().expect("signature");
        let signing_input = format!("{header}.{payload}");
        let mut mac = super::HmacSha256::new_from_slice("secret-one".as_bytes()).expect("HMAC key");
        mac.update(signing_input.as_bytes());
        let expected_signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());

        let different_secret_token = generate_klingai_auth_token(
            &KlingAIProviderSettings::new()
                .with_access_key("access-key")
                .with_secret_key("secret-two"),
        )
        .expect("token generated with different secret");

        assert_eq!(signature, expected_signature);
        assert_ne!(
            token.split('.').nth(2),
            different_secret_token.split('.').nth(2)
        );
    }

    #[test]
    fn klingai_auth_reports_missing_explicit_credentials() {
        assert!(generate_klingai_auth_token(&KlingAIProviderSettings::new()).is_err());
        assert!(
            generate_klingai_auth_token(
                &KlingAIProviderSettings::new().with_access_key("access-only")
            )
            .is_err()
        );
    }

    #[test]
    fn klingai_t2v_model_maps_extended_provider_options_without_element_list() {
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "klingai".to_string(),
            serde_json::from_value(json!({
                "negativePrompt": "rain",
                "sound": "on",
                "cfgScale": 0.5,
                "cameraControl": {"type": "simple", "config": {"horizontal": 10}},
                "multiShot": true,
                "shotType": "intelligence",
                "multiPrompt": ["wide", "close"],
                "elementList": [{"id": "ignored-for-t2v"}],
                "voiceList": [{"voice_id": "voice-1"}],
                "watermarkEnabled": true
            }))
            .expect("provider options deserialize"),
        );

        let (_endpoint, body, warnings, _poll) = klingai_video_request_body(
            "kling-v3.0-t2v",
            &VideoModelCallOptions::new(1)
                .with_prompt("A city")
                .with_aspect_ratio("16:9")
                .with_duration(5.0)
                .with_provider_options(provider_options),
        )
        .expect("request body maps");

        assert_eq!(
            body,
            json!({
                "model_name": "kling-v3",
                "prompt": "A city",
                "negative_prompt": "rain",
                "sound": "on",
                "cfg_scale": 0.5,
                "camera_control": {"type": "simple", "config": {"horizontal": 10}},
                "aspect_ratio": "16:9",
                "duration": "5",
                "multi_shot": true,
                "shot_type": "intelligence",
                "multi_prompt": ["wide", "close"],
                "voice_list": [{"voice_id": "voice-1"}],
                "watermark_info": {"enabled": true}
            })
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn klingai_i2v_model_maps_tail_masks_multishot_voice_and_watermark_options() {
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "klingai".to_string(),
            serde_json::from_value(json!({
                "imageTail": "https://example.com/tail.png",
                "staticMask": "https://example.com/static.png",
                "dynamicMasks": [{"url": "https://example.com/dynamic.png"}],
                "multiShot": true,
                "multiPrompt": ["first", "second"],
                "elementList": [{"id": "element-1"}],
                "voiceList": [{"voice_id": "voice-1"}],
                "watermarkEnabled": false,
                "negativePrompt": "blur"
            }))
            .expect("provider options deserialize"),
        );

        let (_endpoint, body, warnings, _poll) = klingai_video_request_body(
            "kling-v3.0-i2v",
            &VideoModelCallOptions::new(1)
                .with_prompt("Move")
                .with_duration(10.0)
                .with_image(VideoModelFile::url(
                    url::Url::parse("https://example.com/input.png").expect("valid URL"),
                ))
                .with_provider_options(provider_options),
        )
        .expect("request body maps");

        assert_eq!(body["model_name"], json!("kling-v3"));
        assert_eq!(body["image"], json!("https://example.com/input.png"));
        assert_eq!(body["image_tail"], json!("https://example.com/tail.png"));
        assert_eq!(body["static_mask"], json!("https://example.com/static.png"));
        assert_eq!(
            body["dynamic_masks"],
            json!([{"url": "https://example.com/dynamic.png"}])
        );
        assert_eq!(body["multi_shot"], json!(true));
        assert_eq!(body["multi_prompt"], json!(["first", "second"]));
        assert_eq!(body["element_list"], json!([{"id": "element-1"}]));
        assert_eq!(body["voice_list"], json!([{"voice_id": "voice-1"}]));
        assert_eq!(body["watermark_info"], json!({"enabled": false}));
        assert!(warnings.is_empty());
    }

    #[test]
    fn klingai_motion_control_maps_required_provider_options_and_image_url() {
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "klingai".to_string(),
            serde_json::from_value(json!({
                "videoUrl": "https://example.com/source.mp4",
                "characterOrientation": "forward",
                "mode": "pro",
                "keepOriginalSound": "true",
                "elementList": [{"id": "element-1"}]
            }))
            .expect("provider options deserialize"),
        );

        let (endpoint, body, warnings, _poll) = klingai_video_request_body(
            "kling-v3.0-motion-control",
            &VideoModelCallOptions::new(1)
                .with_prompt("Dance")
                .with_image(VideoModelFile::url(
                    url::Url::parse("https://example.com/reference.png").expect("valid URL"),
                ))
                .with_provider_options(provider_options),
        )
        .expect("request body maps");

        assert_eq!(endpoint, "/v1/videos/motion-control");
        assert_eq!(
            body,
            json!({
                "model_name": "kling-v3",
                "video_url": "https://example.com/source.mp4",
                "character_orientation": "forward",
                "mode": "pro",
                "prompt": "Dance",
                "image_url": "https://example.com/reference.png",
                "keep_original_sound": "true",
                "element_list": [{"id": "element-1"}]
            })
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn klingai_t2v_model_maps_body_headers_polling_and_metadata() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: KlingAITransport = Arc::new(move |request| -> KlingAITransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());

            let response = match (request.method, request.url.as_str()) {
                (
                    ProviderApiRequestMethod::Post,
                    "https://api.example.com/v1/videos/text2video",
                ) => json_response(json!({
                    "code": 0,
                    "message": "success",
                    "data": { "task_id": "task-123", "task_status": "submitted" }
                })),
                (
                    ProviderApiRequestMethod::Get,
                    "https://api.example.com/v1/videos/text2video/task-123",
                ) => json_response(json!({
                    "code": 0,
                    "message": "success",
                    "data": {
                        "task_id": "task-123",
                        "task_status": "succeed",
                        "task_result": {
                            "videos": [{
                                "id": "video-1",
                                "url": "https://kling.example/video.mp4",
                                "watermark_url": "https://kling.example/watermark.mp4",
                                "duration": "5"
                            }]
                        }
                    }
                }))
                .with_headers(
                    [("x-task-id".to_string(), "task-123".to_string())]
                        .into_iter()
                        .collect(),
                ),
                _ => ProviderApiResponse::text(
                    404,
                    "Not Found",
                    json!({"code": 404, "message": "unexpected request"}).to_string(),
                ),
            };

            Box::pin(ready(Ok(response)))
        });
        let provider = create_klingai(
            KlingAIProviderSettings::new()
                .with_access_key("access")
                .with_secret_key("secret")
                .with_base_url("https://api.example.com")
                .with_header("x-provider-header", "provider"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "klingai".to_string(),
            serde_json::from_value(json!({
                "mode": "pro",
                "negativePrompt": "rain",
                "sound": "on",
                "cfgScale": 0.7,
                "watermarkEnabled": false,
                "pollIntervalMs": 1,
                "pollTimeoutMs": 100
            }))
            .expect("provider options deserialize"),
        );

        let result = block_on(
            provider.video("kling-v2.6-t2v").do_generate(
                VideoModelCallOptions::new(2)
                    .with_prompt("A cyclist")
                    .with_aspect_ratio("16:9")
                    .with_duration(5.0)
                    .with_seed(42)
                    .with_fps(30.0)
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.videos.len(), 1);
        assert_eq!(result.response.timestamp, fixed_timestamp());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("klingai"))
                .and_then(|metadata| metadata.get("taskId")),
            Some(&json!("task-123"))
        );
        assert_eq!(result.warnings.len(), 3);
        assert!(result.warnings.iter().any(|warning| {
            matches!(
                warning,
                Warning::Unsupported {
                    feature,
                    ..
                } if feature == "seed"
            )
        }));

        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert!(requests[0].headers.get("authorization").is_some());
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "model_name": "kling-v2-6",
                "prompt": "A cyclist",
                "negative_prompt": "rain",
                "sound": "on",
                "cfg_scale": 0.7,
                "mode": "pro",
                "aspect_ratio": "16:9",
                "duration": "5",
                "watermark_info": { "enabled": false }
            })
        );
    }

    #[test]
    fn klingai_i2v_model_maps_file_image_and_aspect_ratio_warning() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_transport = Arc::clone(&requests);
        let transport: KlingAITransport = Arc::new(move |request| -> KlingAITransportFuture {
            requests_for_transport
                .lock()
                .expect("request list mutex is not poisoned")
                .push(request.clone());

            let response = match request.method {
                ProviderApiRequestMethod::Post => json_response(json!({
                    "code": 0,
                    "message": "success",
                    "data": { "task_id": "task-123" }
                })),
                ProviderApiRequestMethod::Get => json_response(json!({
                    "code": 0,
                    "message": "success",
                    "data": {
                        "task_id": "task-123",
                        "task_status": "succeed",
                        "task_result": {
                            "videos": [{ "id": "video-1", "url": "https://kling.example/video.mp4" }]
                        }
                    }
                })),
            };
            Box::pin(ready(Ok(response)))
        });
        let provider = create_klingai(
            KlingAIProviderSettings::new()
                .with_access_key("access")
                .with_secret_key("secret")
                .with_base_url("https://api.example.com"),
        )
        .with_transport(transport);
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(
            "klingai".to_string(),
            serde_json::from_value(json!({
                "imageTail": "tail-image",
                "staticMask": "mask",
                "pollIntervalMs": 1,
                "pollTimeoutMs": 100
            }))
            .expect("provider options deserialize"),
        );

        let result = block_on(
            provider.video("kling-v2.1-master-i2v").do_generate(
                VideoModelCallOptions::new(1)
                    .with_prompt("Move")
                    .with_aspect_ratio("1:1")
                    .with_image(VideoModelFile::file(
                        "image/png",
                        FileDataContent::Bytes(vec![1, 2, 3]),
                    ))
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(result.videos.len(), 1);
        assert!(matches!(result.warnings[0], Warning::Unsupported { .. }));
        let requests = requests.lock().expect("request list mutex is not poisoned");
        assert_eq!(
            requests[0].url,
            "https://api.example.com/v1/videos/image2video"
        );
        assert_eq!(request_body_json(&requests[0])["image"], json!("AQID"));
    }

    #[test]
    fn klingai_video_model_maps_api_errors_to_provider_metadata() {
        let transport: KlingAITransport = Arc::new(move |_| -> KlingAITransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                429,
                "Too Many Requests",
                json!({
                    "code": 429,
                    "message": "rate limit exceeded",
                })
                .to_string(),
            )
            .with_headers(
                [("x-request-id".to_string(), "req-error".to_string())]
                    .into_iter()
                    .collect(),
            ))))
        });
        let provider = create_klingai(
            KlingAIProviderSettings::new()
                .with_access_key("access")
                .with_secret_key("secret")
                .with_base_url("https://api.example.com"),
        )
        .with_transport(transport)
        .with_current_date(fixed_timestamp);

        let result = block_on(
            provider
                .video("kling-v2.6-t2v")
                .do_generate(VideoModelCallOptions::new(1).with_prompt("A cyclist")),
        );

        assert!(result.videos.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("klingai"))
                .and_then(|metadata| metadata.get("errorMessage")),
            Some(&json!("rate limit exceeded"))
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id")),
            Some(&"req-error".to_string())
        );
    }

    #[test]
    fn klingai_motion_control_requires_provider_options() {
        let provider = create_klingai(
            KlingAIProviderSettings::new()
                .with_access_key("access")
                .with_secret_key("secret")
                .with_base_url("https://api.example.com"),
        );

        let result = block_on(
            provider
                .video("kling-v3.0-motion-control")
                .do_generate(VideoModelCallOptions::new(1).with_prompt("Dance")),
        );

        assert!(result.videos.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("klingai"))
                .and_then(|metadata| metadata.get("errorMessage")),
            Some(&json!(
                "KlingAI Motion Control requires providerOptions.klingai with videoUrl, characterOrientation, and mode."
            ))
        );
    }
}
