use std::collections::BTreeMap;
use std::env;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use url::Url;
use url::form_urlencoded::Serializer as FormUrlEncodedSerializer;

use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{
    FinishReason, InputTokenUsage, LanguageModel, LanguageModelCallOptions, LanguageModelContent,
    LanguageModelCustomContent, LanguageModelErrorStreamPart, LanguageModelFinishReason,
    LanguageModelGenerateResult, LanguageModelRequest, LanguageModelResponse,
    LanguageModelStreamPart, LanguageModelStreamResult, LanguageModelStreamResultResponse,
    LanguageModelSupportedUrls, LanguageModelText, LanguageModelUsage, OutputTokenUsage,
};
use crate::provider::{ProviderMetadata, SpecificationVersion};
use crate::provider_utils::{
    FetchErrorInfo, GetFromApiOptions, HandledFetchError, ParseJsonResult, PostJsonToApiOptions,
    ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, RuntimeEnvironment, combine_headers,
    create_event_source_response_handler, create_json_error_response_handler,
    create_json_response_handler, get_from_api, post_json_to_api, with_user_agent_suffix,
    without_trailing_slash,
};

/// Default base URL used by upstream `@ai-sdk/gateway` provider calls.
pub const DEFAULT_GATEWAY_BASE_URL: &str = "https://ai-gateway.vercel.sh/v4/ai";

const AI_GATEWAY_PROTOCOL_VERSION: &str = "0.0.1";
const GATEWAY_AUTH_METHOD_HEADER: &str = "ai-gateway-auth-method";
const GATEWAY_PROVIDER_ID: &str = "gateway";

/// Future returned by an injected Gateway HTTP transport.
pub type GatewayTransportFuture =
    Pin<Box<dyn Future<Output = Result<ProviderApiResponse, FetchErrorInfo>> + Send>>;

/// HTTP transport used by [`GatewayLanguageModel`].
pub type GatewayTransport = Arc<dyn Fn(ProviderApiRequest) -> GatewayTransportFuture + Send + Sync>;

/// Known Gateway model categories returned by metadata discovery.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GatewayModelType {
    /// Text generation language model.
    Language,

    /// Text embedding model.
    Embedding,

    /// Image generation model.
    Image,

    /// Document reranking model.
    Reranking,

    /// Video generation model.
    Video,
}

impl GatewayModelType {
    fn from_gateway_value(value: &str) -> Option<Self> {
        match value {
            "language" => Some(Self::Language),
            "embedding" => Some(Self::Embedding),
            "image" => Some(Self::Image),
            "reranking" => Some(Self::Reranking),
            "video" => Some(Self::Video),
            _ => None,
        }
    }
}

/// Per-token price data returned by Gateway model metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayLanguageModelPricing {
    /// Cost per input token in USD.
    pub input: String,

    /// Cost per output token in USD.
    pub output: String,

    /// Cost per cached input token in USD.
    #[serde(
        default,
        alias = "input_cache_read",
        skip_serializing_if = "Option::is_none"
    )]
    pub cached_input_tokens: Option<String>,

    /// Cost per input token to create/write cache entries in USD.
    #[serde(
        default,
        alias = "input_cache_write",
        skip_serializing_if = "Option::is_none"
    )]
    pub cache_creation_input_tokens: Option<String>,
}

/// Provider-v4 language model specification advertised by Gateway metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayLanguageModelSpecification {
    /// Provider interface version used by the advertised model.
    pub specification_version: SpecificationVersion,

    /// Provider id for the advertised model.
    pub provider: String,

    /// Provider-specific model id.
    pub model_id: String,
}

/// A language model entry returned by Gateway metadata discovery.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayLanguageModelEntry {
    /// The model id used with the Gateway provider.
    pub id: String,

    /// Display name for user-facing model lists.
    pub name: String,

    /// Optional model description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Optional model pricing information.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing: Option<GatewayLanguageModelPricing>,

    /// Provider-v4 specification for this model.
    pub specification: GatewayLanguageModelSpecification,

    /// Optional Gateway model category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_type: Option<GatewayModelType>,
}

/// Available Gateway models returned by metadata discovery.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayFetchMetadataResponse {
    /// Models available to the authenticated Gateway account.
    pub models: Vec<GatewayLanguageModelEntry>,
}

/// Gateway credit balance information for the authenticated account.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayCreditsResponse {
    /// Remaining credit balance available for Gateway API usage.
    pub balance: String,

    /// Total amount of Gateway credits consumed.
    #[serde(rename = "totalUsed", alias = "total_used")]
    pub total_used: String,
}

/// Spend report aggregation dimension.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewaySpendReportGroupBy {
    /// Aggregate by day.
    Day,

    /// Aggregate by user.
    User,

    /// Aggregate by model.
    Model,

    /// Aggregate by tag.
    Tag,

    /// Aggregate by provider.
    Provider,

    /// Aggregate by credential type.
    CredentialType,
}

impl GatewaySpendReportGroupBy {
    const fn as_query_value(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::User => "user",
            Self::Model => "model",
            Self::Tag => "tag",
            Self::Provider => "provider",
            Self::CredentialType => "credential_type",
        }
    }
}

/// Spend report time granularity when grouping by day.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GatewaySpendReportDatePart {
    /// Daily report rows.
    Day,

    /// Hourly report rows.
    Hour,
}

impl GatewaySpendReportDatePart {
    const fn as_query_value(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Hour => "hour",
        }
    }
}

/// Gateway credential source used in spend report filters and rows.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GatewayCredentialType {
    /// Bring-your-own-key credentials.
    Byok,

    /// Gateway-managed system credentials.
    System,
}

impl GatewayCredentialType {
    const fn as_query_value(self) -> &'static str {
        match self {
            Self::Byok => "byok",
            Self::System => "system",
        }
    }
}

/// Parameters for a Gateway spend report request.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySpendReportParams {
    /// Start date in `YYYY-MM-DD` format, inclusive.
    pub start_date: String,

    /// End date in `YYYY-MM-DD` format, inclusive.
    pub end_date: String,

    /// Primary aggregation dimension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<GatewaySpendReportGroupBy>,

    /// Time granularity when grouping by day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_part: Option<GatewaySpendReportDatePart>,

    /// Filter to a specific user's spend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Filter to a specific model id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Filter to a specific provider id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,

    /// Filter to BYOK or system credentials.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_type: Option<GatewayCredentialType>,

    /// Filter to requests with these tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

impl GatewaySpendReportParams {
    /// Creates spend report parameters with the required date range.
    pub fn new(start_date: impl Into<String>, end_date: impl Into<String>) -> Self {
        Self {
            start_date: start_date.into(),
            end_date: end_date.into(),
            ..Self::default()
        }
    }

    /// Sets the primary aggregation dimension.
    pub fn with_group_by(mut self, group_by: GatewaySpendReportGroupBy) -> Self {
        self.group_by = Some(group_by);
        self
    }

    /// Sets the time granularity for day grouping.
    pub fn with_date_part(mut self, date_part: GatewaySpendReportDatePart) -> Self {
        self.date_part = Some(date_part);
        self
    }

