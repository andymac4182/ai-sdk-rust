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
use crate::openai_compatible::{
    OpenAICompatibleChatLanguageModel, OpenAICompatibleImageModel, OpenAICompatibleTransport,
};
use crate::provider::{
    ModelType, NoSuchModelError, Provider, ProviderMetadata, ProviderWithRerankingModel,
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

/// Default base URL for upstream `@ai-sdk/cohere` API calls.
pub const DEFAULT_COHERE_BASE_URL: &str = "https://api.cohere.com/v2";

/// Settings for the upstream Cohere provider.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CohereProviderSettings {
    /// Base URL for Cohere API calls.
    #[serde(
        default,
        rename = "baseURL",
        alias = "baseUrl",
        skip_serializing_if = "Option::is_none"
    )]
    pub base_url: Option<String>,

    /// Cohere API key. When omitted, `COHERE_API_KEY` is read at model creation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl CohereProviderSettings {
    /// Creates empty Cohere provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the Cohere API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the Cohere API key.
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

/// Upstream Cohere provider foundation.
#[derive(Clone)]
pub struct CohereProvider {
    settings: CohereProviderSettings,
    transport: OpenAICompatibleTransport,
}

/// Cohere embedding model for `/embed` calls.
#[derive(Clone)]
pub struct CohereEmbeddingModel {
    model_id: String,
    base_url: String,
    settings: CohereProviderSettings,
    transport: OpenAICompatibleTransport,
}

/// Cohere reranking model for `/rerank` calls.
#[derive(Clone)]
pub struct CohereRerankingModel {
    model_id: String,
    base_url: String,
    settings: CohereProviderSettings,
    transport: OpenAICompatibleTransport,
}

impl CohereProvider {
    /// Creates a Cohere provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(CohereProviderSettings::new())
    }

    /// Creates a provider from explicit Cohere settings.
    pub fn from_settings(settings: CohereProviderSettings) -> Self {
        Self {
            settings,
            transport: default_cohere_transport(),
        }
    }

    /// Sets the Cohere API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the Cohere API base URL for this provider.
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
        self.transport = transport;
        self
    }

    /// Reports that Cohere chat is not yet ported in this Rust slice.
    pub fn language_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::LanguageModel))
    }

    /// Creates a Cohere embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> CohereEmbeddingModel {
        CohereEmbeddingModel::new(
            model_id,
            cohere_base_url(&self.settings),
            self.settings.clone(),
            Arc::clone(&self.transport),
        )
    }

    /// Alias for [`CohereProvider::embedding_model`].
    pub fn embedding(&self, model_id: impl Into<String>) -> CohereEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Deprecated upstream alias for [`CohereProvider::embedding_model`].
    pub fn text_embedding_model(&self, model_id: impl Into<String>) -> CohereEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Deprecated upstream alias for [`CohereProvider::embedding_model`].
    pub fn text_embedding(&self, model_id: impl Into<String>) -> CohereEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Reports that Cohere does not expose image models through this provider.
    pub fn image_model(
        &self,
        model_id: impl Into<String>,
    ) -> Result<OpenAICompatibleImageModel, NoSuchModelError> {
        Err(NoSuchModelError::new(model_id, ModelType::ImageModel))
    }

    /// Creates a Cohere reranking model.
    pub fn reranking_model(&self, model_id: impl Into<String>) -> CohereRerankingModel {
        CohereRerankingModel::new(
            model_id,
            cohere_base_url(&self.settings),
            self.settings.clone(),
            Arc::clone(&self.transport),
        )
    }

    /// Alias for [`CohereProvider::reranking_model`].
    pub fn reranking(&self, model_id: impl Into<String>) -> CohereRerankingModel {
        self.reranking_model(model_id)
    }
}

impl Default for CohereProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for CohereProvider {
    type LanguageModel = OpenAICompatibleChatLanguageModel;
    type EmbeddingModel = CohereEmbeddingModel;
    type ImageModel = OpenAICompatibleImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        CohereProvider::language_model(self, model_id)
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(CohereProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        CohereProvider::image_model(self, model_id)
    }
}

impl ProviderWithRerankingModel for CohereProvider {
    type RerankingModel = CohereRerankingModel;

    fn reranking_model(&self, model_id: &str) -> Result<Self::RerankingModel, NoSuchModelError> {
        Ok(CohereProvider::reranking_model(self, model_id))
    }
}

