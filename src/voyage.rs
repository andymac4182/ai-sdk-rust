use std::collections::BTreeMap;
use std::env;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::embedding_model::{
    EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResponse, EmbeddingModelResult,
    EmbeddingModelUsage,
};
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::openai_compatible::{OpenAICompatibleChatLanguageModel, OpenAICompatibleImageModel};
use crate::provider::{
    ModelType, NoSuchModelError, Provider, ProviderMetadata, ProviderOptions,
    ProviderWithRerankingModel, TooManyEmbeddingValuesForCallError,
};
use crate::provider_utils::{
    FetchErrorInfo, HandledFetchError, PostJsonToApiOptions, ProviderApiRequest,
    ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, RuntimeEnvironment, combine_headers,
    create_json_error_response_handler, create_json_response_handler, post_json_to_api,
    with_user_agent_suffix, without_trailing_slash,
};
use crate::reranking_model::{
    RerankingModel, RerankingModelCallOptions, RerankingModelDocuments, RerankingModelRanking,
    RerankingModelResponse, RerankingModelResult,
};
use crate::warning::Warning;

/// Default base URL for upstream `@ai-sdk/voyage` API calls.
pub const DEFAULT_VOYAGE_BASE_URL: &str = "https://api.voyageai.com/v1";

/// Settings for the upstream Voyage provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoyageProviderSettings {
    /// Base URL for Voyage API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Voyage API key. When omitted, `VOYAGE_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl VoyageProviderSettings {
    /// Creates empty Voyage provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Voyage API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the Voyage API key.
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

/// Upstream Voyage provider foundation.
#[derive(Clone)]
pub struct VoyageProvider {
    settings: VoyageProviderSettings,
    transport: VoyageTransport,
}

/// Voyage embedding model for `/embeddings` calls.
#[derive(Clone)]
pub struct VoyageEmbeddingModel {
    model_id: String,
    base_url: String,
    settings: VoyageProviderSettings,
    transport: VoyageTransport,
}

/// Voyage reranking model for `/rerank` calls.
#[derive(Clone)]
pub struct VoyageRerankingModel {
    model_id: String,
    base_url: String,
    settings: VoyageProviderSettings,
    transport: VoyageTransport,
}

/// Future returned by an injected Voyage HTTP transport.
pub type VoyageTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by Voyage provider models.
pub type VoyageTransport = Arc<dyn Fn(ProviderApiRequest) -> VoyageTransportFuture + Send + Sync>;

impl VoyageProvider {
    /// Creates a Voyage provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(VoyageProviderSettings::new())
    }

    /// Creates a provider from explicit Voyage settings.
    pub fn from_settings(settings: VoyageProviderSettings) -> Self {
        Self {
            settings,
            transport: default_voyage_transport(),
        }
    }

    /// Sets the Voyage API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the Voyage API base URL for this provider.
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
    pub fn with_transport(mut self, transport: VoyageTransport) -> Self {
        self.transport = transport;
        self
    }

    /// Creates a Voyage embedding model.
    pub fn embedding(&self, model_id: impl Into<String>) -> VoyageEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Creates a Voyage embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> VoyageEmbeddingModel {
        VoyageEmbeddingModel::new(
            model_id,
            voyage_base_url(&self.settings),
            self.settings.clone(),
            Arc::clone(&self.transport),
        )
    }

    /// Deprecated upstream alias for [`VoyageProvider::embedding_model`].
    pub fn text_embedding(&self, model_id: impl Into<String>) -> VoyageEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Deprecated upstream alias for [`VoyageProvider::embedding_model`].
    pub fn text_embedding_model(&self, model_id: impl Into<String>) -> VoyageEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Creates a Voyage reranking model.
    pub fn reranking(&self, model_id: impl Into<String>) -> VoyageRerankingModel {
        self.reranking_model(model_id)
    }

    /// Creates a Voyage reranking model.
    pub fn reranking_model(&self, model_id: impl Into<String>) -> VoyageRerankingModel {
        VoyageRerankingModel::new(
            model_id,
            voyage_base_url(&self.settings),
            self.settings.clone(),
            Arc::clone(&self.transport),
        )
    }

    /// Reports that Voyage does not expose language models through this provider.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::LanguageModel))
    }

    /// Reports that Voyage does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }
}