    /// Filters the report to a user id.
    pub fn with_user_id(mut self, user_id: impl Into<String>) -> Self {
        self.user_id = Some(user_id.into());
        self
    }

    /// Filters the report to a model id.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Filters the report to a provider id.
    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Filters the report to a credential type.
    pub fn with_credential_type(mut self, credential_type: GatewayCredentialType) -> Self {
        self.credential_type = Some(credential_type);
        self
    }

    /// Filters the report to the supplied tags.
    pub fn with_tags<I, S>(mut self, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }
}

/// One row returned by the Gateway spend report API.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySpendReportRow {
    /// Date string when grouping by day.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<String>,

    /// Hour timestamp when grouping by day and hour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hour: Option<String>,

    /// User identifier when grouping by user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    /// Model identifier when grouping by model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Tag value when grouping by tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,

    /// Provider id when grouping by provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,

    /// Credential type when grouping by credential type.
    #[serde(
        default,
        rename = "credentialType",
        alias = "credential_type",
        skip_serializing_if = "Option::is_none"
    )]
    pub credential_type: Option<GatewayCredentialType>,

    /// Total cost in USD.
    #[serde(rename = "totalCost", alias = "total_cost")]
    pub total_cost: f64,

    /// Market cost in USD.
    #[serde(
        default,
        rename = "marketCost",
        alias = "market_cost",
        skip_serializing_if = "Option::is_none"
    )]
    pub market_cost: Option<f64>,

    /// Number of input tokens.
    #[serde(
        default,
        rename = "inputTokens",
        alias = "input_tokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub input_tokens: Option<u64>,

    /// Number of output tokens.
    #[serde(
        default,
        rename = "outputTokens",
        alias = "output_tokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub output_tokens: Option<u64>,

    /// Number of cached input tokens.
    #[serde(
        default,
        rename = "cachedInputTokens",
        alias = "cached_input_tokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub cached_input_tokens: Option<u64>,

    /// Number of cache creation input tokens.
    #[serde(
        default,
        rename = "cacheCreationInputTokens",
        alias = "cache_creation_input_tokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub cache_creation_input_tokens: Option<u64>,

    /// Number of reasoning tokens.
    #[serde(
        default,
        rename = "reasoningTokens",
        alias = "reasoning_tokens",
        skip_serializing_if = "Option::is_none"
    )]
    pub reasoning_tokens: Option<u64>,

    /// Number of requests.
    #[serde(
        default,
        rename = "requestCount",
        alias = "request_count",
        skip_serializing_if = "Option::is_none"
    )]
    pub request_count: Option<u64>,
}

/// Gateway spend report response.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySpendReportResponse {
    /// Report rows.
    pub results: Vec<GatewaySpendReportRow>,
}

/// Parameters for a Gateway generation info lookup.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayGenerationInfoParams {
    /// The generation id to look up.
    pub id: String,
}

impl GatewayGenerationInfoParams {
    /// Creates generation info lookup parameters.
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

/// Detailed information about a specific Gateway generation.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayGenerationInfo {
    /// The generation id.
    pub id: String,

    /// Total cost in USD.
    #[serde(rename = "totalCost", alias = "total_cost")]
    pub total_cost: f64,

    /// Upstream inference cost in USD.
    #[serde(rename = "upstreamInferenceCost", alias = "upstream_inference_cost")]
    pub upstream_inference_cost: f64,

    /// Usage cost in USD.
    pub usage: f64,

    /// ISO 8601 timestamp when the generation was created.
    #[serde(rename = "createdAt", alias = "created_at")]
    pub created_at: String,

    /// Model identifier.
    pub model: String,

    /// Whether BYOK credentials were used.
    #[serde(rename = "isByok", alias = "is_byok")]
    pub is_byok: bool,

    /// Provider that served this generation.
    #[serde(rename = "providerName", alias = "provider_name")]
    pub provider_name: String,

    /// Whether streaming was used.
    pub streamed: bool,

    /// Provider finish reason.
    #[serde(rename = "finishReason", alias = "finish_reason")]
    pub finish_reason: String,

    /// Time to first token in milliseconds.
    pub latency: u64,

    /// Total generation time in milliseconds.
    #[serde(rename = "generationTime", alias = "generation_time")]
    pub generation_time: u64,

    /// Number of prompt tokens.
    #[serde(rename = "promptTokens", alias = "native_tokens_prompt")]
    pub prompt_tokens: u64,

    /// Number of completion tokens.
    #[serde(rename = "completionTokens", alias = "native_tokens_completion")]
    pub completion_tokens: u64,

    /// Reasoning tokens used.
    #[serde(rename = "reasoningTokens", alias = "native_tokens_reasoning")]
    pub reasoning_tokens: u64,

    /// Cached tokens used.
    #[serde(rename = "cachedTokens", alias = "native_tokens_cached")]
    pub cached_tokens: u64,

    /// Cache creation input tokens.
    #[serde(rename = "cacheCreationTokens", alias = "native_tokens_cache_creation")]
    pub cache_creation_tokens: u64,

    /// Billable web search calls.
    #[serde(rename = "billableWebSearchCalls", alias = "billable_web_search_calls")]
    pub billable_web_search_calls: u64,
}

/// Configuration for a Vercel AI Gateway provider instance.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayProviderSettings {
    /// Base URL prefix for native AI SDK Gateway API calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,

    /// AI Gateway API key. When omitted, `AI_GATEWAY_API_KEY` and then
    /// `AI_SDK_RUST_AI_GATEWAY_API_KEY` are read at call time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Custom provider-level headers included with each request.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl GatewayProviderSettings {
    /// Creates empty Gateway provider settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the native AI SDK Gateway base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    /// Sets the AI Gateway API key.
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

/// Vercel AI Gateway provider.
#[derive(Clone)]
pub struct GatewayProvider {
    settings: GatewayProviderSettings,
    transport: GatewayTransport,
}

impl GatewayProvider {
    /// Creates a Gateway provider with default settings.
    pub fn new() -> Self {
        Self::from_settings(GatewayProviderSettings::new())
    }

    /// Creates a Gateway provider with explicit settings.
    pub fn from_settings(settings: GatewayProviderSettings) -> Self {
        Self {
            settings,
            transport: default_gateway_transport(),
        }
    }

