use std::collections::BTreeMap;
use std::env;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::file_data::FileDataContent;
use crate::headers::Headers;
use crate::image_model::{
    ImageModel, ImageModelCallOptions, ImageModelProviderMetadata, ImageModelProviderMetadataEntry,
    ImageModelResponse, ImageModelResult,
};
use crate::json::{JsonObject, JsonValue};
use crate::openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleCompletionLanguageModel,
    OpenAICompatibleEmbeddingModel, OpenAICompatibleProvider, OpenAICompatibleProviderSettings,
    OpenAICompatibleTransport,
};
use crate::provider::{NoSuchModelError, Provider};
use crate::provider_utils::{
    FetchErrorInfo, GetFromApiOptions, HandledFetchError, PostJsonToApiOptions, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, RuntimeEnvironment, combine_headers,
    convert_image_model_file_to_data_uri, create_binary_response_handler,
    create_json_response_handler, create_status_code_error_response_handler, get_from_api,
    post_json_to_api, with_user_agent_suffix, without_trailing_slash,
};
use crate::warning::Warning;

/// Default base URL for upstream `@ai-sdk/fireworks` API calls.
pub const DEFAULT_FIREWORKS_BASE_URL: &str = "https://api.fireworks.ai/inference/v1";

const DEFAULT_FIREWORKS_POLL_ATTEMPTS: usize = 240;

/// Settings for the upstream Fireworks provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FireworksProviderSettings {
    /// Base URL for Fireworks API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Fireworks API key. When omitted, `FIREWORKS_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl FireworksProviderSettings {
    /// Creates empty Fireworks provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Fireworks API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the Fireworks API key.
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

/// Upstream Fireworks provider foundation.
#[derive(Clone)]
pub struct FireworksProvider {
    settings: FireworksProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

/// Fireworks image model for provider-specific workflow/image-generation routes.
#[derive(Clone)]
pub struct FireworksImageModel {
    model_id: String,
    base_url: String,
    settings: FireworksProviderSettings,
    transport: OpenAICompatibleTransport,
}

impl FireworksProvider {
    /// Creates a Fireworks provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(FireworksProviderSettings::new())
    }

    /// Creates a provider from explicit Fireworks settings.
    pub fn from_settings(settings: FireworksProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the Fireworks API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the Fireworks API base URL for this provider.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.settings.base_url = Some(base_url.into());
        self
    }

    /// Adds a provider-level request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.settings.headers.insert(name.into(), value.into());
        self
    }

    /// Replaces the HTTP transport. This is primarily useful for tests.
    pub fn with_transport(mut self, transport: OpenAICompatibleTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Creates a Fireworks chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Creates a Fireworks chat language model.
    pub fn chat_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.openai_compatible_provider().chat_model(model_id)
    }

    /// Creates a Fireworks completion language model.
    pub fn completion_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleCompletionLanguageModel {
        self.openai_compatible_provider().completion_model(model_id)
    }

    /// Creates a Fireworks embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.openai_compatible_provider().embedding_model(model_id)
    }

    /// Deprecated upstream alias for [`FireworksProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Creates a Fireworks image model over the provider-specific workflow/image routes.
    pub fn image_model(&self, model_id: impl Into<String>) -> FireworksImageModel {
        FireworksImageModel::new(
            model_id,
            fireworks_base_url(&self.settings),
            self.settings.clone(),
            self.transport
                .as_ref()
                .map(Arc::clone)
                .unwrap_or_else(default_fireworks_transport),
        )
    }

    /// Alias for [`FireworksProvider::image_model`].
    pub fn image(&self, model_id: impl Into<String>) -> FireworksImageModel {
        self.image_model(model_id)
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings =
            OpenAICompatibleProviderSettings::new("fireworks", fireworks_base_url(&self.settings))
                .with_transform_request_body(transform_fireworks_chat_request_body)
                .with_error_to_message(fireworks_error_to_message)
                .with_user_agent_suffix(format!("ai-sdk/fireworks/{}", crate::VERSION));

        if let Some(api_key) = fireworks_api_key(self.settings.api_key.as_ref()) {
            settings = settings.with_api_key(api_key);
        }

        for (name, value) in &self.settings.headers {
            settings = settings.with_header(name.clone(), value.clone());
        }

        let provider = OpenAICompatibleProvider::from_settings(settings);

        if let Some(transport) = &self.transport {
            provider.with_transport(Arc::clone(transport))
        } else {
            provider
        }
    }
}

