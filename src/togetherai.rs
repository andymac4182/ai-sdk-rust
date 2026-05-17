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
use crate::provider::{NoSuchModelError, Provider, ProviderMetadata, ProviderWithRerankingModel};
use crate::provider_utils::{
    FetchErrorInfo, HandledFetchError, PostJsonToApiOptions, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, RuntimeEnvironment, combine_headers,
    convert_image_model_file_to_data_uri, create_json_error_response_handler,
    create_json_response_handler, post_json_to_api, with_user_agent_suffix, without_trailing_slash,
};
use crate::reranking_model::{
    RerankingModel, RerankingModelCallOptions, RerankingModelDocuments, RerankingModelRanking,
    RerankingModelResponse, RerankingModelResult,
};
use crate::warning::Warning;

/// Default base URL for upstream `@ai-sdk/togetherai` API calls.
pub const DEFAULT_TOGETHERAI_BASE_URL: &str = "https://api.together.xyz/v1";

/// Settings for the upstream TogetherAI provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TogetherAIProviderSettings {
    /// Base URL for TogetherAI API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// TogetherAI API key. When omitted, `TOGETHER_API_KEY` and then
    /// deprecated `TOGETHER_AI_API_KEY` are read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl TogetherAIProviderSettings {
    /// Creates empty TogetherAI provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the TogetherAI API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the TogetherAI API key.
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

/// Upstream TogetherAI provider foundation.
#[derive(Clone)]
pub struct TogetherAIProvider {
    settings: TogetherAIProviderSettings,
    transport: Option<OpenAICompatibleTransport>,
}

/// TogetherAI image model for `/images/generations` calls.
#[derive(Clone)]
pub struct TogetherAIImageModel {
    model_id: String,
    base_url: String,
    settings: TogetherAIProviderSettings,
    transport: OpenAICompatibleTransport,
}

/// TogetherAI reranking model for `/rerank` calls.
#[derive(Clone)]
pub struct TogetherAIRerankingModel {
    model_id: String,
    base_url: String,
    settings: TogetherAIProviderSettings,
    transport: OpenAICompatibleTransport,
}

impl TogetherAIImageModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: TogetherAIProviderSettings,
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
        "togetherai.image"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: OpenAICompatibleTransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_generate_result(&self, options: ImageModelCallOptions) -> ImageModelResult {
        if options.mask.is_some() {
            return togetherai_image_unsupported_mask_result(&self.model_id);
        }

        let warnings = togetherai_image_warnings(&options);
        let request_body = togetherai_image_request_body(&self.model_id, &options);
        let request_headers = self.request_headers(options.headers.as_ref());
        let post_options = PostJsonToApiOptions::new(self.image_model_url(), request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    togetherai_image_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    togetherai_error_response,
                    togetherai_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => togetherai_image_result_from_response(
                &self.model_id,
                response.value,
                response.response_headers,
                warnings,
            ),
            Err(error) => togetherai_image_result_from_error(&self.model_id, error, warnings),
        }
    }

    fn image_model_url(&self) -> String {
        format!("{}/images/generations", self.base_url)
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(togetherai_provider_header_entries(&self.settings)),
            optional_headers(call_headers),
        ])
    }
}

impl ImageModel for TogetherAIImageModel {
    type MaxImagesPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        TogetherAIImageModel::provider(self)
    }

    fn model_id(&self) -> &str {
        TogetherAIImageModel::model_id(self)
    }

    fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
        ready(Some(1))
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

impl TogetherAIRerankingModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: TogetherAIProviderSettings,
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
        "togetherai.reranking"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: OpenAICompatibleTransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_rerank_result(&self, options: RerankingModelCallOptions) -> RerankingModelResult {
        let request_body = togetherai_reranking_request_body(&self.model_id, &options);
        let request_body_for_error = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let post_options = PostJsonToApiOptions::new(self.reranking_model_url(), request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    togetherai_reranking_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    togetherai_error_response,
                    togetherai_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => togetherai_reranking_result_from_response(
                response.value,
                response.raw_value,
                response.response_headers,
            ),
            Err(error) => {
                togetherai_reranking_result_from_error(error, Some(request_body_for_error))
            }
        }
    }

    fn reranking_model_url(&self) -> String {
        format!("{}/rerank", self.base_url)
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(togetherai_provider_header_entries(&self.settings)),
            optional_headers(call_headers),
        ])
    }
}