    /// Sets the AI Gateway API key for this provider.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.settings.api_key = Some(api_key.into());
        self
    }

    /// Sets the native AI SDK Gateway base URL for this provider.
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
    pub fn with_transport(mut self, transport: GatewayTransport) -> Self {
        self.transport = transport;
        self
    }

    /// Creates a Gateway language model.
    pub fn language_model(&self, model_id: impl Into<String>) -> GatewayLanguageModel {
        GatewayLanguageModel {
            model_id: model_id.into(),
            settings: self.settings.clone(),
            transport: Arc::clone(&self.transport),
        }
    }

    /// Alias for [`GatewayProvider::language_model`].
    pub fn chat(&self, model_id: impl Into<String>) -> GatewayLanguageModel {
        self.language_model(model_id)
    }

    /// Returns available Gateway models for the authenticated account.
    pub async fn get_available_models(
        &self,
    ) -> Result<GatewayFetchMetadataResponse, HandledFetchError> {
        let request_headers = gateway_provider_headers(&self.settings);
        let get_options = GetFromApiOptions::new(format!("{}/config", self.base_url()))
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        get_gateway_json(get_options, transport, gateway_fetch_metadata_response).await
    }

    /// Returns credit balance information for the authenticated Gateway account.
    pub async fn get_credits(&self) -> Result<GatewayCreditsResponse, HandledFetchError> {
        let request_headers = gateway_provider_headers(&self.settings);
        let get_options =
            GetFromApiOptions::new(gateway_origin_url(&self.base_url(), "/v1/credits")?)
                .with_headers(request_headers)
                .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        get_gateway_json(get_options, transport, gateway_credits_response).await
    }

    /// Returns a Gateway spend report for the supplied date range and filters.
    pub async fn get_spend_report(
        &self,
        params: GatewaySpendReportParams,
    ) -> Result<GatewaySpendReportResponse, HandledFetchError> {
        let request_headers = gateway_provider_headers(&self.settings);
        let url = gateway_spend_report_url(&self.base_url(), &params)?;
        let get_options = GetFromApiOptions::new(url)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        get_gateway_json(get_options, transport, gateway_spend_report_response).await
    }

    /// Returns detailed information for a specific Gateway generation id.
    pub async fn get_generation_info(
        &self,
        params: GatewayGenerationInfoParams,
    ) -> Result<GatewayGenerationInfo, HandledFetchError> {
        let request_headers = gateway_provider_headers(&self.settings);
        let url = gateway_generation_info_url(&self.base_url(), &params)?;
        let get_options = GetFromApiOptions::new(url)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        get_gateway_json(get_options, transport, gateway_generation_info_response).await
    }

    fn base_url(&self) -> String {
        gateway_base_url(&self.settings)
    }
}

impl Default for GatewayProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Creates a Gateway provider with explicit settings.
pub fn create_gateway(settings: GatewayProviderSettings) -> GatewayProvider {
    GatewayProvider::from_settings(settings)
}

/// Creates a Gateway language model using the default provider settings.
pub fn gateway(model_id: impl Into<String>) -> GatewayLanguageModel {
    GatewayProvider::new().language_model(model_id)
}

/// Native AI SDK Gateway language model.
#[derive(Clone)]
pub struct GatewayLanguageModel {
    model_id: String,
    settings: GatewayProviderSettings,
    transport: GatewayTransport,
}

impl GatewayLanguageModel {
    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: GatewayTransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_generate_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelGenerateResult {
        let request_body = serde_json::to_value(&options).unwrap_or_else(|error| {
            json!({
                "serializationError": error.to_string()
            })
        });
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref(), false);
        let post_options = PostJsonToApiOptions::new(self.language_model_url(), request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    clone_json_value,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    clone_json_value,
                    gateway_error_to_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => self.generate_result_from_response(
                response.value,
                response.raw_value,
                response.response_headers,
                request_body_for_response,
            ),
            Err(error) => self.generate_result_from_error(error, request_body_for_error),
        }
    }

    async fn do_stream_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let include_raw_chunks = options.include_raw_chunks.unwrap_or(false);
        let request_body = serde_json::to_value(&options).unwrap_or_else(|error| {
            json!({
                "serializationError": error.to_string()
            })
        });
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref(), true);
        let post_options = PostJsonToApiOptions::new(self.language_model_url(), request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |_request, response| {
                create_event_source_response_handler(
                    response.event_source_response_handler_options(),
                    clone_json_value,
                )
                .map_err(|error| ProviderApiResponseHandlerError::other(error.to_string()))
            },
            |request, response| {
                Ok(create_json_error_response_handler(
                    response.json_error_response_handler_options(request),
                    clone_json_value,
                    gateway_error_to_message,
                    |_, _| None,
                ))
            },
        )
        .await
        {
            Ok(response) => self.stream_result_from_response(
                response.value,
                response.response_headers,
                request_body_for_response,
                include_raw_chunks,
            ),
            Err(error) => self.stream_result_from_error(error, request_body_for_error),
        }
    }

    fn language_model_url(&self) -> String {
        format!("{}/language-model", self.base_url())
    }

    fn base_url(&self) -> String {
        gateway_base_url(&self.settings)
    }

    fn request_headers(
        &self,
        call_headers: Option<&Headers>,
        streaming: bool,
    ) -> BTreeMap<String, Option<String>> {
        let provider_headers = self.provider_headers();
        let call_headers = optional_headers(call_headers);
        let model_headers = Some(vec![
            (
                "ai-language-model-specification-version".to_string(),
                Some("4".to_string()),
            ),
            (
                "ai-language-model-id".to_string(),
                Some(self.model_id.clone()),
            ),
            (
                "ai-language-model-streaming".to_string(),
                Some(streaming.to_string()),
            ),
        ]);

        combine_headers([provider_headers, call_headers, model_headers])
    }

    fn provider_headers(&self) -> Option<Vec<(String, Option<String>)>> {
        Some(
            gateway_provider_headers(&self.settings)
                .into_iter()
                .collect(),
        )
    }

    fn generate_result_from_response(
        &self,
        response: JsonValue,
        raw_response: Option<JsonValue>,
        response_headers: Option<Headers>,
        request_body: JsonValue,
    ) -> LanguageModelGenerateResult {
        let content = language_model_content(response.get("content"));
        let finish_reason = finish_reason(
            response
                .get("finish_reason")
                .or(response.get("finishReason")),
        );
        let usage = usage(response.get("usage"));
        let raw_body = raw_response.unwrap_or_else(|| response.clone());

        let mut result = LanguageModelGenerateResult::new(content, finish_reason, usage)
            .with_request(LanguageModelRequest::new().with_body(request_body));

        let mut response_metadata = LanguageModelResponse::new().with_body(raw_body);

        if let Some(id) = json_string(response.get("id")) {
            response_metadata = response_metadata.with_id(id);
        }

        if let Some(timestamp) = response_timestamp(response.get("created")) {
            response_metadata = response_metadata.with_timestamp(timestamp);
        }

        if let Some(model_id) = json_string(response.get("model").or(response.get("modelId"))) {
            response_metadata = response_metadata.with_model_id(model_id);
        }

        if let Some(headers) = response_headers {
            response_metadata = with_response_headers(response_metadata, headers);
        }

        if let Some(provider_metadata) = response
            .get("providerMetadata")
            .and_then(|value| serde_json::from_value::<ProviderMetadata>(value.clone()).ok())
        {
            result = result.with_provider_metadata(provider_metadata);
        }

        result.with_response(response_metadata)
    }

    fn generate_result_from_error(
        &self,
        error: HandledFetchError,
        request_body: JsonValue,
    ) -> LanguageModelGenerateResult {
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
            .or_else(|| body.map(JsonValue::String));
        let mut response = LanguageModelResponse::new();

        if let Some(headers) = headers {
            response = with_response_headers(response, headers);
        }

        if let Some(body) = response_body {
            response = response.with_body(body);
        }

        let mut result = LanguageModelGenerateResult::new(
            Vec::new(),
            LanguageModelFinishReason {
                unified: FinishReason::Error,
                raw: Some("gateway-error".to_string()),
            },
            LanguageModelUsage::default(),
        )
        .with_request(LanguageModelRequest::new().with_body(request_body))
        .with_response(response);

        result = result.with_provider_metadata(gateway_error_metadata(message));
        result
    }

    fn stream_result_from_response(
        &self,
        events: Vec<ParseJsonResult<JsonValue>>,
        response_headers: Option<Headers>,
        request_body: JsonValue,
        include_raw_chunks: bool,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let stream = events
            .into_iter()
            .filter_map(|event| stream_part_from_gateway_event(event, include_raw_chunks))
            .collect::<Vec<_>>();
        let mut result = LanguageModelStreamResult::new(stream)
            .with_request(LanguageModelRequest::new().with_body(request_body));

        if let Some(headers) = response_headers {
            result = result.with_response(with_stream_response_headers(
                LanguageModelStreamResultResponse::new(),
                headers,
            ));
        }

        result
    }

    fn stream_result_from_error(
        &self,
        error: HandledFetchError,
        request_body: JsonValue,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let (message, headers, body) = match error {
            HandledFetchError::Original { error } => (error.message().to_string(), None, None),
            HandledFetchError::ApiCall { error } => (
                error.message().to_string(),
                error.response_headers().cloned(),
                error.response_body().map(String::from),
            ),
        };
        let mut result =
            LanguageModelStreamResult::new(vec![gateway_stream_error(message, body.as_deref())])
                .with_request(LanguageModelRequest::new().with_body(request_body));

        if let Some(headers) = headers {
            result = result.with_response(with_stream_response_headers(
                LanguageModelStreamResultResponse::new(),
                headers,
            ));
        }

        result
    }
}