impl Default for FireworksProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for FireworksProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = FireworksImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(FireworksProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(FireworksProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        Ok(FireworksProvider::image_model(self, model_id))
    }
}

impl FireworksImageModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: FireworksProviderSettings,
        transport: OpenAICompatibleTransport,
    ) -> Self {
        Self {
            model_id: model_id.into(),
            base_url: base_url.into(),
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
        "fireworks.image"
    }

    async fn do_generate_result(&self, options: ImageModelCallOptions) -> ImageModelResult {
        let timestamp = OffsetDateTime::now_utc();
        let warnings = fireworks_image_warnings(&self.model_id, &options);
        let request_body = fireworks_image_request_body(&self.model_id, &options);
        let request_headers = fireworks_image_headers(&self.settings, options.headers.as_ref());
        let abort_signal = options.abort_signal.clone();

        if fireworks_image_is_async_model(&self.model_id) {
            return self
                .do_generate_async_result(
                    timestamp,
                    request_body,
                    request_headers,
                    warnings,
                    abort_signal,
                )
                .await;
        }

        let post_options = PostJsonToApiOptions::new(
            fireworks_image_url(&self.base_url, &self.model_id),
            request_body,
        )
        .with_headers(request_headers)
        .with_environment(RuntimeEnvironment::unknown())
        .with_optional_abort_signal(abort_signal);
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
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
            Ok(response) => fireworks_image_result_from_bytes(
                &self.model_id,
                timestamp,
                response.value,
                response.response_headers,
                warnings,
            ),
            Err(error) => {
                fireworks_image_result_from_error(&self.model_id, timestamp, error, warnings)
            }
        }
    }

    async fn do_generate_async_result(
        &self,
        timestamp: OffsetDateTime,
        request_body: JsonValue,
        request_headers: impl IntoIterator<Item = (String, Option<String>)> + Clone,
        warnings: Vec<Warning>,
        abort_signal: Option<crate::language_model::ProviderAbortSignal>,
    ) -> ImageModelResult {
        let submit_transport = Arc::clone(&self.transport);
        let submit_options = PostJsonToApiOptions::new(
            fireworks_image_url(&self.base_url, &self.model_id),
            request_body,
        )
        .with_headers(request_headers.clone())
        .with_environment(RuntimeEnvironment::unknown())
        .with_optional_abort_signal(abort_signal.clone());
        let submit = match post_json_to_api(
            submit_options,
            move |request| (submit_transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    fireworks_async_submit_response,
                )
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
            Ok(response) => response.value,
            Err(error) => {
                return fireworks_image_result_from_error(
                    &self.model_id,
                    timestamp,
                    error,
                    warnings,
                );
            }
        };

        if submit.request_id.is_empty() {
            return fireworks_image_result_from_error_message(
                &self.model_id,
                timestamp,
                "Fireworks async submit response is missing request_id",
                warnings,
            );
        }

        let mut last_status = None::<String>;
        let mut image_url = None::<String>;

        for _ in 0..DEFAULT_FIREWORKS_POLL_ATTEMPTS {
            let poll_body =
                json_value_object([("id", JsonValue::String(submit.request_id.clone()))]);
            let poll_transport = Arc::clone(&self.transport);
            let poll_options = PostJsonToApiOptions::new(
                fireworks_image_poll_url(&self.base_url, &self.model_id),
                poll_body,
            )
            .with_headers(request_headers.clone())
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(abort_signal.clone());
            let poll = match post_json_to_api(
                poll_options,
                move |request| (poll_transport)(request),
                |request, response| {
                    create_json_response_handler(
                        response.json_response_handler_options(request),
                        fireworks_async_poll_response,
                    )
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
                Ok(response) => response.value,
                Err(error) => {
                    return fireworks_image_result_from_error(
                        &self.model_id,
                        timestamp,
                        error,
                        warnings,
                    );
                }
            };

            match poll.status.as_str() {
                "Ready" => {
                    if let Some(url) = poll.result.and_then(|result| result.sample) {
                        image_url = Some(url);
                        break;
                    }

                    return fireworks_image_result_from_error_message(
                        &self.model_id,
                        timestamp,
                        "Fireworks poll response is Ready but missing result.sample",
                        warnings,
                    );
                }
                "Error" | "Failed" => {
                    return fireworks_image_result_from_error_message(
                        &self.model_id,
                        timestamp,
                        format!(
                            "Fireworks image generation failed with status: {}",
                            poll.status
                        ),
                        warnings,
                    );
                }
                status => {
                    last_status = Some(status.to_string());
                }
            }
        }

        let Some(image_url) = image_url else {
            let status = last_status.unwrap_or_else(|| "unknown".to_string());
            return fireworks_image_result_from_error_message(
                &self.model_id,
                timestamp,
                format!(
                    "Fireworks image generation timed out after {DEFAULT_FIREWORKS_POLL_ATTEMPTS} polls; last status: {status}"
                ),
                warnings,
            );
        };

        let get_transport = Arc::clone(&self.transport);
        let get_options = GetFromApiOptions::new(image_url)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(abort_signal);

        match get_from_api(
            get_options,
            move |request| (get_transport)(request),
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
            Ok(response) => fireworks_image_result_from_bytes(
                &self.model_id,
                timestamp,
                response.value,
                response.response_headers,
                warnings,
            ),
            Err(error) => {
                fireworks_image_result_from_error(&self.model_id, timestamp, error, warnings)
            }
        }
    }
}