impl Default for VoyageProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for VoyageProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = VoyageEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        VoyageProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(VoyageProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        VoyageProvider::image_model(self, model_id)
    }
}

impl ProviderWithRerankingModel for VoyageProvider {
    type RerankingModel = VoyageRerankingModel;

    fn reranking_model(&self, model_id: &str) -> Result<Self::RerankingModel, NoSuchModelError> {
        Ok(VoyageProvider::reranking_model(self, model_id))
    }
}

impl VoyageEmbeddingModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: VoyageProviderSettings,
        transport: VoyageTransport,
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
        "voyage.embedding"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: VoyageTransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_embed_result(&self, options: EmbeddingModelCallOptions) -> EmbeddingModelResult {
        let request_body = voyage_embedding_request_body(&self.model_id, &options);
        let request_body_for_error = request_body.clone();

        if options.values.len() > 128 {
            let error = TooManyEmbeddingValuesForCallError::new(
                self.provider(),
                self.model_id.clone(),
                128,
                options.values,
            );
            return voyage_embedding_error_result(
                error.to_string(),
                request_body_for_error,
                None,
                None,
            );
        }

        let request_headers = self.request_headers(options.headers.as_ref());
        let post_options = PostJsonToApiOptions::new(self.embedding_model_url(), request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    voyage_embedding_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    clone_json_value,
                    voyage_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => voyage_embedding_result_from_response(
                response.value,
                response.raw_value,
                response.response_headers,
            ),
            Err(error) => voyage_embedding_result_from_error(error, request_body_for_error),
        }
    }

    fn embedding_model_url(&self) -> String {
        format!("{}/embeddings", self.base_url)
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(voyage_provider_header_entries(&self.settings)),
            optional_headers(call_headers),
        ])
    }
}

impl EmbeddingModel for VoyageEmbeddingModel {
    type MaxEmbeddingsPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type SupportsParallelCallsFuture<'a>
        = Ready<bool>
    where
        Self: 'a;

    type EmbedFuture<'a>
        = Pin<Box<dyn Future<Output = EmbeddingModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        VoyageEmbeddingModel::provider(self)
    }

    fn model_id(&self) -> &str {
        VoyageEmbeddingModel::model_id(self)
    }

    fn max_embeddings_per_call(&self) -> Self::MaxEmbeddingsPerCallFuture<'_> {
        ready(Some(128))
    }

    fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
        ready(true)
    }

    fn do_embed(&self, options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
        Box::pin(self.do_embed_result(options))
    }
}

impl VoyageRerankingModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: VoyageProviderSettings,
        transport: VoyageTransport,
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
        "voyage.reranking"
    }

    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: VoyageTransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_rerank_result(&self, options: RerankingModelCallOptions) -> RerankingModelResult {
        let warnings = voyage_reranking_warnings(&options.documents);
        let request_body = voyage_reranking_request_body(&self.model_id, &options);
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
                    voyage_reranking_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    clone_json_value,
                    voyage_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => voyage_reranking_result_from_response(
                response.value,
                response.raw_value,
                response.response_headers,
                warnings,
            ),
            Err(error) => {
                voyage_reranking_result_from_error(error, Some(request_body_for_error), warnings)
            }
        }
    }

    fn reranking_model_url(&self) -> String {
        format!("{}/rerank", self.base_url)
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(voyage_provider_header_entries(&self.settings)),
            optional_headers(call_headers),
        ])
    }
}

impl RerankingModel for VoyageRerankingModel {
    type RerankFuture<'a>
        = Pin<Box<dyn Future<Output = RerankingModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        VoyageRerankingModel::provider(self)
    }

    fn model_id(&self) -> &str {
        VoyageRerankingModel::model_id(self)
    }

    fn do_rerank(&self, options: RerankingModelCallOptions) -> Self::RerankFuture<'_> {
        Box::pin(self.do_rerank_result(options))
    }
}

/// Creates a Voyage provider with explicit settings.
pub fn create_voyage(settings: VoyageProviderSettings) -> VoyageProvider {
    VoyageProvider::from_settings(settings)
}

/// Creates a Voyage provider with default settings.
pub fn voyage() -> VoyageProvider {
    VoyageProvider::new()
}