impl LanguageModel for GatewayLanguageModel {
    type SupportedUrlsFuture<'a>
        = Ready<LanguageModelSupportedUrls>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelGenerateResult> + Send + 'a>>
    where
        Self: 'a;

    type Stream = Vec<LanguageModelStreamPart>;

    type StreamFuture<'a>
        = Pin<Box<dyn Future<Output = LanguageModelStreamResult<Self::Stream>> + Send + 'a>>
    where
        Self: 'a;

    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    fn provider(&self) -> &str {
        GATEWAY_PROVIDER_ID
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn supported_urls(&self) -> Self::SupportedUrlsFuture<'_> {
        ready(BTreeMap::from([(
            "*/*".to_string(),
            vec![".*".to_string()],
        )]))
    }

    fn do_generate(&self, options: LanguageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }

    fn do_stream(&self, options: LanguageModelCallOptions) -> Self::StreamFuture<'_> {
        Box::pin(self.do_stream_result(options))
    }
}

fn gateway_base_url(settings: &GatewayProviderSettings) -> String {
    without_trailing_slash(settings.base_url.as_deref())
        .unwrap_or(DEFAULT_GATEWAY_BASE_URL)
        .to_string()
}

fn resolve_gateway_api_key(settings: &GatewayProviderSettings) -> Option<String> {
    settings
        .api_key
        .clone()
        .or_else(|| env::var("AI_GATEWAY_API_KEY").ok())
        .or_else(|| env::var("AI_SDK_RUST_AI_GATEWAY_API_KEY").ok())
        .filter(|value| !value.trim().is_empty())
}

fn gateway_provider_headers(
    settings: &GatewayProviderSettings,
) -> BTreeMap<String, Option<String>> {
    let mut headers = BTreeMap::from([
        (
            "ai-gateway-protocol-version".to_string(),
            Some(AI_GATEWAY_PROTOCOL_VERSION.to_string()),
        ),
        (
            GATEWAY_AUTH_METHOD_HEADER.to_string(),
            Some("api-key".to_string()),
        ),
    ]);

    if let Some(api_key) = resolve_gateway_api_key(settings) {
        headers.insert(
            "Authorization".to_string(),
            Some(format!("Bearer {api_key}")),
        );
    }

    for (name, value) in &settings.headers {
        headers.insert(name.clone(), Some(value.clone()));
    }

    with_user_agent_suffix(
        Some(headers),
        [format!("ai-sdk/gateway/{}", crate::VERSION)],
    )
    .into_iter()
    .map(|(name, value)| (name, Some(value)))
    .collect()
}

fn gateway_origin_url(base_url: &str, path: &str) -> Result<String, HandledFetchError> {
    let url = Url::parse(base_url).map_err(|error| HandledFetchError::Original {
        error: FetchErrorInfo::new(format!("invalid Gateway base URL: {error}"))
            .with_name("TypeError"),
    })?;
    let mut origin = url.origin().ascii_serialization();
    origin.push_str(path);

    Ok(origin)
}

fn gateway_spend_report_url(
    base_url: &str,
    params: &GatewaySpendReportParams,
) -> Result<String, HandledFetchError> {
    let mut query = FormUrlEncodedSerializer::new(String::new());
    query.append_pair("start_date", &params.start_date);
    query.append_pair("end_date", &params.end_date);

    if let Some(group_by) = params.group_by {
        query.append_pair("group_by", group_by.as_query_value());
    }

    if let Some(date_part) = params.date_part {
        query.append_pair("date_part", date_part.as_query_value());
    }

    if let Some(user_id) = &params.user_id {
        query.append_pair("user_id", user_id);
    }

    if let Some(model) = &params.model {
        query.append_pair("model", model);
    }

    if let Some(provider) = &params.provider {
        query.append_pair("provider", provider);
    }

    if let Some(credential_type) = params.credential_type {
        query.append_pair("credential_type", credential_type.as_query_value());
    }

    if !params.tags.is_empty() {
        query.append_pair("tags", &params.tags.join(","));
    }

    Ok(format!(
        "{}?{}",
        gateway_origin_url(base_url, "/v1/report")?,
        query.finish()
    ))
}

fn gateway_generation_info_url(
    base_url: &str,
    params: &GatewayGenerationInfoParams,
) -> Result<String, HandledFetchError> {
    let mut query = FormUrlEncodedSerializer::new(String::new());
    query.append_pair("id", &params.id);

    Ok(format!(
        "{}?{}",
        gateway_origin_url(base_url, "/v1/generation")?,
        query.finish()
    ))
}