impl CohereEmbeddingModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: CohereProviderSettings,
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
        "cohere.textEmbedding"
    }

    async fn do_embed_result(&self, options: EmbeddingModelCallOptions) -> EmbeddingModelResult {
        let request_body = cohere_embedding_request_body(&self.model_id, &options);
        let request_body_for_error = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let post_options =
            PostJsonToApiOptions::new(format!("{}/embed", self.base_url), request_body)
                .with_headers(request_headers)
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(options.abort_signal);
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    cohere_embedding_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    cohere_error_response,
                    cohere_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => cohere_embedding_result_from_response(
                response.value,
                response.raw_value,
                response.response_headers,
            ),
            Err(error) => cohere_embedding_result_from_error(error, Some(request_body_for_error)),
        }
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(cohere_provider_header_entries(&self.settings)),
            optional_headers(call_headers),
        ])
    }
}

impl EmbeddingModel for CohereEmbeddingModel {
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
        CohereEmbeddingModel::provider(self)
    }

    fn model_id(&self) -> &str {
        CohereEmbeddingModel::model_id(self)
    }

    fn max_embeddings_per_call(&self) -> Self::MaxEmbeddingsPerCallFuture<'_> {
        ready(Some(96))
    }

    fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
        ready(true)
    }

    fn do_embed(&self, options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
        Box::pin(self.do_embed_result(options))
    }
}

impl CohereRerankingModel {
    fn new(
        model_id: impl Into<String>,
        base_url: impl Into<String>,
        settings: CohereProviderSettings,
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
        "cohere.reranking"
    }

    async fn do_rerank_result(&self, options: RerankingModelCallOptions) -> RerankingModelResult {
        let (request_body, warnings) = cohere_reranking_request_body(&self.model_id, &options);
        let request_body_for_error = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let post_options =
            PostJsonToApiOptions::new(format!("{}/rerank", self.base_url), request_body)
                .with_headers(request_headers)
                .with_environment(RuntimeEnvironment::unknown())
                .with_optional_abort_signal(options.abort_signal);
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    cohere_reranking_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    cohere_error_response,
                    cohere_error_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => {
                let mut result = cohere_reranking_result_from_response(
                    response.value,
                    response.raw_value,
                    response.response_headers,
                );
                for warning in warnings {
                    result = result.with_warning(warning);
                }
                result
            }
            Err(error) => {
                let mut result =
                    cohere_reranking_result_from_error(error, Some(request_body_for_error));
                for warning in warnings {
                    result = result.with_warning(warning);
                }
                result
            }
        }
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        combine_headers([
            Some(cohere_provider_header_entries(&self.settings)),
            optional_headers(call_headers),
        ])
    }
}

impl RerankingModel for CohereRerankingModel {
    type RerankFuture<'a>
        = Pin<Box<dyn Future<Output = RerankingModelResult> + Send + 'a>>
    where
        Self: 'a;

    fn provider(&self) -> &str {
        CohereRerankingModel::provider(self)
    }

    fn model_id(&self) -> &str {
        CohereRerankingModel::model_id(self)
    }

    fn do_rerank(&self, options: RerankingModelCallOptions) -> Self::RerankFuture<'_> {
        Box::pin(self.do_rerank_result(options))
    }
}

/// Creates a Cohere provider with explicit settings.
pub fn create_cohere(settings: CohereProviderSettings) -> CohereProvider {
    CohereProvider::from_settings(settings)
}

/// Creates a Cohere chat model with default provider settings.
///
/// Cohere chat is explicitly inventoried for a later provider-specific port, so this
/// helper currently returns the same typed unsupported-model error as the provider.
pub fn cohere(
    model_id: impl Into<String>,
) -> Result<OpenAICompatibleChatLanguageModel, NoSuchModelError> {
    CohereProvider::new().language_model(model_id)
}

fn cohere_base_url(settings: &CohereProviderSettings) -> String {
    let base_url = non_empty_optional_setting(settings.base_url.clone())
        .unwrap_or_else(|| DEFAULT_COHERE_BASE_URL.to_string());

    without_trailing_slash(Some(&base_url))
        .unwrap_or(&base_url)
        .to_string()
}