fn voyage_base_url(settings: &VoyageProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_VOYAGE_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn voyage_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    voyage_api_key_from(explicit_api_key, |name| env::var(name).ok())
}

fn voyage_api_key_from(
    explicit_api_key: Option<&String>,
    env_var: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env_var("VOYAGE_API_KEY")))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn voyage_provider_header_entries(
    settings: &VoyageProviderSettings,
) -> Vec<(String, Option<String>)> {
    let mut headers = Vec::new();

    if let Some(api_key) = voyage_api_key(settings.api_key.as_ref()) {
        headers.push((
            "authorization".to_string(),
            Some(format!("Bearer {api_key}")),
        ));
    }

    headers.extend(
        settings
            .headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    );

    with_user_agent_suffix(Some(headers), [format!("ai-sdk/voyage/{}", crate::VERSION)])
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

fn voyage_embedding_request_body(model_id: &str, options: &EmbeddingModelCallOptions) -> JsonValue {
    let provider_options = voyage_provider_options(options.provider_options.as_ref());
    let mut body = JsonObject::new();
    body.insert("input".to_string(), json!(options.values));
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));

    if let Some(input_type) = provider_option_string(provider_options, "inputType", "input_type") {
        body.insert("input_type".to_string(), JsonValue::String(input_type));
    }

    if let Some(truncation) = provider_option_bool(provider_options, "truncation", "truncation") {
        body.insert("truncation".to_string(), JsonValue::Bool(truncation));
    }

    if let Some(output_dimension) =
        provider_option_u64(provider_options, "outputDimension", "output_dimension")
    {
        body.insert(
            "output_dimension".to_string(),
            JsonValue::from(output_dimension),
        );
    }

    if let Some(output_dtype) =
        provider_option_string(provider_options, "outputDtype", "output_dtype")
    {
        body.insert("output_dtype".to_string(), JsonValue::String(output_dtype));
    }

    JsonValue::Object(body)
}

fn voyage_reranking_request_body(model_id: &str, options: &RerankingModelCallOptions) -> JsonValue {
    let provider_options = voyage_provider_options(options.provider_options.as_ref());
    let mut body = JsonObject::new();
    body.insert(
        "query".to_string(),
        JsonValue::String(options.query.clone()),
    );
    body.insert(
        "documents".to_string(),
        voyage_reranking_documents(&options.documents),
    );
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));

    if let Some(top_n) = options.top_n {
        body.insert("top_k".to_string(), JsonValue::from(top_n));
    }

    if let Some(return_documents) =
        provider_option_bool(provider_options, "returnDocuments", "return_documents")
    {
        body.insert(
            "return_documents".to_string(),
            JsonValue::Bool(return_documents),
        );
    }

    if let Some(truncation) = provider_option_bool(provider_options, "truncation", "truncation") {
        body.insert("truncation".to_string(), JsonValue::Bool(truncation));
    }

    JsonValue::Object(body)
}

fn voyage_provider_options(provider_options: Option<&ProviderOptions>) -> Option<&JsonObject> {
    provider_options.and_then(|options| options.get("voyage"))
}

fn provider_option_string(
    options: Option<&JsonObject>,
    camel_key: &str,
    snake_key: &str,
) -> Option<String> {
    options
        .and_then(|options| options.get(camel_key).or_else(|| options.get(snake_key)))
        .and_then(JsonValue::as_str)
        .map(String::from)
}

fn provider_option_bool(
    options: Option<&JsonObject>,
    camel_key: &str,
    snake_key: &str,
) -> Option<bool> {
    options
        .and_then(|options| options.get(camel_key).or_else(|| options.get(snake_key)))
        .and_then(JsonValue::as_bool)
}

fn provider_option_u64(
    options: Option<&JsonObject>,
    camel_key: &str,
    snake_key: &str,
) -> Option<u64> {
    options
        .and_then(|options| options.get(camel_key).or_else(|| options.get(snake_key)))
        .and_then(JsonValue::as_u64)
}

fn voyage_reranking_documents(documents: &RerankingModelDocuments) -> JsonValue {
    match documents {
        RerankingModelDocuments::Text { values } => json!(values),
        RerankingModelDocuments::Object { values } => JsonValue::Array(
            values
                .iter()
                .map(|value| serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string()))
                .map(JsonValue::String)
                .collect(),
        ),
    }
}