impl RerankingModel for TogetherAIRerankingModel {
    type RerankFuture<'a>
        = Pin<Box<dyn Future<Output = RerankingModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        TogetherAIRerankingModel::provider(self)
    }

    fn model_id(&self) -> &str {
        TogetherAIRerankingModel::model_id(self)
    }

    fn do_rerank(&self, options: RerankingModelCallOptions) -> Self::RerankFuture<'_> {
        Box::pin(self.do_rerank_result(options))
    }
}

impl TogetherAIProvider {
    /// Creates a TogetherAI provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(TogetherAIProviderSettings::new())
    }

    /// Creates a provider from explicit TogetherAI settings.
    pub fn from_settings(settings: TogetherAIProviderSettings) -> Self {
        Self {
            settings,
            transport: None,
        }
    }

    /// Sets the TogetherAI API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the TogetherAI API base URL for this provider.
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

    /// Creates a TogetherAI chat language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Creates a TogetherAI chat language model.
    pub fn chat_model(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.openai_compatible_provider().chat_model(model_id)
    }

    /// Alias for [`TogetherAIProvider::chat_model`].
    pub fn chat(&self, model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
        self.chat_model(model_id)
    }

    /// Creates a TogetherAI completion language model.
    pub fn completion_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleCompletionLanguageModel {
        self.openai_compatible_provider().completion_model(model_id)
    }

    /// Creates a TogetherAI embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.openai_compatible_provider().embedding_model(model_id)
    }

    /// Alias for [`TogetherAIProvider::embedding_model`].
    pub fn embedding(&self, model_id: impl Into<String>) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Deprecated upstream alias for [`TogetherAIProvider::embedding_model`].
    pub fn text_embedding_model(
        &self,
        model_id: impl Into<String>,
    ) -> OpenAICompatibleEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Creates a TogetherAI image model.
    pub fn image_model(&self, model_id: impl Into<String>) -> TogetherAIImageModel {
        TogetherAIImageModel::new(
            model_id,
            togetherai_base_url(&self.settings),
            self.settings.clone(),
            self.transport
                .as_ref()
                .map(Arc::clone)
                .unwrap_or_else(default_togetherai_transport),
        )
    }

    /// Alias for [`TogetherAIProvider::image_model`].
    pub fn image(&self, model_id: impl Into<String>) -> TogetherAIImageModel {
        self.image_model(model_id)
    }

    /// Creates a TogetherAI reranking model.
    pub fn reranking_model(&self, model_id: impl Into<String>) -> TogetherAIRerankingModel {
        TogetherAIRerankingModel::new(
            model_id,
            togetherai_base_url(&self.settings),
            self.settings.clone(),
            self.transport
                .as_ref()
                .map(Arc::clone)
                .unwrap_or_else(default_togetherai_transport),
        )
    }

    /// Alias for [`TogetherAIProvider::reranking_model`].
    pub fn reranking(&self, model_id: impl Into<String>) -> TogetherAIRerankingModel {
        self.reranking_model(model_id)
    }

    fn openai_compatible_provider(&self) -> OpenAICompatibleProvider {
        let mut settings = OpenAICompatibleProviderSettings::new(
            "togetherai",
            togetherai_base_url(&self.settings),
        )
        .with_user_agent_suffix(format!("ai-sdk/togetherai/{}", crate::VERSION));

        if let Some(api_key) = togetherai_api_key(self.settings.api_key.as_ref()) {
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

impl Default for TogetherAIProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for TogetherAIProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = OpenAICompatibleEmbeddingModel;
    type ImageModel = TogetherAIImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(TogetherAIProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(TogetherAIProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        Ok(TogetherAIProvider::image_model(self, model_id))
    }
}

impl ProviderWithRerankingModel for TogetherAIProvider {
    type RerankingModel = TogetherAIRerankingModel;

    fn reranking_model(&self, model_id: &str) -> Result<Self::RerankingModel, NoSuchModelError> {
        Ok(TogetherAIProvider::reranking_model(self, model_id))
    }
}

/// Creates a TogetherAI provider with explicit settings.
pub fn create_togetherai(settings: TogetherAIProviderSettings) -> TogetherAIProvider {
    TogetherAIProvider::from_settings(settings)
}

/// Creates a TogetherAI chat language model using the default provider settings.
pub fn togetherai(model_id: impl Into<String>) -> OpenAICompatibleChatLanguageModel {
    TogetherAIProvider::new().language_model(model_id)
}

fn togetherai_base_url(settings: &TogetherAIProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_TOGETHERAI_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn togetherai_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    togetherai_api_key_from(explicit_api_key, |name| env::var(name).ok())
}

fn togetherai_api_key_from(
    explicit_api_key: Option<&String>,
    load_env: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(load_env("TOGETHER_API_KEY")))
        .or_else(|| non_empty_optional_setting(load_env("TOGETHER_AI_API_KEY")))
}

fn togetherai_provider_headers(settings: &TogetherAIProviderSettings) -> Headers {
    let mut headers = Headers::new();

    if let Some(api_key) = togetherai_api_key(settings.api_key.as_ref()) {
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
        [format!("ai-sdk/togetherai/{}", crate::VERSION)],
    )
}

fn togetherai_provider_header_entries(
    settings: &TogetherAIProviderSettings,
) -> Vec<(String, Option<String>)> {
    togetherai_provider_headers(settings)
        .into_iter()
        .map(|(name, value)| (name, Some(value)))
        .collect()
}

fn optional_headers(headers: Option<&Headers>) -> Option<Vec<(String, Option<String>)>> {
    headers.map(|headers| {
        headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone())))
            .collect()
    })
}