impl ImageModel for FireworksImageModel {
    type MaxImagesPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        FireworksImageModel::provider(self)
    }

    fn model_id(&self) -> &str {
        FireworksImageModel::model_id(self)
    }

    fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
        ready(Some(1))
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

/// Creates a Fireworks provider with explicit settings.
pub fn create_fireworks(settings: FireworksProviderSettings) -> FireworksProvider {
    FireworksProvider::from_settings(settings)
}

/// Creates a Fireworks chat language model using default provider settings.
pub fn fireworks(model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
    FireworksProvider::new().language_model(model_id)
}

fn fireworks_base_url(settings: &FireworksProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_FIREWORKS_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn fireworks_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("FIREWORKS_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn fireworks_error_to_message(value: &JsonValue) -> Option<String> {
    value
        .get("error")
        .and_then(JsonValue::as_str)
        .map(str::to_string)
}

fn transform_fireworks_chat_request_body(value: JsonValue) -> JsonValue {
    let JsonValue::Object(mut body) = value else {
        return value;
    };

    if let Some(JsonValue::Object(thinking)) = body.remove("thinking") {
        let mut transformed = JsonObject::new();

        if let Some(kind) = thinking.get("type") {
            transformed.insert("type".to_string(), kind.clone());
        }

        if let Some(budget_tokens) = thinking.get("budgetTokens") {
            transformed.insert("budget_tokens".to_string(), budget_tokens.clone());
        }

        body.insert("thinking".to_string(), JsonValue::Object(transformed));
    }

    if let Some(reasoning_history) = body.remove("reasoningHistory") {
        body.insert("reasoning_history".to_string(), reasoning_history);
    }

    if let Some(JsonValue::String(reasoning_effort)) = body.get_mut("reasoning_effort") {
        if reasoning_effort == "minimal" {
            *reasoning_effort = "low".to_string();
        } else if reasoning_effort == "xhigh" {
            *reasoning_effort = "high".to_string();
        }
    }

    JsonValue::Object(body)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FireworksImageBackendFormat {
    Workflows,
    WorkflowsAsync,
    ImageGeneration,
}

#[derive(Clone, Debug, Deserialize)]
struct FireworksAsyncSubmitResponse {
    #[serde(rename = "request_id")]
    request_id: String,
}

#[derive(Clone, Debug, Deserialize)]
struct FireworksAsyncPollResponse {
    #[serde(rename = "id")]
    _id: String,
    status: String,
    #[serde(default)]
    result: Option<FireworksAsyncPollResult>,
}

#[derive(Clone, Debug, Deserialize)]
struct FireworksAsyncPollResult {
    #[serde(default)]
    sample: Option<String>,
}

fn fireworks_image_headers(
    settings: &FireworksProviderSettings,
    call_headers: Option<&Headers>,
) -> BTreeMap<String, Option<String>> {
    combine_headers([
        Some(fireworks_provider_header_entries(settings)),
        optional_headers(call_headers),
    ])
}

fn fireworks_provider_header_entries(
    settings: &FireworksProviderSettings,
) -> Vec<(String, Option<String>)> {
    fireworks_provider_headers(settings)
        .into_iter()
        .map(|(name, value)| (name, Some(value)))
        .collect()
}

fn fireworks_provider_headers(settings: &FireworksProviderSettings) -> Headers {
    let mut headers = Headers::new();

    if let Some(api_key) = fireworks_api_key(settings.api_key.as_ref()) {
        headers.insert("authorization".to_string(), format!("Bearer {api_key}"));
    }

    for (name, value) in &settings.headers {
        headers.insert(name.clone(), value.clone());
    }

    with_user_agent_suffix(
        Some(
            headers
                .into_iter()
                .map(|(name, value)| (name, Some(value)))
                .collect::<Vec<_>>(),
        ),
        [format!("ai-sdk/fireworks/{}", crate::VERSION)],
    )
}

fn optional_headers(headers: Option<&Headers>) -> Option<Vec<(String, Option<String>)>> {
    headers.map(|headers| {
        headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone())))
            .collect()
    })
}