fn voyage_reranking_warnings(documents: &RerankingModelDocuments) -> Vec<Warning> {
    match documents {
        RerankingModelDocuments::Text { .. } => Vec::new(),
        RerankingModelDocuments::Object { .. } => vec![Warning::Compatibility {
            feature: "object documents".to_string(),
            details: Some("Object documents are converted to strings.".to_string()),
        }],
    }
}

#[derive(Clone, Debug, Deserialize)]
struct VoyageEmbeddingResponse {
    data: Vec<VoyageEmbeddingData>,
    #[serde(default)]
    usage: Option<VoyageEmbeddingUsage>,
}

#[derive(Clone, Debug, Deserialize)]
struct VoyageEmbeddingData {
    embedding: Vec<f64>,
    index: usize,
}

#[derive(Clone, Debug, Deserialize)]
struct VoyageEmbeddingUsage {
    total_tokens: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct VoyageRerankingResponse {
    data: Vec<VoyageRerankingData>,
}

#[derive(Clone, Debug, Deserialize)]
struct VoyageRerankingData {
    index: usize,
    relevance_score: f64,
}

fn voyage_embedding_response(
    value: &JsonValue,
) -> Result<VoyageEmbeddingResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn voyage_reranking_response(
    value: &JsonValue,
) -> Result<VoyageRerankingResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn clone_json_value(value: &JsonValue) -> Result<JsonValue, serde_json::Error> {
    Ok(value.clone())
}

fn voyage_error_message(value: &JsonValue) -> String {
    value
        .get("detail")
        .and_then(JsonValue::as_str)
        .or_else(|| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(JsonValue::as_str)
        })
        .or_else(|| value.get("message").and_then(JsonValue::as_str))
        .unwrap_or("Unknown error")
        .to_string()
}

fn voyage_embedding_result_from_response(
    response: VoyageEmbeddingResponse,
    raw_response: Option<JsonValue>,
    response_headers: Option<Headers>,
) -> EmbeddingModelResult {
    let mut data = response.data;
    data.sort_by_key(|item| item.index);

    let mut result = EmbeddingModelResult::new(
        data.into_iter()
            .map(|item| item.embedding)
            .collect::<Vec<_>>(),
    )
    .with_usage(EmbeddingModelUsage::new(
        response.usage.map_or(0, |usage| usage.total_tokens),
    ));
    let mut response_metadata = EmbeddingModelResponse::new();

    if let Some(raw_response) = raw_response {
        response_metadata = response_metadata.with_body(raw_response);
    }

    if let Some(headers) = response_headers {
        response_metadata = with_embedding_response_headers(response_metadata, headers);
    }

    result = result.with_response(response_metadata);
    result
}

fn voyage_embedding_result_from_error(
    error: HandledFetchError,
    request_body: JsonValue,
) -> EmbeddingModelResult {
    let (message, headers, body) = match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None, None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
            error.response_body().map(String::from),
        ),
    };

    voyage_embedding_error_result(message, request_body, headers, body.as_deref())
}

fn voyage_embedding_error_result(
    message: String,
    request_body: JsonValue,
    response_headers: Option<Headers>,
    raw_body: Option<&str>,
) -> EmbeddingModelResult {
    let response_body = raw_body
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
        .or_else(|| raw_body.map(|body| JsonValue::String(body.to_string())))
        .unwrap_or(request_body);
    let mut response = EmbeddingModelResponse::new().with_body(response_body);

    if let Some(headers) = response_headers {
        response = with_embedding_response_headers(response, headers);
    }

    EmbeddingModelResult::new(Vec::new())
        .with_usage(EmbeddingModelUsage::new(0))
        .with_provider_metadata(voyage_error_metadata(message))
        .with_response(response)
}

fn with_embedding_response_headers(
    mut response: EmbeddingModelResponse,
    headers: Headers,
) -> EmbeddingModelResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }

    response
}