async fn get_gateway_json<T, V, E>(
    options: GetFromApiOptions,
    transport: GatewayTransport,
    validate: V,
) -> Result<T, HandledFetchError>
where
    V: FnOnce(&JsonValue) -> Result<T, E>,
    E: std::fmt::Display,
{
    get_from_api(
        options,
        move |request| (transport)(request),
        |request, response| {
            create_json_response_handler(response.json_response_handler_options(request), validate)
                .map_err(ProviderApiResponseHandlerError::from)
        },
        |request, response| {
            Ok(create_json_error_response_handler(
                response.json_error_response_handler_options(request),
                clone_json_value,
                gateway_error_to_message,
                |_, _| None,
            ))
        },
    )
    .await
    .map(|response| response.value)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawGatewayFetchMetadataResponse {
    models: Vec<RawGatewayLanguageModelEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawGatewayLanguageModelEntry {
    id: String,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    pricing: Option<GatewayLanguageModelPricing>,
    specification: GatewayLanguageModelSpecification,
    #[serde(default)]
    model_type: Option<String>,
}

impl<'de> Deserialize<'de> for GatewayFetchMetadataResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawGatewayFetchMetadataResponse::deserialize(deserializer)?;

        Ok(gateway_fetch_metadata_response_from_raw(raw))
    }
}

fn gateway_fetch_metadata_response(
    value: &JsonValue,
) -> Result<GatewayFetchMetadataResponse, serde_json::Error> {
    let raw = serde_json::from_value::<RawGatewayFetchMetadataResponse>(value.clone())?;
    Ok(gateway_fetch_metadata_response_from_raw(raw))
}

fn gateway_fetch_metadata_response_from_raw(
    raw: RawGatewayFetchMetadataResponse,
) -> GatewayFetchMetadataResponse {
    let models = raw
        .models
        .into_iter()
        .filter_map(|model| {
            let model_type = match model.model_type {
                Some(model_type) => Some(GatewayModelType::from_gateway_value(&model_type)?),
                None => None,
            };

            Some(GatewayLanguageModelEntry {
                id: model.id,
                name: model.name,
                description: model.description,
                pricing: model.pricing,
                specification: model.specification,
                model_type,
            })
        })
        .collect();

    GatewayFetchMetadataResponse { models }
}

fn gateway_credits_response(
    value: &JsonValue,
) -> Result<GatewayCreditsResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn gateway_spend_report_response(
    value: &JsonValue,
) -> Result<GatewaySpendReportResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

#[derive(Deserialize)]
struct RawGatewayGenerationInfoResponse {
    data: GatewayGenerationInfo,
}

fn gateway_generation_info_response(
    value: &JsonValue,
) -> Result<GatewayGenerationInfo, serde_json::Error> {
    serde_json::from_value::<RawGatewayGenerationInfoResponse>(value.clone())
        .map(|response| response.data)
}

fn optional_headers(headers: Option<&Headers>) -> Option<Vec<(String, Option<String>)>> {
    headers.map(|headers| {
        headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone())))
            .collect()
    })
}

fn clone_json_value(value: &JsonValue) -> Result<JsonValue, &'static str> {
    Ok(value.clone())
}

fn gateway_error_to_message(error: &JsonValue) -> String {
    error
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(JsonValue::as_str)
        .or_else(|| error.get("message").and_then(JsonValue::as_str))
        .map_or_else(|| error.to_string(), String::from)
}

fn language_model_content(content: Option<&JsonValue>) -> Vec<LanguageModelContent> {
    match content {
        Some(JsonValue::Array(parts)) => parts.iter().filter_map(content_part).collect(),
        Some(value) => content_part(value).into_iter().collect(),
        None => Vec::new(),
    }
}

fn content_part(value: &JsonValue) -> Option<LanguageModelContent> {
    if let Some(text) = value.as_str() {
        return Some(LanguageModelContent::Text(LanguageModelText::new(text)));
    }

    let object = value.as_object()?;
    let part_type = object.get("type").and_then(JsonValue::as_str)?;

    match part_type {
        "text" => json_string(object.get("text"))
            .map(LanguageModelText::new)
            .map(LanguageModelContent::Text),
        other => Some(LanguageModelContent::Custom(
            LanguageModelCustomContent::new(format!("gateway.{other}")),
        )),
    }
}

fn finish_reason(value: Option<&JsonValue>) -> LanguageModelFinishReason {
    let raw = json_string(value).unwrap_or_else(|| "unknown".to_string());
    let unified = match raw.as_str() {
        "stop" => FinishReason::Stop,
        "length" | "max_tokens" => FinishReason::Length,
        "content-filter" | "content_filter" => FinishReason::ContentFilter,
        "tool-calls" | "tool_calls" => FinishReason::ToolCalls,
        "error" => FinishReason::Error,
        _ => FinishReason::Other,
    };

    LanguageModelFinishReason {
        unified,
        raw: Some(raw),
    }
}

fn usage(value: Option<&JsonValue>) -> LanguageModelUsage {
    let Some(value) = value else {
        return LanguageModelUsage::default();
    };

    let input_total = json_u64(
        value
            .get("prompt_tokens")
            .or_else(|| value.get("promptTokens"))
            .or_else(|| value.get("input_tokens"))
            .or_else(|| value.get("inputTokens")),
    );
    let output_total = json_u64(
        value
            .get("completion_tokens")
            .or_else(|| value.get("completionTokens"))
            .or_else(|| value.get("output_tokens"))
            .or_else(|| value.get("outputTokens")),
    );
    let cache_read = json_u64(
        value
            .get("cached_prompt_tokens")
            .or_else(|| value.get("cachedPromptTokens"))
            .or_else(|| {
                value.get("input_tokens_details").and_then(|details| {
                    details
                        .get("cached_tokens")
                        .or_else(|| details.get("cachedTokens"))
                })
            }),
    );
    let raw = value.as_object().cloned();

    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: input_total,
            no_cache: input_total
                .zip(cache_read)
                .map(|(total, cached)| total.saturating_sub(cached)),
            cache_read,
            cache_write: None,
        },
        output_tokens: OutputTokenUsage {
            total: output_total,
            text: output_total,
            reasoning: json_u64(
                value
                    .get("reasoning_tokens")
                    .or_else(|| value.get("reasoningTokens")),
            ),
        },
        raw,
    }
}

fn json_string(value: Option<&JsonValue>) -> Option<String> {
    match value {
        Some(JsonValue::String(value)) => Some(value.clone()),
        Some(JsonValue::Number(value)) => Some(value.to_string()),
        _ => None,
    }
}

fn json_u64(value: Option<&JsonValue>) -> Option<u64> {
    match value {
        Some(JsonValue::Number(value)) => value.as_u64(),
        Some(JsonValue::String(value)) => value.parse::<u64>().ok(),
        _ => None,
    }
}

fn response_timestamp(value: Option<&JsonValue>) -> Option<OffsetDateTime> {
    match value {
        Some(JsonValue::Number(value)) => value
            .as_i64()
            .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok()),
        Some(JsonValue::String(value)) => value
            .parse::<i64>()
            .ok()
            .and_then(|seconds| OffsetDateTime::from_unix_timestamp(seconds).ok()),
        _ => None,
    }
}

fn gateway_error_metadata(message: String) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    let mut gateway = JsonObject::new();
    gateway.insert("errorMessage".to_string(), JsonValue::String(message));
    metadata.insert(GATEWAY_PROVIDER_ID.to_string(), gateway);
    metadata
}