fn fireworks_image_warnings(model_id: &str, options: &ImageModelCallOptions) -> Vec<Warning> {
    let mut warnings = Vec::new();
    let supports_size = fireworks_image_supports_size(model_id);

    if !supports_size && options.size.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "size".to_string(),
            details: Some(
                "This model does not support the `size` option. Use `aspectRatio` instead."
                    .to_string(),
            ),
        });
    }

    if supports_size && options.aspect_ratio.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "aspectRatio".to_string(),
            details: Some("This model does not support the `aspectRatio` option.".to_string()),
        });
    }

    if options.files.as_ref().is_some_and(|files| files.len() > 1) {
        warnings.push(Warning::Other {
            message: "Fireworks only supports a single input image. Additional images are ignored."
                .to_string(),
        });
    }

    if options.mask.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "mask".to_string(),
            details: Some(
                "Fireworks Kontext models do not support explicit masks. Use the prompt to describe the areas to edit."
                    .to_string(),
            ),
        });
    }

    warnings
}

fn fireworks_image_request_body(_model_id: &str, options: &ImageModelCallOptions) -> JsonValue {
    let mut body = JsonObject::new();

    if let Some(prompt) = &options.prompt {
        body.insert("prompt".to_string(), JsonValue::String(prompt.clone()));
    }

    if let Some(aspect_ratio) = &options.aspect_ratio {
        body.insert(
            "aspect_ratio".to_string(),
            JsonValue::String(aspect_ratio.clone()),
        );
    }

    if let Some(seed) = options.seed {
        body.insert("seed".to_string(), JsonValue::from(seed));
    }

    body.insert("samples".to_string(), JsonValue::from(options.n));

    if let Some(file) = options.files.as_ref().and_then(|files| files.first()) {
        body.insert(
            "input_image".to_string(),
            JsonValue::String(convert_image_model_file_to_data_uri(file)),
        );
    }

    if let Some(size) = options.size.as_deref() {
        let mut parts = size.split('x');
        if let (Some(width), Some(height), None) = (parts.next(), parts.next(), parts.next()) {
            body.insert("width".to_string(), JsonValue::String(width.to_string()));
            body.insert("height".to_string(), JsonValue::String(height.to_string()));
        }
    }

    if let Some(provider_options) = options.provider_options.get("fireworks") {
        for (key, value) in provider_options {
            body.insert(key.clone(), value.clone());
        }
    }

    JsonValue::Object(body)
}