fn voyage_reranking_result_from_response(
    response: VoyageRerankingResponse,
    raw_response: Option<JsonValue>,
    response_headers: Option<Headers>,
    warnings: Vec<Warning>,
) -> RerankingModelResult {
    let ranking = response
        .data
        .into_iter()
        .map(|result| RerankingModelRanking::new(result.index, result.relevance_score))
        .collect();
    let mut result = RerankingModelResult::new(ranking);
    let mut response_metadata = RerankingModelResponse::new();

    if let Some(body) = raw_response {
        response_metadata = response_metadata.with_body(body);
    }

    if let Some(headers) = response_headers {
        response_metadata = with_reranking_response_headers(response_metadata, headers);
    }

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result.with_response(response_metadata)
}

fn voyage_reranking_result_from_error(
    error: HandledFetchError,
    request_body: Option<JsonValue>,
    warnings: Vec<Warning>,
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

    let mut result = RerankingModelResult::new(Vec::new())
        .with_provider_metadata(voyage_error_metadata(message))
        .with_response(response);

    for warning in warnings {
        result = result.with_warning(warning);
    }

    result
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

fn voyage_error_metadata(message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut provider = JsonObject::new();
    provider.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert("voyage".to_string(), provider);
    metadata
}

fn default_voyage_transport() -> VoyageTransport {
    Arc::new(|request| Box::pin(ready(execute_voyage_request(request))))
}

fn execute_voyage_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_voyage_get_request(request),
        ProviderApiRequestMethod::Post => execute_voyage_post_request(request),
    }
}

fn execute_voyage_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    voyage_provider_api_response(response)
}

fn execute_voyage_post_request(
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
                "multipart form data is not supported by the Voyage transport",
            ));
        }
        None => builder.send_empty(),
    };

    voyage_provider_api_response(response)
}