fn togetherai_image_request_body(model_id: &str, options: &ImageModelCallOptions) -> JsonValue {
    let mut body = JsonObject::new();
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));

    if let Some(prompt) = &options.prompt {
        body.insert("prompt".to_string(), JsonValue::String(prompt.clone()));
    }

    if let Some(seed) = options.seed {
        body.insert("seed".to_string(), JsonValue::from(seed));
    }

    if options.n > 1 {
        body.insert("n".to_string(), JsonValue::from(options.n));
    }

    if let Some(size) = &options.size
        && let Some((width, height)) = size.split_once('x')
    {
        if let Ok(width) = width.parse::<u64>() {
            body.insert("width".to_string(), JsonValue::from(width));
        }

        if let Ok(height) = height.parse::<u64>() {
            body.insert("height".to_string(), JsonValue::from(height));
        }
    }

    if let Some(first_file) = options.files.as_ref().and_then(|files| files.first()) {
        body.insert(
            "image_url".to_string(),
            JsonValue::String(convert_image_model_file_to_data_uri(first_file)),
        );
    }

    body.insert(
        "response_format".to_string(),
        JsonValue::String("base64".to_string()),
    );

    if let Some(provider_options) = options.provider_options.get("togetherai") {
        for (name, value) in provider_options {
            body.insert(name.clone(), value.clone());
        }
    }

    JsonValue::Object(body)
}

fn togetherai_image_warnings(options: &ImageModelCallOptions) -> Vec<Warning> {
    let mut warnings = Vec::new();

    if options.aspect_ratio.is_some() {
        warnings.push(Warning::Unsupported {
            feature: "aspectRatio".to_string(),
            details: Some(
                "This model does not support the `aspectRatio` option. Use `size` instead."
                    .to_string(),
            ),
        });
    }

    if options.files.as_ref().is_some_and(|files| files.len() > 1) {
        warnings.push(Warning::Other {
            message:
                "Together AI only supports a single input image. Additional images are ignored."
                    .to_string(),
        });
    }

    warnings
}

fn togetherai_reranking_request_body(
    model_id: &str,
    options: &RerankingModelCallOptions,
) -> JsonValue {
    let mut body = JsonObject::new();
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    body.insert(
        "documents".to_string(),
        togetherai_reranking_documents(&options.documents),
    );
    body.insert(
        "query".to_string(),
        JsonValue::String(options.query.clone()),
    );

    if let Some(top_n) = options.top_n {
        body.insert("top_n".to_string(), JsonValue::from(top_n));
    }

    if let Some(rank_fields) = options
        .provider_options
        .as_ref()
        .and_then(|options| options.get("togetherai"))
        .and_then(|options| {
            options
                .get("rankFields")
                .or_else(|| options.get("rank_fields"))
                .cloned()
        })
    {
        body.insert("rank_fields".to_string(), rank_fields);
    }

    body.insert("return_documents".to_string(), JsonValue::Bool(false));
    JsonValue::Object(body)
}