fn stream_part_from_gateway_event(
    event: ParseJsonResult<JsonValue>,
    include_raw_chunks: bool,
) -> Option<LanguageModelStreamPart> {
    match event {
        ParseJsonResult::Success { value, raw_value } => {
            if is_raw_stream_part(&value) && !include_raw_chunks {
                return None;
            }

            match serde_json::from_value::<LanguageModelStreamPart>(value) {
                Ok(part) => Some(part),
                Err(error) => Some(gateway_stream_error(
                    error.to_string(),
                    Some(&raw_value.to_string()),
                )),
            }
        }
        ParseJsonResult::Failure { error, raw_value } => Some(gateway_stream_error(
            error.to_string(),
            raw_value.as_ref().map(JsonValue::to_string).as_deref(),
        )),
    }
}

fn is_raw_stream_part(value: &JsonValue) -> bool {
    value
        .get("type")
        .and_then(JsonValue::as_str)
        .is_some_and(|part_type| part_type == "raw")
}

fn gateway_stream_error(message: String, raw_body: Option<&str>) -> LanguageModelStreamPart {
    let mut error = JsonObject::new();
    error.insert("message".to_string(), JsonValue::String(message));

    if let Some(raw_body) = raw_body {
        error.insert("body".to_string(), JsonValue::String(raw_body.to_string()));
    }

    LanguageModelStreamPart::Error(LanguageModelErrorStreamPart::new(JsonValue::Object(error)))
}

fn with_response_headers(
    mut response: LanguageModelResponse,
    headers: Headers,
) -> LanguageModelResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }

    response
}

fn with_stream_response_headers(
    mut response: LanguageModelStreamResultResponse,
    headers: Headers,
) -> LanguageModelStreamResultResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }

    response
}

fn default_gateway_transport() -> GatewayTransport {
    Arc::new(|request| Box::pin(ready(execute_gateway_request(request))))
}

fn execute_gateway_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    match request.method {
        ProviderApiRequestMethod::Get => execute_gateway_get_request(request),
        ProviderApiRequestMethod::Post => execute_gateway_post_request(request),
    }
}

fn execute_gateway_get_request(
    request: ProviderApiRequest,
) -> Result<ProviderApiResponse, FetchErrorInfo> {
    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let response = builder.config().http_status_as_error(false).build().call();

    provider_api_response(response)
}

fn execute_gateway_post_request(
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
                "multipart form data is not supported by the Gateway transport",
            ));
        }
        None => builder.send_empty(),
    };

    provider_api_response(response)
}