fn voyage_provider_api_response(
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

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_VOYAGE_BASE_URL, VoyageProvider, VoyageProviderSettings, VoyageTransport,
        VoyageTransportFuture, create_voyage, voyage, voyage_api_key_from,
    };
    use crate::embed::{EmbedManyOptions, embed_many};
    use crate::embedding_model::EmbeddingModel;
    use crate::headers::Headers;
    use crate::json::JsonValue;
    use crate::provider::{ModelType, Provider, ProviderOptions, ProviderWithRerankingModel};
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    };
    use crate::reranking_model::{
        RerankingModel, RerankingModelCallOptions, RerankingModelDocuments,
    };
    use crate::warning::Warning;
    use serde_json::json;
    use std::future::Future;
    use std::future::ready;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

    #[test]
    fn voyage_provider_creates_embedding_model_with_options_headers_and_sorted_results() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: VoyageTransport = Arc::new(move |request| -> VoyageTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "data": [
                        {
                            "embedding": [0.4, 0.5, 0.6],
                            "index": 1
                        },
                        {
                            "embedding": [0.1, 0.2, 0.3],
                            "index": 0
                        }
                    ],
                    "model": "voyage-3-large",
                    "usage": {
                        "total_tokens": 8
                    }
                })
                .to_string(),
            )
            .with_headers(Headers::from([(
                "x-request-id".to_string(),
                "req_voyage_embedding".to_string(),
            )])))))
        });
        let provider = create_voyage(
            VoyageProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.voyage.test/v1/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.embedding_model("voyage-3-large");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "voyage": {
                "inputType": "query",
                "truncation": true,
                "outputDimension": 512,
                "outputDtype": "float"
            }
        }))
        .expect("provider options deserialize");
        let result = poll_ready(embed_many(
            EmbedManyOptions::new(&model, ["sunny day", "rainy city"])
                .with_provider_options(provider_options)
                .with_header("x-call", "embed"),
        ));

        assert_eq!(model.provider(), "voyage.embedding");
        assert_eq!(model.model_id(), "voyage-3-large");
        assert_eq!(
            provider.text_embedding("voyage-3-large").provider(),
            "voyage.embedding"
        );
        assert_eq!(
            provider.text_embedding_model("voyage-3-large").provider(),
            "voyage.embedding"
        );
        assert_eq!(result.embeddings[0], vec![0.1, 0.2, 0.3]);
        assert_eq!(result.embeddings[1], vec![0.4, 0.5, 0.6]);
        assert_eq!(result.usage.tokens, 8);
        assert_eq!(
            result
                .responses
                .as_ref()
                .and_then(|responses| responses.first())
                .and_then(Option::as_ref)
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_voyage_embedding")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.voyage.test/v1/embeddings");
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
            Some("embed")
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "input": ["sunny day", "rainy city"],
                "model": "voyage-3-large",
                "input_type": "query",
                "truncation": true,
                "output_dimension": 512,
                "output_dtype": "float"
            }))
        );
    }

    #[test]
    fn voyage_embedding_model_chunks_at_128_and_maps_api_error_to_metadata() {
        let call_count = Arc::new(Mutex::new(0_u32));
        let call_count_for_transport = Arc::clone(&call_count);
        let transport: VoyageTransport = Arc::new(move |_request| -> VoyageTransportFuture {
            let mut count = call_count_for_transport
                .lock()
                .expect("call count mutex is not poisoned");
            *count += 1;
            let index = *count;

            Box::pin(ready(Ok(if index == 1 {
                ProviderApiResponse::text(
                    200,
                    "OK",
                    json!({
                        "data": (0..128)
                            .map(|index| json!({
                                "embedding": [index as f64],
                                "index": index
                            }))
                            .collect::<Vec<_>>(),
                        "model": "voyage-3",
                        "usage": {
                            "total_tokens": 128
                        }
                    })
                    .to_string(),
                )
            } else {
                ProviderApiResponse::text(
                    429,
                    "Too Many Requests",
                    json!({
                        "detail": "rate limit exceeded"
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_voyage_embedding_error".to_string(),
                )]))
            })))
        });
        let model = VoyageProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .embedding_model("voyage-3");
        let values = (0..129)
            .map(|index| format!("document {index}"))
            .collect::<Vec<_>>();
        let result = poll_ready(embed_many(EmbedManyOptions::new(&model, values)));

        assert_eq!(result.embeddings.len(), 128);
        assert_eq!(result.usage.tokens, 128);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("voyage"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("rate limit exceeded")
        );
        assert_eq!(
            result
                .responses
                .as_ref()
                .and_then(|responses| responses.get(1))
                .and_then(Option::as_ref)
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_voyage_embedding_error")
        );
    }

    #[test]
    fn voyage_provider_creates_reranking_model_with_object_warning_and_options() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: VoyageTransport = Arc::new(move |request| -> VoyageTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "data": [
                        {
                            "index": 1,
                            "relevance_score": 0.91
                        },
                        {
                            "index": 0,
                            "relevance_score": 0.32
                        }
                    ]
                })
                .to_string(),
            )
            .with_headers(Headers::from([(
                "x-request-id".to_string(),
                "req_voyage_rerank".to_string(),
            )])))))
        });
        let provider = VoyageProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.voyage.test/v1/")
            .with_header("custom-header", "value")
            .with_transport(transport);
        let model = provider.reranking_model("rerank-2.5");
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "voyage": {
                "returnDocuments": true,
                "truncation": false
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

        assert_eq!(model.provider(), "voyage.reranking");
        assert_eq!(model.model_id(), "rerank-2.5");
        assert_eq!(
            provider.reranking("rerank-2.5").provider(),
            "voyage.reranking"
        );
        assert_eq!(result.ranking[0].index, 1);
        assert_eq!(result.ranking[0].relevance_score, 0.91);
        assert_eq!(
            result.warnings,
            vec![Warning::Compatibility {
                feature: "object documents".to_string(),
                details: Some("Object documents are converted to strings.".to_string())
            }]
        );
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_voyage_rerank")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.voyage.test/v1/rerank");
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
                .is_some_and(|value| value.contains("ai-sdk/voyage/0.1.0"))
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "query": "rainy day",
                "documents": [
                    "{\"example\":\"sunny day at the beach\"}",
                    "{\"example\":\"rainy day in the city\"}"
                ],
                "model": "rerank-2.5",
                "top_k": 2,
                "return_documents": true,
                "truncation": false
            }))
        );
    }

    #[test]
    fn voyage_reranking_model_maps_api_error_to_metadata() {
        let transport: VoyageTransport = Arc::new(move |_request| -> VoyageTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                400,
                "Bad Request",
                json!({
                    "detail": "bad rerank request"
                })
                .to_string(),
            )
            .with_headers(Headers::from([(
                "x-request-id".to_string(),
                "req_voyage_rerank_error".to_string(),
            )])))))
        });
        let model = VoyageProvider::new()
            .with_api_key("test-api-key")
            .with_transport(transport)
            .reranking_model("rerank-2.5");
        let result = poll_ready(model.do_rerank(RerankingModelCallOptions::new(
            RerankingModelDocuments::text(vec!["sunny day".to_string(), "rainy city".to_string()]),
            "rainy",
        )));

        assert!(result.ranking.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("voyage"))
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
            Some("req_voyage_rerank_error")
        );
    }

    #[test]
    fn voyage_provider_reports_unsupported_language_and_image_models() {
        let provider = VoyageProvider::new();
        let language = match provider.language_model("chat-model") {
            Ok(_) => panic!("language models are unsupported"),
            Err(error) => error,
        };
        assert_eq!(language.model_type(), ModelType::LanguageModel);
        assert_eq!(language.message(), "No such languageModel: chat-model");

        let image = match provider.image_model("image-model") {
            Ok(_) => panic!("image models are unsupported"),
            Err(error) => error,
        };
        assert_eq!(image.model_type(), ModelType::ImageModel);
        assert_eq!(image.message(), "No such imageModel: image-model");
    }

    #[test]
    fn voyage_provider_uses_default_base_url_and_factory_alias() {
        let provider = voyage();
        let model = provider.embedding_model("voyage-3-large");

        assert_eq!(model.provider(), "voyage.embedding");
        assert_eq!(model.model_id(), "voyage-3-large");
        assert_eq!(
            super::voyage_base_url(&VoyageProviderSettings::new()),
            DEFAULT_VOYAGE_BASE_URL
        );
    }

    #[test]
    fn voyage_provider_implements_provider_traits() {
        let provider = VoyageProvider::new();
        let embedding =
            Provider::embedding_model(&provider, "voyage-3-large").expect("embedding exists");
        let reranking = ProviderWithRerankingModel::reranking_model(&provider, "rerank-2.5")
            .expect("reranking exists");

        assert_eq!(embedding.provider(), "voyage.embedding");
        assert_eq!(reranking.provider(), "voyage.reranking");
        assert!(Provider::language_model(&provider, "chat-model").is_err());
        assert!(Provider::image_model(&provider, "image-model").is_err());
    }

    #[test]
    fn voyage_provider_settings_serde_accepts_upstream_base_url() {
        let settings: VoyageProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.voyage.test/v1/",
            "apiKey": "test-api-key",
            "headers": {
                "custom-header": "value"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            VoyageProviderSettings::new()
                .with_base_url("https://api.voyage.test/v1/")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value")
        );
        assert_eq!(
            serde_json::to_value(settings).expect("settings serialize"),
            json!({
                "baseURL": "https://api.voyage.test/v1/",
                "apiKey": "test-api-key",
                "headers": {
                    "custom-header": "value"
                }
            })
        );
    }

    #[test]
    fn voyage_api_key_prefers_explicit_then_env() {
        let explicit = "explicit-key".to_string();

        assert_eq!(
            voyage_api_key_from(Some(&explicit), |_| Some("env-key".to_string())),
            Some("explicit-key".to_string())
        );
        assert_eq!(
            voyage_api_key_from(None, |name| match name {
                "VOYAGE_API_KEY" => Some("env-key".to_string()),
                _ => None,
            }),
            Some("env-key".to_string())
        );
        assert_eq!(
            voyage_api_key_from(Some(&String::new()), |name| match name {
                "VOYAGE_API_KEY" => Some("env-key".to_string()),
                _ => None,
            }),
            Some("env-key".to_string())
        );
    }

    #[test]
    fn voyage_embedding_direct_call_reports_too_many_values() {
        let model = VoyageProvider::new()
            .with_api_key("test-api-key")
            .embedding_model("voyage-3-large");
        let values = (0..129)
            .map(|index| format!("document {index}"))
            .collect::<Vec<_>>();
        let result = poll_ready(model.do_embed(
            crate::embedding_model::EmbeddingModelCallOptions::new(values),
        ));

        assert!(result.embeddings.is_empty());
        assert_eq!(result.usage.as_ref().map(|usage| usage.tokens), Some(0));
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("voyage"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some(
                "Too many values for a single embedding call. The voyage.embedding model \"voyage-3-large\" can only embed up to 128 values per call, but 129 values were provided."
            )
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