fn togetherai_reranking_documents(documents: &RerankingModelDocuments) -> JsonValue {
    match documents {
        RerankingModelDocuments::Text { values } => serde_json::to_value(values),
        RerankingModelDocuments::Object { values } => serde_json::to_value(values),
    }
    .unwrap_or(JsonValue::Null)
}

#[derive(Clone, Debug, Deserialize)]
struct TogetherAIImageResponse {
    data: Vec<TogetherAIImageResponseData>,
}

#[derive(Clone, Debug, Deserialize)]
struct TogetherAIImageResponseData {
    b64_json: String,
}

#[derive(Clone, Debug, Deserialize)]
struct TogetherAIRerankingResponse {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    results: Vec<TogetherAIRerankingResult>,
}

#[derive(Clone, Debug, Deserialize)]
struct TogetherAIRerankingResult {
    index: usize,
    relevance_score: f64,
}

fn togetherai_image_response(
    value: &JsonValue,
) -> Result<TogetherAIImageResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn togetherai_reranking_response(
    value: &JsonValue,
) -> Result<TogetherAIRerankingResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn togetherai_error_response(value: &JsonValue) -> Result<JsonValue, serde_json::Error> {
    Ok(value.clone())
}

fn togetherai_error_message(value: &JsonValue) -> String {
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(JsonValue::as_str)
        .unwrap_or("Unknown error")
        .to_string()
}

fn togetherai_image_result_from_response(
    model_id: &str,
    response: TogetherAIImageResponse,
    response_headers: Option<Headers>,
    warnings: Vec<Warning>,
) -> ImageModelResult {
    let mut result = ImageModelResult::new(
        response
            .data
            .into_iter()
            .map(|image| FileDataContent::Base64(image.b64_json))
            .collect(),
        togetherai_image_response_metadata(model_id, response_headers),
    );

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn togetherai_image_result_from_error(
    model_id: &str,
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
    let mut result = ImageModelResult::new(
        Vec::new(),
        togetherai_image_response_metadata(model_id, headers),
    )
    .with_provider_metadata(togetherai_image_error_metadata(message));

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
}

fn togetherai_image_unsupported_mask_result(model_id: &str) -> ImageModelResult {
    let message = togetherai_unsupported_mask_message();

    ImageModelResult::new(
        Vec::new(),
        togetherai_image_response_metadata(model_id, None),
    )
    .with_warning(Warning::Unsupported {
        feature: "mask".to_string(),
        details: Some(message.clone()),
    })
    .with_provider_metadata(togetherai_image_error_metadata(message))
}

fn togetherai_unsupported_mask_message() -> String {
    "Together AI does not support mask-based image editing. Use FLUX Kontext models (e.g., black-forest-labs/FLUX.1-kontext-pro) with a reference image and descriptive prompt instead.".to_string()
}

fn togetherai_image_response_metadata(
    model_id: &str,
    headers: Option<Headers>,
) -> ImageModelResponse {
    let mut response = ImageModelResponse::new(OffsetDateTime::now_utc(), model_id);

    if let Some(headers) = headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    response
}

fn togetherai_image_error_metadata(message: String) -> ImageModelProviderMetadata {
    let mut extra = JsonObject::new();
    extra.insert("errorMessage".to_string(), JsonValue::String(message));

    ImageModelProviderMetadata::from([(
        "togetherai".to_string(),
        ImageModelProviderMetadataEntry {
            images: Vec::new(),
            extra,
        },
    )])
}

fn togetherai_reranking_result_from_response(
    response: TogetherAIRerankingResponse,
    raw_response: Option<JsonValue>,
    response_headers: Option<Headers>,
) -> RerankingModelResult {
    let ranking = response
        .results
        .into_iter()
        .map(|result| RerankingModelRanking::new(result.index, result.relevance_score))
        .collect();
    let mut result = RerankingModelResult::new(ranking);
    let mut response_metadata = RerankingModelResponse::new();

    if let Some(id) = response.id {
        response_metadata = response_metadata.with_id(id);
    }

    if let Some(model) = response.model {
        response_metadata = response_metadata.with_model_id(model);
    }

    if let Some(body) = raw_response {
        response_metadata = response_metadata.with_body(body);
    }

    if let Some(headers) = response_headers {
        response_metadata = with_reranking_response_headers(response_metadata, headers);
    }

    result = result.with_response(response_metadata);
    result
}

fn togetherai_reranking_result_from_error(
    error: HandledFetchError,
    request_body: Option<JsonValue>,
) -> RerankingModelResult {
    let (message, headers, body) = match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None, None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
            error.response_body().map(String::from),
        ),
    };
    let response_body = body
        .as_deref()
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
        .or_else(|| body.map(JsonValue::String))
        .or(request_body);
    let mut response = RerankingModelResponse::new();

    if let Some(body) = response_body {
        response = response.with_body(body);
    }

    if let Some(headers) = headers {
        response = with_reranking_response_headers(response, headers);
    }

    RerankingModelResult::new(Vec::new())
        .with_provider_metadata(togetherai_error_metadata(message))
        .with_response(response)
}