fn provider_api_response(
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
        GatewayCredentialType, GatewayGenerationInfoParams, GatewayModelType, GatewayProvider,
        GatewayProviderSettings, GatewaySpendReportDatePart, GatewaySpendReportGroupBy,
        GatewaySpendReportParams, GatewayTransport, GatewayTransportFuture, gateway,
    };
    use crate::generate_text::{GenerateTextOptions, generate_text};
    use crate::headers::Headers;
    use crate::json::JsonValue;
    use crate::language_model::{
        FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelContent,
        LanguageModelStreamPart,
    };
    use crate::prompt::Prompt;
    use crate::provider_utils::{
        ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    };
    use crate::stream_text::{StreamTextOptions, stream_text};
    use serde_json::json;
    use std::env;
    use std::fs;
    use std::future::{Future, ready};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    #[test]
    fn gateway_model_generates_text_through_generate_text() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "id": "test-id",
                    "created": 1711115037,
                    "model": "openai/gpt-4.1-mini",
                    "content": {
                        "type": "text",
                        "text": "Hello from Gateway"
                    },
                    "finish_reason": "stop",
                    "usage": {
                        "prompt_tokens": 4,
                        "completion_tokens": 3
                    }
                })
                .to_string(),
            )
            .with_headers(Headers::from([(
                "x-request-id".to_string(),
                "req_gateway".to_string(),
            )])))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token")
                .with_header("x-provider", "provider-value"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(12)
                .with_temperature(0.0),
        ));

        assert_eq!(result.text, "Hello from Gateway");
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(4));
        assert_eq!(result.usage.output_tokens.total, Some(3));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.test.com/language-model");
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
                .and_then(|body| body.get("maxOutputTokens").cloned()),
            Some(json!(12))
        );
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-token")
        );
        assert_eq!(
            request
                .headers
                .get("ai-gateway-protocol-version")
                .map(String::as_str),
            Some("0.0.1")
        );
        assert_eq!(
            request
                .headers
                .get("ai-gateway-auth-method")
                .map(String::as_str),
            Some("api-key")
        );
        assert_eq!(
            request
                .headers
                .get("ai-language-model-specification-version")
                .map(String::as_str),
            Some("4")
        );
        assert_eq!(
            request
                .headers
                .get("ai-language-model-id")
                .map(String::as_str),
            Some("openai/gpt-4.1-mini")
        );
        assert_eq!(
            request
                .headers
                .get("ai-language-model-streaming")
                .map(String::as_str),
            Some("false")
        );
        assert_eq!(
            request.headers.get("x-provider").map(String::as_str),
            Some("provider-value")
        );
    }

    #[test]
    fn gateway_model_streams_text_through_stream_text() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                gateway_stream_body(),
            )
            .with_headers(Headers::from([(
                "content-type".to_string(),
                "text/event-stream".to_string(),
            )])))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(&model, Prompt::from_prompt("Say hello"))
                .expect("prompt is valid")
                .with_max_output_tokens(12)
                .with_temperature(0.0),
        ));

        assert_eq!(result.text, "Hello Gateway");
        assert_eq!(result.text_stream, vec!["Hello ", "Gateway"]);
        assert_eq!(result.finish_reason, FinishReason::Stop);
        assert_eq!(result.usage.input_tokens.total, Some(2));
        assert_eq!(result.usage.output_tokens.total, Some(3));
        assert!(result.errors.is_empty());

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(
            request
                .headers
                .get("ai-language-model-streaming")
                .map(String::as_str),
            Some("true")
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text)
                .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
                .and_then(|body| body.get("maxOutputTokens").cloned()),
            Some(json!(12))
        );
    }

    #[test]
    fn gateway_model_filters_raw_stream_parts_unless_requested() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                gateway_stream_body(),
            ))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");

        let without_raw = poll_ready(model.do_stream(LanguageModelCallOptions::new(Vec::new())));
        assert!(
            without_raw
                .stream
                .iter()
                .all(|part| !matches!(part, LanguageModelStreamPart::Raw(_)))
        );

        let with_raw = poll_ready(
            model
                .do_stream(LanguageModelCallOptions::new(Vec::new()).with_include_raw_chunks(true)),
        );
        assert!(
            with_raw
                .stream
                .iter()
                .any(|part| matches!(part, LanguageModelStreamPart::Raw(_)))
        );
    }

    #[test]
    fn gateway_model_maps_gateway_error_to_error_finish_reason() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                401,
                "Unauthorized",
                json!({
                    "error": {
                        "message": "Invalid API key",
                        "type": "authentication_error"
                    }
                })
                .to_string(),
            ))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");
        let result = poll_ready(model.do_generate(
            crate::language_model::LanguageModelCallOptions::new(Vec::new()),
        ));

        assert_eq!(result.content, Vec::<LanguageModelContent>::new());
        assert_eq!(result.finish_reason.unified, FinishReason::Error);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("Invalid API key")
        );
    }

    #[test]
    fn gateway_function_uses_default_gateway_provider() {
        let model = gateway("openai/gpt-4.1-mini");

        assert_eq!(model.provider(), "gateway");
        assert_eq!(model.model_id(), "openai/gpt-4.1-mini");
    }

    #[test]
    fn gateway_provider_fetches_available_models_metadata() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "models": [
                        {
                            "id": "openai/gpt-4.1-mini",
                            "name": "GPT 4.1 mini",
                            "description": "Small OpenAI model",
                            "pricing": {
                                "input": "0.0000004",
                                "output": "0.0000016",
                                "input_cache_read": "0.0000001",
                                "input_cache_write": "0.0000002"
                            },
                            "specification": {
                                "specificationVersion": "v4",
                                "provider": "gateway",
                                "modelId": "openai/gpt-4.1-mini"
                            },
                            "modelType": "language"
                        },
                        {
                            "id": "future/model",
                            "name": "Future Model",
                            "specification": {
                                "specificationVersion": "v4",
                                "provider": "gateway",
                                "modelId": "future/model"
                            },
                            "modelType": "future-model-family"
                        }
                    ]
                })
                .to_string(),
            ))))
        });
        let provider = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com/v4/ai")
                .with_api_key("test-token")
                .with_header("x-provider", "provider-value"),
        )
        .with_transport(transport);
        let result = poll_ready(provider.get_available_models()).expect("metadata fetch succeeds");

        assert_eq!(result.models.len(), 1);
        let model = &result.models[0];
        assert_eq!(model.id, "openai/gpt-4.1-mini");
        assert_eq!(model.model_type, Some(GatewayModelType::Language));
        assert_eq!(
            model
                .pricing
                .as_ref()
                .and_then(|pricing| pricing.cached_input_tokens.as_deref()),
            Some("0.0000001")
        );
        assert_eq!(
            model
                .pricing
                .as_ref()
                .and_then(|pricing| pricing.cache_creation_input_tokens.as_deref()),
            Some("0.0000002")
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.method, ProviderApiRequestMethod::Get);
        assert_eq!(request.url, "https://api.test.com/v4/ai/config");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-token")
        );
        assert_eq!(
            request
                .headers
                .get("ai-gateway-protocol-version")
                .map(String::as_str),
            Some("0.0.1")
        );
        assert_eq!(
            request
                .headers
                .get("ai-gateway-auth-method")
                .map(String::as_str),
            Some("api-key")
        );
        assert_eq!(
            request.headers.get("x-provider").map(String::as_str),
            Some("provider-value")
        );
    }

    #[test]
    fn gateway_provider_fetches_credits_from_gateway_origin() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "balance": "150.50",
                    "total_used": "75.25"
                })
                .to_string(),
            ))))
        });
        let provider = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com/v4/ai")
                .with_api_key("test-token"),
        )
        .with_transport(transport);
        let result = poll_ready(provider.get_credits()).expect("credits fetch succeeds");

        assert_eq!(result.balance, "150.50");
        assert_eq!(result.total_used, "75.25");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.method, ProviderApiRequestMethod::Get);
        assert_eq!(request.url, "https://api.test.com/v1/credits");
    }

    #[test]
    fn gateway_provider_metadata_surfaces_api_errors() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                429,
                "Too Many Requests",
                json!({
                    "error": {
                        "message": "Rate limit exceeded",
                        "type": "rate_limit_exceeded"
                    }
                })
                .to_string(),
            ))))
        });
        let provider = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com/v4/ai")
                .with_api_key("test-token"),
        )
        .with_transport(transport);
        let error = poll_ready(provider.get_available_models()).expect_err("request fails");
        let api_error = error
            .api_call_error()
            .expect("gateway metadata errors are API call errors");

        assert_eq!(api_error.status_code(), Some(429));
        assert_eq!(api_error.message(), "Rate limit exceeded");
        assert!(api_error.is_retryable());
    }

    #[test]
    fn gateway_provider_fetches_spend_report_with_query_params() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "results": [
                        {
                            "day": "2026-03-01",
                            "credential_type": "byok",
                            "total_cost": 12.5,
                            "market_cost": 11.0,
                            "input_tokens": 50000,
                            "output_tokens": 10000,
                            "cached_input_tokens": 5000,
                            "cache_creation_input_tokens": 2000,
                            "reasoning_tokens": 1000,
                            "request_count": 42
                        }
                    ]
                })
                .to_string(),
            ))))
        });
        let provider = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com/v4/ai")
                .with_api_key("test-token"),
        )
        .with_transport(transport);
        let result = poll_ready(
            provider.get_spend_report(
                GatewaySpendReportParams::new("2026-03-01", "2026-03-25")
                    .with_group_by(GatewaySpendReportGroupBy::Model)
                    .with_date_part(GatewaySpendReportDatePart::Hour)
                    .with_user_id("user_123")
                    .with_model("anthropic/claude-sonnet-4.5")
                    .with_provider("anthropic")
                    .with_credential_type(GatewayCredentialType::Byok)
                    .with_tags(["production", "api"]),
            ),
        )
        .expect("spend report fetch succeeds");

        assert_eq!(result.results.len(), 1);
        let row = &result.results[0];
        assert_eq!(row.day.as_deref(), Some("2026-03-01"));
        assert_eq!(row.credential_type, Some(GatewayCredentialType::Byok));
        assert_eq!(row.total_cost, 12.5);
        assert_eq!(row.market_cost, Some(11.0));
        assert_eq!(row.input_tokens, Some(50000));
        assert_eq!(row.output_tokens, Some(10000));
        assert_eq!(row.cached_input_tokens, Some(5000));
        assert_eq!(row.cache_creation_input_tokens, Some(2000));
        assert_eq!(row.reasoning_tokens, Some(1000));
        assert_eq!(row.request_count, Some(42));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let url = url::Url::parse(&request.url).expect("request URL is valid");

        assert_eq!(request.method, ProviderApiRequestMethod::Get);
        assert_eq!(
            url.as_str().split('?').next(),
            Some("https://api.test.com/v1/report")
        );
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "start_date")
                .map(|(_, value)| value.into_owned()),
            Some("2026-03-01".to_string())
        );
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "end_date")
                .map(|(_, value)| value.into_owned()),
            Some("2026-03-25".to_string())
        );
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "group_by")
                .map(|(_, value)| value.into_owned()),
            Some("model".to_string())
        );
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "date_part")
                .map(|(_, value)| value.into_owned()),
            Some("hour".to_string())
        );
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "user_id")
                .map(|(_, value)| value.into_owned()),
            Some("user_123".to_string())
        );
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "model")
                .map(|(_, value)| value.into_owned()),
            Some("anthropic/claude-sonnet-4.5".to_string())
        );
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "provider")
                .map(|(_, value)| value.into_owned()),
            Some("anthropic".to_string())
        );
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "credential_type")
                .map(|(_, value)| value.into_owned()),
            Some("byok".to_string())
        );
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "tags")
                .map(|(_, value)| value.into_owned()),
            Some("production,api".to_string())
        );
    }

    #[test]
    fn gateway_provider_fetches_generation_info_and_unwraps_data() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "data": {
                        "id": "gen_01ARZ3NDEKTSV4RRFFQ69G5FAV",
                        "total_cost": 0.00123,
                        "upstream_inference_cost": 0.0011,
                        "usage": 0.00123,
                        "created_at": "2024-01-01T00:00:00.000Z",
                        "model": "gpt-4",
                        "is_byok": false,
                        "provider_name": "openai",
                        "streamed": true,
                        "finish_reason": "stop",
                        "latency": 200,
                        "generation_time": 1500,
                        "native_tokens_prompt": 100,
                        "native_tokens_completion": 50,
                        "native_tokens_reasoning": 0,
                        "native_tokens_cached": 0,
                        "native_tokens_cache_creation": 0,
                        "billable_web_search_calls": 0
                    }
                })
                .to_string(),
            ))))
        });
        let provider = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com/v4/ai")
                .with_api_key("test-token"),
        )
        .with_transport(transport);
        let result = poll_ready(
            provider.get_generation_info(GatewayGenerationInfoParams::new(
                "gen_01ARZ3NDEKTSV4RRFFQ69G5FAV",
            )),
        )
        .expect("generation info fetch succeeds");

        assert_eq!(result.id, "gen_01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert_eq!(result.total_cost, 0.00123);
        assert_eq!(result.upstream_inference_cost, 0.0011);
        assert_eq!(result.usage, 0.00123);
        assert_eq!(result.created_at, "2024-01-01T00:00:00.000Z");
        assert_eq!(result.model, "gpt-4");
        assert!(!result.is_byok);
        assert_eq!(result.provider_name, "openai");
        assert!(result.streamed);
        assert_eq!(result.finish_reason, "stop");
        assert_eq!(result.latency, 200);
        assert_eq!(result.generation_time, 1500);
        assert_eq!(result.prompt_tokens, 100);
        assert_eq!(result.completion_tokens, 50);
        assert_eq!(result.reasoning_tokens, 0);
        assert_eq!(result.cached_tokens, 0);
        assert_eq!(result.cache_creation_tokens, 0);
        assert_eq!(result.billable_web_search_calls, 0);

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let url = url::Url::parse(&request.url).expect("request URL is valid");

        assert_eq!(request.method, ProviderApiRequestMethod::Get);
        assert_eq!(
            url.as_str().split('?').next(),
            Some("https://api.test.com/v1/generation")
        );
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "id")
                .map(|(_, value)| value.into_owned()),
            Some("gen_01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string())
        );
    }

    fn gateway_stream_body() -> String {
        [
            json!({
                "type": "stream-start",
                "warnings": []
            }),
            json!({
                "type": "response-metadata",
                "id": "resp_gateway",
                "timestamp": "2024-01-02T03:04:05Z",
                "modelId": "openai/gpt-4.1-mini"
            }),
            json!({
                "type": "text-start",
                "id": "0"
            }),
            json!({
                "type": "text-delta",
                "id": "0",
                "delta": "Hello "
            }),
            json!({
                "type": "raw",
                "rawValue": {
                    "provider": "gateway"
                }
            }),
            json!({
                "type": "text-delta",
                "id": "0",
                "delta": "Gateway"
            }),
            json!({
                "type": "text-end",
                "id": "0"
            }),
            json!({
                "type": "finish",
                "finishReason": {
                    "unified": "stop",
                    "raw": "stop"
                },
                "usage": {
                    "inputTokens": {
                        "total": 2
                    },
                    "outputTokens": {
                        "total": 3
                    }
                }
            }),
        ]
        .into_iter()
        .map(|event| format!("data: {event}\n\n"))
        .chain(["data: [DONE]\n\n".to_string()])
        .collect::<String>()
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI model call"]
    fn live_gateway_openai_generate_text() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!("skipping live Gateway test because no API key is configured");
            return;
        };
        let model_id = env::var("AI_SDK_RUST_GATEWAY_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = GatewayProvider::new()
            .with_api_key(api_key)
            .language_model(model_id);
        let result = poll_ready(generate_text(
            GenerateTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("Reply with exactly: rust-gateway-ok"),
            )
            .expect("prompt is valid")
            .with_max_output_tokens(16)
            .with_temperature(0.0),
        ));

        assert!(
            result.text.to_lowercase().contains("rust-gateway-ok"),
            "gateway response did not contain expected marker"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live OpenAI model stream call"]
    fn live_gateway_openai_stream_text() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!("skipping live Gateway stream test because no API key is configured");
            return;
        };
        let model_id = env::var("AI_SDK_RUST_GATEWAY_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_MODEL"))
            .unwrap_or_else(|_| "openai/gpt-4.1-mini".to_string());
        let model = GatewayProvider::new()
            .with_api_key(api_key)
            .language_model(model_id);
        let result = poll_ready(stream_text(
            StreamTextOptions::from_prompt(
                &model,
                Prompt::from_prompt("Reply with exactly: rust-gateway-stream-ok"),
            )
            .expect("prompt is valid")
            .with_max_output_tokens(20)
            .with_temperature(0.0),
        ));

        assert!(
            result
                .text
                .to_lowercase()
                .contains("rust-gateway-stream-ok"),
            "gateway stream response did not contain expected marker"
        );
    }

    #[test]
    #[ignore = "requires a Vercel AI Gateway API key and makes a live metadata call"]
    fn live_gateway_available_models() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!("skipping live Gateway metadata test because no API key is configured");
            return;
        };
        let provider = GatewayProvider::new().with_api_key(api_key);
        let result =
            poll_ready(provider.get_available_models()).expect("gateway metadata fetch succeeds");

        assert!(
            result
                .models
                .iter()
                .any(|model| model.id.starts_with("openai/")),
            "gateway metadata did not include an OpenAI model"
        );
    }

    fn live_gateway_api_key() -> Option<String> {
        env::var("AI_SDK_RUST_AI_GATEWAY_API_KEY")
            .or_else(|_| env::var("AI_GATEWAY_API_KEY"))
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(load_gateway_api_key_from_dotenv)
    }

    fn load_gateway_api_key_from_dotenv() -> Option<String> {
        let contents = fs::read_to_string(".env.local").ok()?;

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let Some((name, value)) = line.split_once('=') else {
                continue;
            };

            if matches!(
                name.trim(),
                "AI_SDK_RUST_AI_GATEWAY_API_KEY" | "AI_GATEWAY_API_KEY"
            ) {
                let value = value
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\'')
                    .to_string();
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }

        None
    }

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures should be ready"),
        }
    }
}