fn fireworks_image_url(base_url: &str, model_id: &str) -> String {
    match fireworks_image_backend_format(model_id) {
        FireworksImageBackendFormat::ImageGeneration => {
            format!("{base_url}/image_generation/{model_id}")
        }
        FireworksImageBackendFormat::WorkflowsAsync => format!("{base_url}/workflows/{model_id}"),
        FireworksImageBackendFormat::Workflows => {
            format!("{base_url}/workflows/{model_id}/text_to_image")
        }
    }
}

fn fireworks_image_poll_url(base_url: &str, model_id: &str) -> String {
    format!("{base_url}/workflows/{model_id}/get_result")
}

fn fireworks_image_is_async_model(model_id: &str) -> bool {
    fireworks_image_backend_format(model_id) == FireworksImageBackendFormat::WorkflowsAsync
}

fn fireworks_image_supports_size(model_id: &str) -> bool {
    matches!(
        model_id,
        "accounts/fireworks/models/playground-v2-5-1024px-aesthetic"
            | "accounts/fireworks/models/japanese-stable-diffusion-xl"
            | "accounts/fireworks/models/playground-v2-1024px-aesthetic"
            | "accounts/fireworks/models/stable-diffusion-xl-1024-v1-0"
            | "accounts/fireworks/models/SSD-1B"
    )
}

fn fireworks_image_backend_format(model_id: &str) -> FireworksImageBackendFormat {
    match model_id {
        "accounts/fireworks/models/playground-v2-5-1024px-aesthetic"
        | "accounts/fireworks/models/japanese-stable-diffusion-xl"
        | "accounts/fireworks/models/playground-v2-1024px-aesthetic"
        | "accounts/fireworks/models/stable-diffusion-xl-1024-v1-0"
        | "accounts/fireworks/models/SSD-1B" => FireworksImageBackendFormat::ImageGeneration,
        "accounts/fireworks/models/flux-kontext-pro"
        | "accounts/fireworks/models/flux-kontext-max" => {
            FireworksImageBackendFormat::WorkflowsAsync
        }
        _ => FireworksImageBackendFormat::Workflows,
    }
}

fn fireworks_async_submit_response(
    value: &JsonValue,
) -> Result<FireworksAsyncSubmitResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn fireworks_async_poll_response(
    value: &JsonValue,
) -> Result<FireworksAsyncPollResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn fireworks_image_result_from_bytes(
    model_id: &str,
    timestamp: OffsetDateTime,
    image: Vec<u8>,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
) -> ImageModelResult {
    let mut result = ImageModelResult::new(
        vec![FileDataContent::Bytes(image)],
        fireworks_image_response_metadata(model_id, headers, timestamp),
    );

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn fireworks_image_result_from_error(
    model_id: &str,
    timestamp: OffsetDateTime,
    error: HandledFetchError,
    warnings: Vec<Warning>,
) -> ImageModelResult {
    let (message, headers) = match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
        ),
    };

    fireworks_image_result_from_error_message_with_headers(
        model_id, timestamp, message, headers, warnings,
    )
}

fn fireworks_image_result_from_error_message(
    model_id: &str,
    timestamp: OffsetDateTime,
    message: impl Into<String>,
    warnings: Vec<Warning>,
) -> ImageModelResult {
    fireworks_image_result_from_error_message_with_headers(
        model_id,
        timestamp,
        message.into(),
        None,
        warnings,
    )
}