fn cohere_api_key(explicit_api_key: Option<&String>) -> Option<String> {
    non_empty_optional_setting(explicit_api_key.cloned())
        .or_else(|| non_empty_optional_setting(env::var("COHERE_API_KEY").ok()))
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn cohere_provider_headers(settings: &CohereProviderSettings) -> Headers {
    let mut headers = Headers::new();

    if let Some(api_key) = cohere_api_key(settings.api_key.as_ref()) {
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
        [format!("ai-sdk/cohere/{}", crate::VERSION)],
    )
}

fn cohere_provider_header_entries(
    settings: &CohereProviderSettings,
) -> Vec<(String, Option<String>)> {
    cohere_provider_headers(settings)
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

fn cohere_embedding_request_body(model_id: &str, options: &EmbeddingModelCallOptions) -> JsonValue {
    let mut body = JsonObject::new();
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    body.insert("embedding_types".to_string(), json!(["float"]));
    body.insert("texts".to_string(), json!(options.values));
    body.insert(
        "input_type".to_string(),
        cohere_option_string(options, "inputType")
            .map(JsonValue::String)
            .unwrap_or_else(|| JsonValue::String("search_query".to_string())),
    );

    if let Some(truncate) = cohere_option_string(options, "truncate") {
        body.insert("truncate".to_string(), JsonValue::String(truncate));
    }

    if let Some(output_dimension) = cohere_option_value(options, "outputDimension") {
        body.insert("output_dimension".to_string(), output_dimension);
    }

    JsonValue::Object(body)
}

fn cohere_reranking_request_body(
    model_id: &str,
    options: &RerankingModelCallOptions,
) -> (JsonValue, Vec<Warning>) {
    let mut body = JsonObject::new();
    let mut warnings = Vec::new();
    body.insert("model".to_string(), JsonValue::String(model_id.to_string()));
    body.insert(
        "query".to_string(),
        JsonValue::String(options.query.clone()),
    );
    body.insert(
        "documents".to_string(),
        cohere_reranking_documents(&options.documents, &mut warnings),
    );

    if let Some(top_n) = options.top_n {
        body.insert("top_n".to_string(), JsonValue::from(top_n));
    }

    if let Some(max_tokens_per_doc) = cohere_reranking_option_value(options, "maxTokensPerDoc") {
        body.insert("max_tokens_per_doc".to_string(), max_tokens_per_doc);
    }

    if let Some(priority) = cohere_reranking_option_value(options, "priority") {
        body.insert("priority".to_string(), priority);
    }

    (JsonValue::Object(body), warnings)
}

fn cohere_reranking_documents(
    documents: &RerankingModelDocuments,
    warnings: &mut Vec<Warning>,
) -> JsonValue {
    match documents {
        RerankingModelDocuments::Text { values } => json!(values),
        RerankingModelDocuments::Object { values } => {
            warnings.push(Warning::Compatibility {
                feature: "object documents".to_string(),
                details: Some("Object documents are converted to strings.".to_string()),
            });
            JsonValue::Array(
                values
                    .iter()
                    .map(|value| JsonValue::String(JsonValue::Object(value.clone()).to_string()))
                    .collect(),
            )
        }
    }
}

fn cohere_option_string(options: &EmbeddingModelCallOptions, name: &str) -> Option<String> {
    cohere_option_value(options, name).and_then(|value| value.as_str().map(str::to_string))
}

fn cohere_option_value(options: &EmbeddingModelCallOptions, name: &str) -> Option<JsonValue> {
    options
        .provider_options
        .as_ref()
        .and_then(|provider_options| provider_options.get("cohere"))
        .and_then(|options| options.get(name))
        .cloned()
}

fn cohere_reranking_option_value(
    options: &RerankingModelCallOptions,
    name: &str,
) -> Option<JsonValue> {
    options
        .provider_options
        .as_ref()
        .and_then(|provider_options| provider_options.get("cohere"))
        .and_then(|options| options.get(name))
        .cloned()
}

#[derive(Clone, Debug, Deserialize)]
struct CohereEmbeddingResponse {
    embeddings: CohereEmbeddingVectors,
    meta: CohereEmbeddingMeta,
}

#[derive(Clone, Debug, Deserialize)]
struct CohereEmbeddingVectors {
    float: Vec<Vec<f64>>,
}

#[derive(Clone, Debug, Deserialize)]
struct CohereEmbeddingMeta {
    billed_units: CohereEmbeddingBilledUnits,
}

#[derive(Clone, Debug, Deserialize)]
struct CohereEmbeddingBilledUnits {
    input_tokens: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct CohereRerankingResponse {
    #[serde(default)]
    id: Option<String>,
    results: Vec<CohereRerankingResult>,
}

#[derive(Clone, Debug, Deserialize)]
struct CohereRerankingResult {
    index: usize,
    relevance_score: f64,
}

fn cohere_embedding_response(
    value: &JsonValue,
) -> Result<CohereEmbeddingResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn cohere_reranking_response(
    value: &JsonValue,
) -> Result<CohereRerankingResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn cohere_error_response(value: &JsonValue) -> Result<JsonValue, serde_json::Error> {
    Ok(value.clone())
}

fn cohere_error_message(value: &JsonValue) -> String {
    value
        .get("message")
        .or_else(|| value.get("error").and_then(|error| error.get("message")))
        .and_then(JsonValue::as_str)
        .unwrap_or("Unknown error")
        .to_string()
}

fn cohere_embedding_result_from_response(
    response: CohereEmbeddingResponse,
    raw_response: Option<JsonValue>,
    response_headers: Option<Headers>,
) -> EmbeddingModelResult {
    let mut result = EmbeddingModelResult::new(response.embeddings.float).with_usage(
        EmbeddingModelUsage::new(response.meta.billed_units.input_tokens),
    );
    let mut response_metadata = EmbeddingModelResponse::new();

    if let Some(body) = raw_response {
        response_metadata = response_metadata.with_body(body);
    }

    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response_metadata = response_metadata.with_header(name, value);
        }
    }

    result = result.with_response(response_metadata);
    result
}

fn cohere_embedding_result_from_error(
    error: HandledFetchError,
    request_body: Option<JsonValue>,
) -> EmbeddingModelResult {
    let (message, headers, body) = cohere_error_parts(error);
    let response_body = body
        .as_deref()
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
        .or_else(|| body.map(JsonValue::String))
        .or(request_body);
    let mut response = EmbeddingModelResponse::new();

    if let Some(body) = response_body {
        response = response.with_body(body);
    }

    if let Some(headers) = headers {
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    EmbeddingModelResult::new(Vec::new())
        .with_provider_metadata(cohere_error_metadata(message))
        .with_response(response)
}

fn cohere_reranking_result_from_response(
    response: CohereRerankingResponse,
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

    if let Some(body) = raw_response {
        response_metadata = response_metadata.with_body(body);
    }

    if let Some(headers) = response_headers {
        for (name, value) in headers {
            response_metadata = response_metadata.with_header(name, value);
        }
    }

    result = result.with_response(response_metadata);
    result
}

fn cohere_reranking_result_from_error(
    error: HandledFetchError,
    request_body: Option<JsonValue>,
) -> RerankingModelResult {
    let (message, headers, body) = cohere_error_parts(error);
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
        for (name, value) in headers {
            response = response.with_header(name, value);
        }
    }

    RerankingModelResult::new(Vec::new())
        .with_provider_metadata(cohere_error_metadata(message))
        .with_response(response)
}

fn cohere_error_parts(error: HandledFetchError) -> (String, Option<Headers>, Option<String>) {
    match error {
        HandledFetchError::Original { error } => (error.message().to_string(), None, None),
        HandledFetchError::ApiCall { error } => (
            error.message().to_string(),
            error.response_headers().cloned(),
            error.response_body().map(String::from),
        ),
    }
}

fn cohere_error_metadata(message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut extra = JsonObject::new();
    extra.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert("cohere".to_string(), extra);
    metadata
}

fn default_cohere_transport() -> OpenAICompatibleTransport {
    Arc::new(|request| Box::pin(ready(execute_cohere_request(request))))
}

fn execute_cohere_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_cohere_get_request(request),
        ProviderApiRequestMethod::Post => execute_cohere_post_request(request),
    }
}