fn with_reranking_response_headers(
    mut response: RerankingModelResponse,
    headers: Headers,
) -> RerankingModelResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }

    response
}

fn togetherai_error_metadata(message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut extra = JsonObject::new();
    extra.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert("togetherai".to_string(), extra);
    metadata
}

fn default_togetherai_transport() -> OpenAICompatibleTransport {
    Arc::new(|request| Box::pin(ready(execute_togetherai_request(request))))
}

fn execute_togetherai_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_togetherai_get_request(request),
        ProviderApiRequestMethod::Post => execute_togetherai_post_request(request),
    }
}

fn execute_togetherai_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    togetherai_provider_api_response(response)
}

fn execute_togetherai_post_request(
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
                "multipart form data is not supported by the TogetherAI transport",
            ));
        }
        None => builder.send_empty(),
    };

    togetherai_provider_api_response(response)
}

fn togetherai_provider_api_response(
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
    let body = response.body_mut().read_to_string().map_err(|error| {
        FetchErrorInfo::new("failed to read response body")
            .with_name("Error")
            .with_cause_message(error.to_string())
    })?;

    Ok(ProviderApiResponse::text(status.as_u16(), status_text, body).with_headers(headers))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_TOGETHERAI_BASE_URL, TogetherAIProvider, TogetherAIProviderSettings,
        create_togetherai, togetherai_api_key_from,
    };
    use crate::embed::{EmbedManyOptions, embed_many};
    use crate::file_data::FileDataContent;
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::image_model::{ImageModel, ImageModelCallOptions, ImageModelFile};
    use crate::json::JsonValue;
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::prompt::Prompt;
    use crate::provider::{Provider, ProviderOptions, ProviderWithRerankingModel};
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    };
    use crate::reranking_model::{
        RerankingModel, RerankingModelCallOptions, RerankingModelDocuments,
    };
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    use url::Url;

    #[test]
    fn togetherai_provider_creates_chat_model_with_headers_base_url_and_body() {
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
                        "id": "chatcmpl-togetherai",
                        "created": 1711115037,
                        "model": "meta-llama/Llama-3.3-70B-Instruct-Turbo",
                        "choices": [
                            {
                                "index": 0,
                                "message": {
                                    "role": "assistant",
                                    "content": "Hello from TogetherAI"
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
                    "req_togetherai".to_string(),
                )])))))
            });
        let provider = create_togetherai(
            TogetherAIProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.together.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("meta-llama/Llama-3.3-70B-Instruct-Turbo");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(16)
                .with_temperature(0.0),
        ));

        assert_eq!(model.provider(), "togetherai.chat");
        assert_eq!(model.model_id(), "meta-llama/Llama-3.3-70B-Instruct-Turbo");
        assert_eq!(result.text, "Hello from TogetherAI");
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("chatcmpl-togetherai")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.together.test/v1/chat/completions");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/togetherai/0.1.0")),
            "TogetherAI user-agent suffix is included"
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "meta-llama/Llama-3.3-70B-Instruct-Turbo",
                "messages": [
                    {
                        "role": "user",
                        "content": "Say hello"
                    }
                ],
                "max_tokens": 16,
                "temperature": 0.0
            }))
        );
    }

    #[test]
    fn togetherai_provider_creates_completion_model() {
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
                        "id": "cmpl-togetherai",
                        "created": 1711115037,
                        "model": "completion-model",
                        "choices": [
                            {
                                "index": 0,
                                "text": " completed",
                                "finish_reason": "stop"
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 2,
                            "completion_tokens": 1,
                            "total_tokens": 3
                        }
                    })
                    .to_string(),
                ))))
            });
        let model = TogetherAIProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.together.test/v1/")
            .with_transport(transport)
            .completion_model("completion-model");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Complete this"))
                .expect("prompt is valid"),
        ));

        assert_eq!(model.provider(), "togetherai.completion");
        assert_eq!(result.text, " completed");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.url, "https://api.together.test/v1/completions");
    }

    #[test]
    fn togetherai_provider_creates_embedding_model_aliases() {
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
                        "model": "BAAI/bge-large-en-v1.5",
                        "data": [
                            {
                                "index": 0,
                                "embedding": [0.1, 0.2, 0.3]
                            },
                            {
                                "index": 1,
                                "embedding": [0.4, 0.5, 0.6]
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "total_tokens": 4
                        }
                    })
                    .to_string(),
                ))))
            });
        let provider = TogetherAIProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.together.test/v1/")
            .with_transport(transport);
        let model = provider.embedding_model("BAAI/bge-large-en-v1.5");
        let result = poll_ready(embed_many(EmbedManyOptions::new(
            &model,
            ["sunny day", "rainy city"],
        )));

        assert_eq!(model.provider(), "togetherai.embedding");
        assert_eq!(
            provider.embedding("BAAI/bge-large-en-v1.5").provider(),
            "togetherai.embedding"
        );
        assert_eq!(
            provider
                .text_embedding_model("BAAI/bge-large-en-v1.5")
                .provider(),
            "togetherai.embedding"
        );
        assert_eq!(result.embeddings.len(), 2);
        assert_eq!(result.embeddings[0], vec![0.1, 0.2, 0.3]);

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.url, "https://api.together.test/v1/embeddings");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "BAAI/bge-large-en-v1.5",
                "input": ["sunny day", "rainy city"],
                "encoding_format": "float"
            }))
        );
    }

    #[test]
    fn togetherai_provider_uses_default_base_url_and_function_alias() {
        let model = super::togetherai("meta-llama/Llama-3.3-70B-Instruct-Turbo");

        assert_eq!(model.provider(), "togetherai.chat");
        assert_eq!(model.model_id(), "meta-llama/Llama-3.3-70B-Instruct-Turbo");
        assert_eq!(DEFAULT_TOGETHERAI_BASE_URL, "https://api.together.xyz/v1");
    }

    #[test]
    fn togetherai_provider_creates_image_model_and_generates_images() {
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
                        "data": [
                            {
                                "b64_json": "together-image-data"
                            }
                        ]
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_togetherai_image".to_string(),
                )])))))
            });
        let provider = create_togetherai(
            TogetherAIProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.together.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.image_model("stabilityai/stable-diffusion-xl");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "togetherai": {
                "steps": 28,
                "guidance": 3.5
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(2)
                    .with_prompt("A watercolor mountain")
                    .with_size("1024x768")
                    .with_aspect_ratio("1:1")
                    .with_seed(42)
                    .with_files(vec![
                        ImageModelFile::url(
                            Url::parse("https://example.com/input1.jpg").expect("url parses"),
                        ),
                        ImageModelFile::url(
                            Url::parse("https://example.com/input2.jpg").expect("url parses"),
                        ),
                    ])
                    .with_provider_options(provider_options)
                    .with_header("x-call", "image"),
            ),
        );

        assert_eq!(model.provider(), "togetherai.image");
        assert_eq!(model.model_id(), "stabilityai/stable-diffusion-xl");
        assert_eq!(poll_ready(model.max_images_per_call()), Some(1));
        assert_eq!(
            result.images,
            vec![FileDataContent::Base64("together-image-data".to_string())]
        );
        assert_eq!(result.warnings.len(), 2);
        assert!(result.warnings.iter().any(|warning| {
            matches!(warning, crate::warning::Warning::Unsupported { feature, .. } if feature == "aspectRatio")
        }));
        assert!(result.warnings.iter().any(|warning| {
            matches!(warning, crate::warning::Warning::Other { message } if message.contains("single input image"))
        }));
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_togetherai_image")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            request.url,
            "https://api.together.test/v1/images/generations"
        );
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert_eq!(
            request.headers.get("x-call").map(String::as_str),
            Some("image")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/togetherai/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "stabilityai/stable-diffusion-xl",
                "prompt": "A watercolor mountain",
                "seed": 42,
                "n": 2,
                "width": 1024,
                "height": 768,
                "image_url": "https://example.com/input1.jpg",
                "response_format": "base64",
                "steps": 28,
                "guidance": 3.5
            }))
        );
    }

    #[test]
    fn togetherai_image_model_maps_api_error_to_metadata() {
        let transport: OpenAICompatibleTransport =
            Arc::new(move |_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    400,
                    "Bad Request",
                    json!({
                        "error": {
                            "message": "bad image request"
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_togetherai_image_error".to_string(),
                )])))))
            });
        let model = TogetherAIProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .image_model("stabilityai/stable-diffusion-xl");
        let result = poll_ready(
            model.do_generate(ImageModelCallOptions::new(1).with_prompt("bad image request")),
        );

        assert!(result.images.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("togetherai"))
                .and_then(|metadata| metadata.extra.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("bad image request")
        );
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_togetherai_image_error")
        );
    }

    #[test]
    fn togetherai_image_model_reports_unsupported_mask_without_request() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: OpenAICompatibleTransport =
            Arc::new(move |request| -> OpenAICompatibleTransportFuture {
                *captured_request_for_transport
                    .lock()
                    .expect("captured request mutex is not poisoned") = Some(request.clone());

                Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", "{}"))))
            });
        let model = TogetherAIProvider::new()
            .with_transport(transport)
            .image_model("black-forest-labs/FLUX.1-kontext-pro");
        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("Inpaint this area")
                    .with_files(vec![ImageModelFile::url(
                        Url::parse("https://example.com/input.jpg").expect("url parses"),
                    )])
                    .with_mask(ImageModelFile::url(
                        Url::parse("https://example.com/mask.png").expect("url parses"),
                    )),
            ),
        );

        assert!(result.images.is_empty());
        assert!(result.warnings.iter().any(|warning| {
            matches!(warning, crate::warning::Warning::Unsupported { feature, .. } if feature == "mask")
        }));
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("togetherai"))
                .and_then(|metadata| metadata.extra.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some(super::togetherai_unsupported_mask_message().as_str())
        );
        assert!(
            captured_request
                .lock()
                .expect("captured request mutex is not poisoned")
                .is_none()
        );
    }

    #[test]
    fn togetherai_provider_creates_reranking_model() {
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
                        "id": "rerank-response",
                        "model": "Salesforce/Llama-Rank-v1",
                        "object": "rerank",
                        "results": [
                            {
                                "index": 1,
                                "relevance_score": 0.91
                            },
                            {
                                "index": 0,
                                "relevance_score": 0.82
                            }
                        ],
                        "usage": {
                            "prompt_tokens": 4,
                            "total_tokens": 4
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_togetherai_rerank".to_string(),
                )])))))
            });
        let provider = TogetherAIProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.together.test/v1/")
            .with_header("custom-header", "value")
            .with_transport(transport);
        let model = provider.reranking_model("Salesforce/Llama-Rank-v1");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "togetherai": {
                "rankFields": ["example"]
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(
            model.do_rerank(
                RerankingModelCallOptions::new(
                    RerankingModelDocuments::object(vec![
                        serde_json::from_value(json!({
                            "example": "sunny day at the beach"
                        }))
                        .expect("object document deserializes"),
                        serde_json::from_value(json!({
                            "example": "rainy day in the city"
                        }))
                        .expect("object document deserializes"),
                    ]),
                    "rainy day",
                )
                .with_top_n(2)
                .with_provider_options(provider_options)
                .with_header("x-call", "rerank"),
            ),
        );

        assert_eq!(model.provider(), "togetherai.reranking");
        assert_eq!(model.model_id(), "Salesforce/Llama-Rank-v1");
        assert_eq!(result.ranking[0].index, 1);
        assert_eq!(result.ranking[0].relevance_score, 0.91);
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("rerank-response")
        );
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.model_id.as_deref()),
            Some("Salesforce/Llama-Rank-v1")
        );
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_togetherai_rerank")
        );
        assert!(
            result
                .response
                .as_ref()
                .and_then(|response| response.body.as_ref())
                .and_then(|body| body.get("usage"))
                .is_some()
        );
        assert_eq!(
            provider.reranking("Salesforce/Llama-Rank-v1").provider(),
            "togetherai.reranking"
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.together.test/v1/rerank");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
        );
        assert_eq!(
            request.headers.get("x-call").map(String::as_str),
            Some("rerank")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/togetherai/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "Salesforce/Llama-Rank-v1",
                "documents": [
                    {
                        "example": "sunny day at the beach"
                    },
                    {
                        "example": "rainy day in the city"
                    }
                ],
                "query": "rainy day",
                "top_n": 2,
                "rank_fields": ["example"],
                "return_documents": false
            }))
        );
    }

    #[test]
    fn togetherai_reranking_model_maps_api_error_to_metadata() {
        let transport: OpenAICompatibleTransport =
            Arc::new(move |_request| -> OpenAICompatibleTransportFuture {
                Box::pin(ready(Ok(ProviderApiResponse::text(
                    400,
                    "Bad Request",
                    json!({
                        "error": {
                            "message": "bad rerank request"
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_togetherai_rerank_error".to_string(),
                )])))))
            });
        let model = TogetherAIProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .reranking_model("Salesforce/Llama-Rank-v1");
        let result = poll_ready(model.do_rerank(RerankingModelCallOptions::new(
            RerankingModelDocuments::text(vec!["sunny day".to_string(), "rainy city".to_string()]),
            "rainy",
        )));

        assert!(result.ranking.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("togetherai"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("bad rerank request")
        );
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_togetherai_rerank_error")
        );
    }

    #[test]
    fn togetherai_api_key_prefers_explicit_then_new_env_then_deprecated_env() {
        let explicit = "explicit-key".to_string();

        assert_eq!(
            togetherai_api_key_from(Some(&explicit), |_| Some("env-key".to_string())),
            Some("explicit-key".to_string())
        );
        assert_eq!(
            togetherai_api_key_from(None, |name| match name {
                "TOGETHER_API_KEY" => Some("new-env-key".to_string()),
                "TOGETHER_AI_API_KEY" => Some("deprecated-env-key".to_string()),
                _ => None,
            }),
            Some("new-env-key".to_string())
        );
        assert_eq!(
            togetherai_api_key_from(None, |name| match name {
                "TOGETHER_AI_API_KEY" => Some("deprecated-env-key".to_string()),
                _ => None,
            }),
            Some("deprecated-env-key".to_string())
        );
        assert_eq!(
            togetherai_api_key_from(Some(&String::new()), |name| match name {
                "TOGETHER_API_KEY" => Some(String::new()),
                "TOGETHER_AI_API_KEY" => Some("deprecated-env-key".to_string()),
                _ => None,
            }),
            Some("deprecated-env-key".to_string())
        );
    }

    #[test]
    fn togetherai_provider_implements_provider_trait() {
        let provider = TogetherAIProvider::new();
        let model = Provider::language_model(&provider, "meta-llama/Llama-3.3-70B-Instruct-Turbo")
            .expect("language model is supported");

        assert_eq!(model.provider(), "togetherai.chat");
        assert_eq!(model.model_id(), "meta-llama/Llama-3.3-70B-Instruct-Turbo");
        let embedding = Provider::embedding_model(&provider, "BAAI/bge-large-en-v1.5")
            .expect("embedding model is supported");
        assert_eq!(embedding.provider(), "togetherai.embedding");
        let image = Provider::image_model(&provider, "stabilityai/stable-diffusion-xl")
            .expect("image model is supported");
        assert_eq!(image.provider(), "togetherai.image");
        assert_eq!(image.model_id(), "stabilityai/stable-diffusion-xl");
        let reranking =
            ProviderWithRerankingModel::reranking_model(&provider, "Salesforce/Llama-Rank-v1")
                .expect("reranking model is supported");
        assert_eq!(reranking.provider(), "togetherai.reranking");
        assert_eq!(reranking.model_id(), "Salesforce/Llama-Rank-v1");
    }

    #[test]
    fn togetherai_provider_settings_serde_accepts_upstream_base_url() {
        let settings: TogetherAIProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.together.test/v1",
            "apiKey": "test-key",
            "headers": {
                "custom-header": "value"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            TogetherAIProviderSettings::new()
                .with_base_url("https://api.together.test/v1")
                .with_api_key("test-key")
                .with_header("custom-header", "value")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://api.together.test/v1",
                "apiKey": "test-key",
                "headers": {
                    "custom-header": "value"
                }
            })
        );
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);
        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => {
                struct NoopWake;

                impl Wake for NoopWake {
                    fn wake(self: Arc<Self>) {}
                }

                let waker = Waker::from(Arc::new(NoopWake));
                let mut context = Context::from_waker(&waker);
                loop {
                    match Pin::new(&mut future).poll(&mut context) {
                        Poll::Ready(value) => break value,
                        Poll::Pending => std::thread::yield_now(),
                    }
                }
            }
        }
    }
}