fn fireworks_image_result_from_error_message_with_headers(
    model_id: &str,
    timestamp: OffsetDateTime,
    message: String,
    headers: Option<Headers>,
    warnings: Vec<Warning>,
) -> ImageModelResult {
    let mut result = ImageModelResult::new(
        Vec::new(),
        fireworks_image_response_metadata(model_id, headers, timestamp),
    )
    .with_provider_metadata(fireworks_image_error_metadata(message));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn fireworks_image_response_metadata(
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

fn fireworks_image_error_metadata(message: String) -> ImageModelProviderMetadata {
    let mut extra = JsonObject::new();
    extra.insert("errorMessage".to_string(), JsonValue::String(message));

    ImageModelProviderMetadata::from([(
        "fireworks".to_string(),
        ImageModelProviderMetadataEntry {
            images: Vec::new(),
            extra,
        },
    )])
}

fn json_value_object<I, K>(entries: I) -> JsonValue
where
    I: IntoIterator<Item = (K, JsonValue)>,
    K: Into<String>,
{
    let mut object = JsonObject::new();

    for (key, value) in entries {
        object.insert(key.into(), value);
    }

    JsonValue::Object(object)
}

fn default_fireworks_transport() -> OpenAICompatibleTransport {
    Arc::new(|request| Box::pin(ready(execute_fireworks_request(request))))
}

fn execute_fireworks_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => Err(FetchErrorInfo::new(
            "Fireworks image downloads require an injected transport",
        )),
        ProviderApiRequestMethod::Post => match request.body {
            Some(ProviderApiRequestBody::Text { .. }) => Err(FetchErrorInfo::new(
                "Fireworks image requests require an injected transport",
            )),
            Some(ProviderApiRequestBody::FormData { .. }) => Err(FetchErrorInfo::new(
                "multipart form data requires an injected Fireworks transport",
            )),
            Some(ProviderApiRequestBody::Bytes { .. }) | None => Err(FetchErrorInfo::new(
                "Fireworks provider requests require an injected transport",
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_FIREWORKS_BASE_URL, FireworksProvider, FireworksProviderSettings, create_fireworks,
        fireworks,
    };
    use crate::embed::{EmbedManyOptions, embed_many};
    use crate::file_data::FileDataContent;
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::image_model::{ImageModel, ImageModelCallOptions, ImageModelFile};
    use crate::json::JsonValue;
    use crate::language_model::ProviderAbortController;
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::prompt::Prompt;
    use crate::provider::{Provider, ProviderOptions};
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    };
    use serde_json::json;
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

        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures use ready transports"),
        }
    }

    fn request_body_json(request: &ProviderApiRequest) -> JsonValue {
        request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON text")
    }

    #[test]
    fn fireworks_provider_creates_chat_model_with_transformed_provider_options() {
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
                        "id": "chatcmpl-fireworks",
                        "created": 1711115037,
                        "model": "accounts/fireworks/models/llama-v3p1-8b-instruct",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from Fireworks"
                                },
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "completion_tokens": 4,
                            "total_tokens": 8
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_fireworks".to_string(),
                )])))))
            });
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "fireworks": {
                "thinking": {
                    "type": "enabled",
                    "budgetTokens": 2048
                },
                "reasoningHistory": "interleaved",
                "reasoning_effort": "xhigh"
            }
        }))
        .expect("provider options deserialize");
        let provider = create_fireworks(
            FireworksProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.fireworks.test/inference/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.chat_model("accounts/fireworks/models/llama-v3p1-8b-instruct");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_provider_options(provider_options),
        ));

        assert_eq!(model.provider(), "fireworks.chat");
        assert_eq!(result.text, "Hello from Fireworks");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.fireworks.test/inference/v1/chat/completions"
        );
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/fireworks/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "accounts/fireworks/models/llama-v3p1-8b-instruct",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ],
                "thinking": {
                    "type": "enabled",
                    "budget_tokens": 2048
                },
                "reasoning_history": "interleaved",
                "reasoning_effort": "high"
            }))
        );
    }

    #[test]
    fn fireworks_provider_creates_completion_embedding_and_image_models() {
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
                        "model": "nomic-ai/nomic-embed-text-v1.5",
                        "data": [
                            {
                                "index": 0,
                                "embedding": [0.1, 0.2]
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 2,
                            "total_tokens": 2
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = FireworksProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.fireworks.test/inference/v1/")
            .with_transport(transport);
        let completion = provider.completion_model("accounts/fireworks/models/completion");
        let embedding = provider.embedding_model("nomic-ai/nomic-embed-text-v1.5");
        let text_embedding = provider.text_embedding_model("nomic-ai/nomic-embed-text-v1.5");
        let image = provider.image_model("accounts/fireworks/models/flux-1-dev-fp8");
        let result = poll_ready(embed_many(EmbedManyOptions::new(&embedding, ["hello"])));

        assert_eq!(completion.provider(), "fireworks.completion");
        assert_eq!(embedding.provider(), "fireworks.embedding");
        assert_eq!(text_embedding.provider(), "fireworks.embedding");
        assert_eq!(image.provider(), "fireworks.image");
        assert_eq!(result.embeddings, vec![vec![0.1, 0.2]]);

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request.url,
            "https://api.fireworks.test/inference/v1/embeddings"
        );
    }

    #[test]
    fn fireworks_image_model_sends_workflow_request_and_returns_binary() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::bytes(
                    200,
                    "OK",
                    b"fireworks-image".to_vec(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "img-sync".to_string(),
                )])))))
            });
        let provider = FireworksProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.fireworks.test/inference/v1/")
            .with_header("x-provider-header", "provider")
            .with_transport(transport);
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "fireworks": {
                "additional_param": "value"
            }
        }))
        .expect("provider options deserialize");
        let model = provider.image_model("accounts/fireworks/models/flux-1-dev-fp8");
        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(2)
                    .with_prompt("A small ceramic vase")
                    .with_aspect_ratio("16:9")
                    .with_seed(42)
                    .with_provider_options(provider_options)
                    .with_header("x-request-header", "request"),
            ),
        );

        assert_eq!(
            result.images,
            vec![FileDataContent::Bytes(b"fireworks-image".to_vec())]
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("img-sync")
        );
        assert!(result.warnings.is_empty());

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.fireworks.test/inference/v1/workflows/accounts/fireworks/models/flux-1-dev-fp8/text_to_image"
        );
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("x-provider-header").map(String::as_str),
            Some("provider")
        );
        assert_eq!(
            request.headers.get("x-request-header").map(String::as_str),
            Some("request")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/fireworks/0.1.0"))
        );
        assert_eq!(
            request_body_json(&request),
            json!({
                "prompt": "A small ceramic vase",
                "aspect_ratio": "16:9",
                "seed": 42,
                "samples": 2,
                "additional_param": "value"
            })
        );
    }

    #[test]
    fn fireworks_image_model_sends_async_edit_input_image_and_warnings() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                captured_requests_for_transport
                    .lock()
                    .expect("captured request list mutex is not poisoned")
                    .push(request.clone());

                let response = match (request.method, request.url.as_str()) {
                    (ProviderApiRequestMethod::Post, url) if url.ends_with("/flux-kontext-pro") => {
                        ProviderApiResponse::text(
                            200,
                            "OK",
                            json!({ "request_id": "edit-request-123" }).to_string(),
                        )
                    }
                    (ProviderApiRequestMethod::Post, url)
                        if url.ends_with("/flux-kontext-pro/get_result") =>
                    {
                        ProviderApiResponse::text(
                            200,
                            "OK",
                            json!({
                                "id": "edit-request-123",
                                "status": "Ready",
                                "result": {
                                    "sample": "https://example.com/edited.png"
                                }
                            })
                            .to_string(),
                        )
                    }
                    (ProviderApiRequestMethod::Get, "https://example.com/edited.png") => {
                        ProviderApiResponse::bytes(200, "OK", b"edited-image".to_vec())
                    }
                    _ => ProviderApiResponse::text(500, "Unexpected Request", "{}"),
                };

                Box::pin(ready(Ok(response)))
            });
        let model = FireworksProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.fireworks.test/inference/v1/")
            .with_transport(transport)
            .image_model("accounts/fireworks/models/flux-kontext-pro");
        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("Replace the skyline")
                    .with_files(vec![
                        ImageModelFile::file(
                            "image/png",
                            FileDataContent::Bytes(vec![137, 80, 78, 71]),
                        ),
                        ImageModelFile::url(
                            Url::parse("https://example.com/extra.png").expect("valid URL"),
                        ),
                    ])
                    .with_mask(ImageModelFile::url(
                        Url::parse("https://example.com/mask.png").expect("valid URL"),
                    )),
            ),
        );

        assert_eq!(
            result.images,
            vec![FileDataContent::Bytes(b"edited-image".to_vec())]
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| matches!(warning, crate::warning::Warning::Other { .. }))
        );
        assert!(result.warnings.iter().any(|warning| {
            matches!(warning, crate::warning::Warning::Unsupported { feature, .. } if feature == "mask")
        }));

        let requests = captured_requests
            .lock()
            .expect("captured request list mutex is not poisoned");
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests[0].url,
            "https://api.fireworks.test/inference/v1/workflows/accounts/fireworks/models/flux-kontext-pro"
        );
        assert_eq!(
            request_body_json(&requests[0]),
            json!({
                "prompt": "Replace the skyline",
                "samples": 1,
                "input_image": "data:image/png;base64,iVBORw=="
            })
        );
        assert_eq!(
            requests[1].url,
            "https://api.fireworks.test/inference/v1/workflows/accounts/fireworks/models/flux-kontext-pro/get_result"
        );
        assert_eq!(
            request_body_json(&requests[1]),
            json!({ "id": "edit-request-123" })
        );
        assert_eq!(requests[2].method, ProviderApiRequestMethod::Get);
        assert_eq!(requests[2].url, "https://example.com/edited.png");
    }

    #[test]
    fn fireworks_image_model_maps_image_generation_size_and_aspect_warning() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::bytes(
                    200,
                    "OK",
                    vec![9, 8, 7],
                ))))
            });
        let model = FireworksProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.fireworks.test/inference/v1/")
            .with_transport(transport)
            .image_model("accounts/fireworks/models/playground-v2-5-1024px-aesthetic");
        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A geometric poster")
                    .with_size("1024x768")
                    .with_aspect_ratio("1:1"),
            ),
        );

        assert_eq!(result.images, vec![FileDataContent::Bytes(vec![9, 8, 7])]);
        assert!(result.warnings.iter().any(|warning| {
            matches!(warning, crate::warning::Warning::Unsupported { feature, .. } if feature == "aspectRatio")
        }));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request.url,
            "https://api.fireworks.test/inference/v1/image_generation/accounts/fireworks/models/playground-v2-5-1024px-aesthetic"
        );
        assert_eq!(
            request_body_json(&request),
            json!({
                "prompt": "A geometric poster",
                "aspect_ratio": "1:1",
                "samples": 1,
                "width": "1024",
                "height": "768"
            })
        );
    }

    #[test]
    fn fireworks_image_model_aborts_before_request() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::bytes(200, "OK", vec![1]))))
            });
        let abort_controller = ProviderAbortController::new();
        abort_controller.abort_with_reason("client disconnected");
        let model = FireworksProvider::new()
            .with_transport(transport)
            .image_model("accounts/fireworks/models/flux-1-dev-fp8");
        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("aborted")
                    .with_abort_signal(abort_controller.signal()),
            ),
        );

        assert!(result.images.is_empty());
        assert!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("fireworks"))
                .and_then(|metadata| metadata.extra.get("errorMessage"))
                .is_some()
        );
        assert!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .is_none()
        );
    }

    #[test]
    fn fireworks_provider_uses_default_base_url_and_function_alias() {
        let model = fireworks("accounts/fireworks/models/llama-v3p1-8b-instruct");

        assert_eq!(model.provider(), "fireworks.chat");
        assert_eq!(
            model.model_id(),
            "accounts/fireworks/models/llama-v3p1-8b-instruct"
        );
        assert_eq!(
            DEFAULT_FIREWORKS_BASE_URL,
            "https://api.fireworks.ai/inference/v1"
        );
    }

    #[test]
    fn fireworks_provider_implements_provider_trait() {
        let provider = FireworksProvider::new();
        let model =
            Provider::language_model(&provider, "accounts/fireworks/models/chat").expect("model");
        let embedding = Provider::embedding_model(&provider, "embed").expect("embedding");
        let image = Provider::image_model(&provider, "image").expect("image");

        assert_eq!(model.provider(), "fireworks.chat");
        assert_eq!(embedding.provider(), "fireworks.embedding");
        assert_eq!(image.provider(), "fireworks.image");
    }

    #[test]
    fn fireworks_provider_settings_serde_accepts_upstream_base_url() {
        let settings: FireworksProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.fireworks.test/inference/v1/",
            "apiKey": "key",
            "headers": {
                "x-provider": "fireworks"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            FireworksProviderSettings::new()
                .with_base_url("https://api.fireworks.test/inference/v1/")
                .with_api_key("key")
                .with_header("x-provider", "fireworks")
        );
    }
}