fn execute_cohere_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    cohere_provider_api_response(response)
}

fn execute_cohere_post_request(
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
                "multipart form data is not supported by the Cohere transport",
            ));
        }
        None => builder.send_empty(),
    };

    cohere_provider_api_response(response)
}

fn cohere_provider_api_response(
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
        CohereProvider, CohereProviderSettings, DEFAULT_COHERE_BASE_URL, cohere, create_cohere,
    };
    use crate::embedding_model::{EmbeddingModel, EmbeddingModelCallOptions};
    use crate::headers::Headers;
    use crate::json::{JsonObject, JsonValue};
    use crate::openai_compatible::{OpenAICompatibleTransport, OpenAICompatibleTransportFuture};
    use crate::provider::{ModelType, Provider, ProviderOptions, ProviderWithRerankingModel};
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    };
    use crate::reranking_model::{
        RerankingModel, RerankingModelCallOptions, RerankingModelDocuments,
    };
    use serde_json::json;
    use std::future::{Future, ready};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Wake, Waker};

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

    #[test]
    fn cohere_provider_creates_embedding_model_with_options_headers_and_usage() {
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
                        "embeddings": {
                            "float": [
                                [0.1, 0.2],
                                [0.3, 0.4]
                            ]
                        },
                        "meta": {
                            "billed_units": {
                                "input_tokens": 7
                            }
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_cohere_embedding".to_string(),
                )])))))
            });
        let provider = create_cohere(
            CohereProviderSettings::new()
                .with_api_key("test-api-key")
                .with_base_url("https://api.cohere.test/v2/")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "cohere": {
                "inputType": "classification",
                "truncate": "END",
                "outputDimension": 512
            }
        }))
        .expect("provider options deserialize");
        let model = provider.embedding_model("embed-v4.0");
        let result = poll_ready(
            model.do_embed(
                EmbeddingModelCallOptions::new(vec!["sunny".to_string(), "rainy".to_string()])
                    .with_provider_options(provider_options),
            ),
        );

        assert_eq!(model.provider(), "cohere.textEmbedding");
        assert_eq!(result.embeddings, vec![vec![0.1, 0.2], vec![0.3, 0.4]]);
        assert_eq!(result.usage.as_ref().map(|usage| usage.tokens), Some(7));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.cohere.test/v2/embed");
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
                .iter()
                .any(|(name, value)| name.eq_ignore_ascii_case("user-agent")
                    && value.contains("ai-sdk/cohere/0.1.0")),
            "headers: {:?}",
            request.headers
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "embed-v4.0",
                "embedding_types": ["float"],
                "texts": ["sunny", "rainy"],
                "input_type": "classification",
                "truncate": "END",
                "output_dimension": 512
            }))
        );
    }

    #[test]
    fn cohere_embedding_model_uses_default_input_type_and_exposes_raw_response() {
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
                        "embeddings": {
                            "float": [[0.1, 0.2]]
                        },
                        "meta": {
                            "billed_units": {
                                "input_tokens": 2
                            }
                        }
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_cohere_embedding_default".to_string(),
                )])))))
            });
        let model = CohereProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.cohere.test/v2/")
            .with_transport(transport)
            .embedding_model("embed-v4.0");
        let result =
            poll_ready(model.do_embed(EmbeddingModelCallOptions::new(vec!["sunny".to_string()])));

        assert_eq!(result.embeddings, vec![vec![0.1, 0.2]]);
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_cohere_embedding_default")
        );
        assert!(
            result
                .response
                .as_ref()
                .and_then(|response| response.body.as_ref())
                .and_then(|body| body.get("meta"))
                .is_some()
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "embed-v4.0",
                "embedding_types": ["float"],
                "texts": ["sunny"],
                "input_type": "search_query"
            }))
        );
    }

    #[test]
    fn cohere_provider_creates_reranking_model_with_object_warning_and_options() {
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
                        "id": "rerank-cohere",
                        "results": [
                            { "index": 1, "relevance_score": 0.91 },
                            { "index": 0, "relevance_score": 0.82 }
                        ]
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_cohere_rerank".to_string(),
                )])))))
            });
        let provider = CohereProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.cohere.test/v2/")
            .with_transport(transport);
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "cohere": {
                "maxTokensPerDoc": 256,
                "priority": 1
            }
        }))
        .expect("provider options deserialize");
        let first = json!({ "title": "A", "body": "first" })
            .as_object()
            .cloned()
            .expect("object");
        let second = json!({ "title": "B", "body": "second" })
            .as_object()
            .cloned()
            .expect("object");
        let model = provider.reranking_model("rerank-v3.5");
        let result = poll_ready(
            model.do_rerank(
                RerankingModelCallOptions::new(
                    RerankingModelDocuments::Object {
                        values: vec![first, second],
                    },
                    "query",
                )
                .with_top_n(2)
                .with_provider_options(provider_options),
            ),
        );

        assert_eq!(model.provider(), "cohere.reranking");
        assert_eq!(result.ranking[0].index, 1);
        assert!(result.warnings.iter().any(|warning| {
            matches!(warning, crate::warning::Warning::Compatibility { feature, .. } if feature == "object documents")
        }));
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.id.as_deref()),
            Some("rerank-cohere")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.url, "https://api.cohere.test/v2/rerank");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "rerank-v3.5",
                "query": "query",
                "documents": [
                    JsonValue::Object(JsonObject::from_iter([
                        ("body".to_string(), JsonValue::String("first".to_string())),
                        ("title".to_string(), JsonValue::String("A".to_string())),
                    ])).to_string(),
                    JsonValue::Object(JsonObject::from_iter([
                        ("body".to_string(), JsonValue::String("second".to_string())),
                        ("title".to_string(), JsonValue::String("B".to_string())),
                    ])).to_string()
                ],
                "top_n": 2,
                "max_tokens_per_doc": 256,
                "priority": 1
            }))
        );
    }

    #[test]
    fn cohere_reranking_model_sends_text_documents_without_warnings() {
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
                        "id": "rerank-cohere-text",
                        "results": [
                            { "index": 1, "relevance_score": 0.91 },
                            { "index": 0, "relevance_score": 0.82 }
                        ]
                    })
                    .to_string(),
                )
                .with_headers(Headers::from([(
                    "x-request-id".to_string(),
                    "req_cohere_rerank_text".to_string(),
                )])))))
            });
        let model = CohereProvider::new()
            .with_api_key("test-api-key")
            .with_base_url("https://api.cohere.test/v2/")
            .with_transport(transport)
            .reranking_model("rerank-v3.5");
        let result = poll_ready(
            model.do_rerank(
                RerankingModelCallOptions::new(
                    RerankingModelDocuments::Text {
                        values: vec!["sunny day".to_string(), "rainy city".to_string()],
                    },
                    "rainy",
                )
                .with_top_n(2),
            ),
        );

        assert!(result.warnings.is_empty());
        assert!(result.provider_metadata.is_none());
        assert_eq!(result.ranking[0].index, 1);
        assert_eq!(
            result
                .response
                .as_ref()
                .and_then(|response| response.headers.as_ref())
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_cohere_rerank_text")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok()),
            Some(json!({
                "model": "rerank-v3.5",
                "query": "rainy",
                "documents": ["sunny day", "rainy city"],
                "top_n": 2
            }))
        );
    }

    #[test]
    fn cohere_provider_reports_chat_and_image_exceptions() {
        let provider = CohereProvider::new();

        let chat_error = cohere("command-r")
            .err()
            .expect("chat is explicitly not ported in this slice");
        let image_error = Provider::image_model(&provider, "image")
            .err()
            .expect("image models are unsupported");

        assert_eq!(chat_error.model_type(), ModelType::LanguageModel);
        assert_eq!(image_error.model_type(), ModelType::ImageModel);
        assert_eq!(DEFAULT_COHERE_BASE_URL, "https://api.cohere.com/v2");
    }

    #[test]
    fn cohere_provider_implements_embedding_and_reranking_traits() {
        let provider = CohereProvider::new();
        let embedding = Provider::embedding_model(&provider, "embed-v4.0").expect("embedding");
        let reranking = ProviderWithRerankingModel::reranking_model(&provider, "rerank-v3.5")
            .expect("reranking");

        assert_eq!(embedding.provider(), "cohere.textEmbedding");
        assert_eq!(reranking.provider(), "cohere.reranking");
    }

    #[test]
    fn cohere_provider_settings_serde_accepts_upstream_base_url() {
        let settings: CohereProviderSettings = serde_json::from_value(json!({
            "baseURL": "https://api.cohere.test/v2/",
            "apiKey": "key",
            "headers": {
                "x-provider": "cohere"
            }
        }))
        .expect("settings deserialize");

        assert_eq!(
            settings,
            CohereProviderSettings::new()
                .with_base_url("https://api.cohere.test/v2/")
                .with_api_key("key")
                .with_header("x-provider", "cohere")
        );
    }
}
