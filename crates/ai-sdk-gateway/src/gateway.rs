use std::collections::BTreeMap;
use std::env;
use std::fmt;
use std::future::{Future, Ready, ready};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use url::Url;
use url::form_urlencoded::Serializer as FormUrlEncodedSerializer;

use crate::gateway_error::{
    GATEWAY_AUTH_METHOD_HEADER, GatewayAuthMethod, GatewayAuthenticationError, GatewayError,
    GatewayInvalidRequestError, as_gateway_error, parse_gateway_auth_method,
};
use crate::gateway_tools::GatewayTools;
use ai_sdk_provider::FileDataContent;
use ai_sdk_provider::Headers;
use ai_sdk_provider::Warning;
use ai_sdk_provider::{
    ApiCallError, NoSuchModelError, Provider, ProviderMetadata, ProviderOptions,
    ProviderWithRerankingModel, ProviderWithVideoModel, SpecificationVersion,
};
use ai_sdk_provider::{
    EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResponse, EmbeddingModelResult,
    EmbeddingModelUsage,
};
use ai_sdk_provider::{
    FinishReason, InputTokenUsage, LanguageModel, LanguageModelCallOptions, LanguageModelContent,
    LanguageModelCustomContent, LanguageModelErrorStreamPart, LanguageModelFinishReason,
    LanguageModelGenerateResult, LanguageModelRequest, LanguageModelResponse,
    LanguageModelStreamPart, LanguageModelStreamResult, LanguageModelStreamResultResponse,
    LanguageModelSupportedUrls, LanguageModelText, LanguageModelUsage, OutputTokenUsage,
};
use ai_sdk_provider::{
    ImageModel, ImageModelCallOptions, ImageModelFile, ImageModelProviderMetadata,
    ImageModelProviderMetadataEntry, ImageModelResponse, ImageModelResult, ImageModelUsage,
};
use ai_sdk_provider::{JsonArray, JsonObject, JsonValue};
use ai_sdk_provider::{
    RerankingModel, RerankingModelCallOptions, RerankingModelRanking, RerankingModelResponse,
    RerankingModelResult,
};
use ai_sdk_provider::{
    VideoModel, VideoModelCallOptions, VideoModelFile, VideoModelResponse, VideoModelResult,
    VideoModelVideoData,
};
use ai_sdk_provider_utils::{
    FetchErrorInfo, GetFromApiOptions, HandledFetchError, ParseJsonResult, PostJsonToApiOptions,
    ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod, ProviderApiResponse,
    ProviderApiResponseHandlerError, ResponseHandlerResult, RuntimeEnvironment, combine_headers,
    convert_bytes_to_base64, convert_to_base64, create_event_source_response_handler,
    create_json_error_response_handler, create_json_response_handler, get_from_api,
    post_json_to_api, with_user_agent_suffix, without_trailing_slash,
};

/// Default base URL used by upstream `@ai-sdk/gateway` provider calls.
pub const DEFAULT_GATEWAY_BASE_URL: &str = "https://ai-gateway.vercel.sh/v4/ai";

const AI_GATEWAY_PROTOCOL_VERSION: &str = "0.0.1";
const GATEWAY_PROVIDER_ID: &str = "gateway";
const DEFAULT_METADATA_CACHE_REFRESH_MILLIS: u64 = 1000 * 60 * 5;
const VERCEL_OIDC_TOKEN_ENV: &str = "VERCEL_OIDC_TOKEN";
const VERCEL_REQUEST_ID_ENV: &str = "VERCEL_REQUEST_ID";
const X_VERCEL_ID_ENV: &str = "X_VERCEL_ID";
const MIN_GATEWAY_BYOK_TIMEOUT_MILLIS: u64 = 1000;

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

    /// How frequently available-model metadata should be refreshed, in
    /// milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata_cache_refresh_millis: Option<u64>,

    /// Vercel request id to send as an AI Gateway observability header.
    ///
    /// Upstream reads this from Vercel's JavaScript request context. Rust
    /// callers can provide the current request id explicitly when they have
    /// one available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vercel_request_id: Option<String>,
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

    /// Sets how frequently available-model metadata is refreshed, in
    /// milliseconds.
    pub fn with_metadata_cache_refresh_millis(
        mut self,
        metadata_cache_refresh_millis: u64,
    ) -> Self {
        self.metadata_cache_refresh_millis = Some(metadata_cache_refresh_millis);
        self
    }

    /// Sets the Vercel request id used for Gateway observability headers.
    pub fn with_vercel_request_id(mut self, vercel_request_id: impl Into<String>) -> Self {
        self.vercel_request_id = Some(vercel_request_id.into());
        self
    }
}

/// Gateway provider routing sort strategy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GatewayProviderOptionsSort {
    /// Route to the lowest-cost provider first.
    Cost,

    /// Route to the lowest time-to-first-token provider first.
    Ttft,

    /// Route to the highest tokens-per-second provider first.
    Tps,
}

/// Per-provider timeout configuration for Gateway BYOK credentials.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayProviderTimeouts {
    /// BYOK provider timeouts in milliseconds, keyed by provider slug.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub byok: BTreeMap<String, u64>,
}

impl GatewayProviderTimeouts {
    /// Creates empty Gateway provider timeouts.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a BYOK provider timeout in milliseconds.
    pub fn with_byok_timeout(mut self, provider: impl Into<String>, timeout_millis: u64) -> Self {
        self.byok.insert(provider.into(), timeout_millis);
        self
    }

    /// Adds a BYOK provider timeout after validating the upstream minimum.
    pub fn try_with_byok_timeout(
        mut self,
        provider: impl Into<String>,
        timeout_millis: u64,
    ) -> Result<Self, GatewayProviderOptionsValidationError> {
        validate_gateway_byok_timeout(timeout_millis)?;
        self.byok.insert(provider.into(), timeout_millis);
        Ok(self)
    }

    /// Validates this timeout configuration against the upstream Gateway schema.
    pub fn validate(&self) -> Result<(), GatewayProviderOptionsValidationError> {
        for (provider, timeout_millis) in &self.byok {
            if *timeout_millis < MIN_GATEWAY_BYOK_TIMEOUT_MILLIS {
                return Err(GatewayProviderOptionsValidationError::new(format!(
                    "Gateway providerTimeouts.byok.{provider} must be at least {MIN_GATEWAY_BYOK_TIMEOUT_MILLIS} milliseconds"
                )));
            }
        }

        Ok(())
    }
}

/// Request-scoped Gateway provider options.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayProviderOptions {
    /// Provider slugs that are the only ones allowed to be used.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,

    /// Provider slugs in the sequence Gateway should attempt them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<String>,

    /// Sort providers by a routing metric before dispatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<GatewayProviderOptionsSort>,

    /// End-user identifier for spend attribution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    /// User-specified tags for reporting and filtering usage.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,

    /// Fallback model ids to use in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,

    /// Request-scoped BYOK credentials keyed by provider slug.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub byok: BTreeMap<String, Vec<JsonObject>>,

    /// Filter to providers with zero-data-retention agreements.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zero_data_retention: Option<bool>,

    /// Filter to providers that do not train on prompt data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disallow_prompt_training: Option<bool>,

    /// Filter to HIPAA-compliant providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hipaa_compliant: Option<bool>,

    /// Entity id used for quota tracking and enforcement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_entity_id: Option<String>,

    /// Per-provider BYOK timeout settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_timeouts: Option<GatewayProviderTimeouts>,
}

impl GatewayProviderOptions {
    /// Creates empty Gateway provider options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the only allowed provider slugs.
    pub fn with_only<I, S>(mut self, only: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.only = only.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the provider routing order.
    pub fn with_order<I, S>(mut self, order: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.order = order.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the provider routing sort strategy.
    pub fn with_sort(mut self, sort: GatewayProviderOptionsSort) -> Self {
        self.sort = Some(sort);
        self
    }

    /// Sets the end-user identifier for spend attribution.
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Sets reporting tags.
    pub fn with_tags<I, S>(mut self, tags: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.tags = tags.into_iter().map(Into::into).collect();
        self
    }

    /// Sets fallback model ids.
    pub fn with_models<I, S>(mut self, models: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.models = models.into_iter().map(Into::into).collect();
        self
    }

    /// Adds BYOK credentials for a provider slug.
    pub fn with_byok_credentials<I>(mut self, provider: impl Into<String>, credentials: I) -> Self
    where
        I: IntoIterator<Item = JsonObject>,
    {
        self.byok
            .insert(provider.into(), credentials.into_iter().collect());
        self
    }

    /// Sets whether Gateway should require zero-data-retention providers.
    pub fn with_zero_data_retention(mut self, zero_data_retention: bool) -> Self {
        self.zero_data_retention = Some(zero_data_retention);
        self
    }

    /// Sets whether Gateway should reject providers that train on prompt data.
    pub fn with_disallow_prompt_training(mut self, disallow_prompt_training: bool) -> Self {
        self.disallow_prompt_training = Some(disallow_prompt_training);
        self
    }

    /// Sets whether Gateway should require HIPAA-compliant providers.
    pub fn with_hipaa_compliant(mut self, hipaa_compliant: bool) -> Self {
        self.hipaa_compliant = Some(hipaa_compliant);
        self
    }

    /// Sets the quota entity id.
    pub fn with_quota_entity_id(mut self, quota_entity_id: impl Into<String>) -> Self {
        self.quota_entity_id = Some(quota_entity_id.into());
        self
    }

    /// Sets BYOK provider timeout configuration.
    pub fn with_provider_timeouts(mut self, provider_timeouts: GatewayProviderTimeouts) -> Self {
        self.provider_timeouts = Some(provider_timeouts);
        self
    }

    /// Validates these options against the upstream Gateway provider-options schema.
    pub fn validate(&self) -> Result<(), GatewayProviderOptionsValidationError> {
        if let Some(provider_timeouts) = &self.provider_timeouts {
            provider_timeouts.validate()?;
        }

        Ok(())
    }

    /// Converts validated options into the provider-options map expected by
    /// language and model call options.
    pub fn try_into_provider_options(
        self,
    ) -> Result<ProviderOptions, GatewayProviderOptionsValidationError> {
        self.validate()?;
        Ok(gateway_provider_options(self))
    }

    /// Converts these options into the provider-options map expected by
    /// language and model call options.
    pub fn into_provider_options(self) -> ProviderOptions {
        gateway_provider_options(self)
    }
}

impl From<GatewayProviderOptions> for ProviderOptions {
    fn from(options: GatewayProviderOptions) -> Self {
        options.into_provider_options()
    }
}

/// Wraps request-scoped Gateway options in a provider-options map.
pub fn gateway_provider_options(options: GatewayProviderOptions) -> ProviderOptions {
    let gateway_options = serde_json::to_value(options)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();

    ProviderOptions::from([(GATEWAY_PROVIDER_ID.to_string(), gateway_options)])
}

/// Validates and wraps request-scoped Gateway options in a provider-options map.
pub fn try_gateway_provider_options(
    options: GatewayProviderOptions,
) -> Result<ProviderOptions, GatewayProviderOptionsValidationError> {
    options.try_into_provider_options()
}

/// Error returned when Gateway provider options fail upstream schema validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GatewayProviderOptionsValidationError {
    message: String,
}

impl GatewayProviderOptionsValidationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the validation message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for GatewayProviderOptionsValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for GatewayProviderOptionsValidationError {}

fn validate_gateway_byok_timeout(
    timeout_millis: u64,
) -> Result<(), GatewayProviderOptionsValidationError> {
    if timeout_millis < MIN_GATEWAY_BYOK_TIMEOUT_MILLIS {
        return Err(GatewayProviderOptionsValidationError::new(format!(
            "Gateway providerTimeouts.byok values must be at least {MIN_GATEWAY_BYOK_TIMEOUT_MILLIS} milliseconds"
        )));
    }

    Ok(())
}

/// Authentication token selected for an AI Gateway request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayAuthToken {
    /// Bearer token value sent in the Authorization header.
    pub token: String,

    /// Authentication source advertised to Gateway.
    pub auth_method: GatewayAuthMethod,
}

impl GatewayAuthToken {
    fn new(token: impl Into<String>, auth_method: GatewayAuthMethod) -> Self {
        Self {
            token: token.into(),
            auth_method,
        }
    }
}

/// Vercel AI Gateway provider.
#[derive(Clone)]
pub struct GatewayProvider {
    settings: GatewayProviderSettings,
    transport: GatewayTransport,
    metadata_cache: Arc<Mutex<GatewayMetadataCache>>,
}

#[derive(Clone, Debug, Default)]
struct GatewayMetadataCache {
    fetched_at: Option<Instant>,
    value: Option<GatewayFetchMetadataResponse>,
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
            metadata_cache: Arc::new(Mutex::new(GatewayMetadataCache::default())),
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

    /// Sets how frequently available-model metadata is refreshed, in
    /// milliseconds.
    pub fn with_metadata_cache_refresh_millis(
        mut self,
        metadata_cache_refresh_millis: u64,
    ) -> Self {
        self.settings.metadata_cache_refresh_millis = Some(metadata_cache_refresh_millis);
        self
    }

    /// Sets the Vercel request id used for Gateway observability headers.
    pub fn with_vercel_request_id(mut self, vercel_request_id: impl Into<String>) -> Self {
        self.settings.vercel_request_id = Some(vercel_request_id.into());
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

    /// Creates a Gateway embedding model.
    pub fn embedding_model(&self, model_id: impl Into<String>) -> GatewayEmbeddingModel {
        GatewayEmbeddingModel {
            model_id: model_id.into(),
            settings: self.settings.clone(),
            transport: Arc::clone(&self.transport),
        }
    }

    /// Alias for [`GatewayProvider::embedding_model`].
    pub fn embedding(&self, model_id: impl Into<String>) -> GatewayEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Deprecated upstream alias for [`GatewayProvider::embedding_model`].
    pub fn text_embedding_model(&self, model_id: impl Into<String>) -> GatewayEmbeddingModel {
        self.embedding_model(model_id)
    }

    /// Creates a Gateway image model.
    pub fn image_model(&self, model_id: impl Into<String>) -> GatewayImageModel {
        GatewayImageModel {
            model_id: model_id.into(),
            settings: self.settings.clone(),
            transport: Arc::clone(&self.transport),
        }
    }

    /// Alias for [`GatewayProvider::image_model`].
    pub fn image(&self, model_id: impl Into<String>) -> GatewayImageModel {
        self.image_model(model_id)
    }

    /// Creates a Gateway reranking model.
    pub fn reranking_model(&self, model_id: impl Into<String>) -> GatewayRerankingModel {
        GatewayRerankingModel {
            model_id: model_id.into(),
            settings: self.settings.clone(),
            transport: Arc::clone(&self.transport),
        }
    }

    /// Alias for [`GatewayProvider::reranking_model`].
    pub fn reranking(&self, model_id: impl Into<String>) -> GatewayRerankingModel {
        self.reranking_model(model_id)
    }

    /// Creates a Gateway video model.
    pub fn video_model(&self, model_id: impl Into<String>) -> GatewayVideoModel {
        GatewayVideoModel {
            model_id: model_id.into(),
            settings: self.settings.clone(),
            transport: Arc::clone(&self.transport),
        }
    }

    /// Alias for [`GatewayProvider::video_model`].
    pub fn video(&self, model_id: impl Into<String>) -> GatewayVideoModel {
        self.video_model(model_id)
    }

    /// Returns Gateway-specific provider-executed tools.
    pub fn tools(&self) -> GatewayTools {
        GatewayTools::new()
    }

    /// Returns available Gateway models for the authenticated account.
    pub async fn get_available_models(&self) -> Result<GatewayFetchMetadataResponse, GatewayError> {
        if let Some(cached) = self.cached_available_models() {
            return Ok(cached);
        }

        let request_headers = try_gateway_provider_headers(&self.settings)?;
        let auth_method = parse_gateway_auth_method(&request_headers);
        let get_options = GetFromApiOptions::new(format!("{}/config", self.base_url()))
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        let response = get_gateway_json(
            get_options,
            transport,
            gateway_fetch_metadata_response,
            auth_method,
        )
        .await?;
        self.store_available_models(response.clone());

        Ok(response)
    }

    /// Returns credit balance information for the authenticated Gateway account.
    pub async fn get_credits(&self) -> Result<GatewayCreditsResponse, GatewayError> {
        let request_headers = try_gateway_provider_headers(&self.settings)?;
        let auth_method = parse_gateway_auth_method(&request_headers);
        let get_options =
            GetFromApiOptions::new(gateway_origin_url(&self.base_url(), "/v1/credits")?)
                .with_headers(request_headers)
                .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        get_gateway_json(
            get_options,
            transport,
            gateway_credits_response,
            auth_method,
        )
        .await
    }

    /// Returns a Gateway spend report for the supplied date range and filters.
    pub async fn get_spend_report(
        &self,
        params: GatewaySpendReportParams,
    ) -> Result<GatewaySpendReportResponse, GatewayError> {
        let request_headers = try_gateway_provider_headers(&self.settings)?;
        let auth_method = parse_gateway_auth_method(&request_headers);
        let url = gateway_spend_report_url(&self.base_url(), &params)?;
        let get_options = GetFromApiOptions::new(url)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        get_gateway_json(
            get_options,
            transport,
            gateway_spend_report_response,
            auth_method,
        )
        .await
    }

    /// Returns detailed information for a specific Gateway generation id.
    pub async fn get_generation_info(
        &self,
        params: GatewayGenerationInfoParams,
    ) -> Result<GatewayGenerationInfo, GatewayError> {
        let request_headers = try_gateway_provider_headers(&self.settings)?;
        let auth_method = parse_gateway_auth_method(&request_headers);
        let url = gateway_generation_info_url(&self.base_url(), &params)?;
        let get_options = GetFromApiOptions::new(url)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown());
        let transport = Arc::clone(&self.transport);

        get_gateway_json(
            get_options,
            transport,
            gateway_generation_info_response,
            auth_method,
        )
        .await
    }

    fn base_url(&self) -> String {
        gateway_base_url(&self.settings)
    }

    fn cached_available_models(&self) -> Option<GatewayFetchMetadataResponse> {
        let refresh_duration = metadata_cache_refresh_duration(&self.settings);
        if refresh_duration.is_zero() {
            return None;
        }

        let cache = self
            .metadata_cache
            .lock()
            .expect("gateway metadata cache mutex is not poisoned");
        let fetched_at = cache.fetched_at?;

        if fetched_at.elapsed() < refresh_duration {
            cache.value.clone()
        } else {
            None
        }
    }

    fn store_available_models(&self, response: GatewayFetchMetadataResponse) {
        let mut cache = self
            .metadata_cache
            .lock()
            .expect("gateway metadata cache mutex is not poisoned");

        cache.fetched_at = Some(Instant::now());
        cache.value = Some(response);
    }
}

impl Default for GatewayProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for GatewayProvider {
    type LanguageModel = GatewayLanguageModel;
    type EmbeddingModel = GatewayEmbeddingModel;
    type ImageModel = GatewayImageModel;

    fn language_model(&self, model_id: &str) -> Result<Self::LanguageModel, NoSuchModelError> {
        Ok(GatewayProvider::language_model(self, model_id))
    }

    fn embedding_model(&self, model_id: &str) -> Result<Self::EmbeddingModel, NoSuchModelError> {
        Ok(GatewayProvider::embedding_model(self, model_id))
    }

    fn image_model(&self, model_id: &str) -> Result<Self::ImageModel, NoSuchModelError> {
        Ok(GatewayProvider::image_model(self, model_id))
    }
}

impl ProviderWithRerankingModel for GatewayProvider {
    type RerankingModel = GatewayRerankingModel;

    fn reranking_model(&self, model_id: &str) -> Result<Self::RerankingModel, NoSuchModelError> {
        Ok(GatewayProvider::reranking_model(self, model_id))
    }
}

impl ProviderWithVideoModel for GatewayProvider {
    type VideoModel = GatewayVideoModel;

    fn video_model(&self, model_id: &str) -> Result<Self::VideoModel, NoSuchModelError> {
        Ok(GatewayProvider::video_model(self, model_id))
    }
}

/// Creates a Gateway provider with explicit settings.
pub fn create_gateway(settings: GatewayProviderSettings) -> GatewayProvider {
    GatewayProvider::from_settings(settings)
}

/// Deprecated upstream alias for [`create_gateway`].
pub fn create_gateway_provider(settings: GatewayProviderSettings) -> GatewayProvider {
    create_gateway(settings)
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

/// Native AI SDK Gateway embedding model.
#[derive(Clone)]
pub struct GatewayEmbeddingModel {
    model_id: String,
    settings: GatewayProviderSettings,
    transport: GatewayTransport,
}

/// Native AI SDK Gateway image model.
#[derive(Clone)]
pub struct GatewayImageModel {
    model_id: String,
    settings: GatewayProviderSettings,
    transport: GatewayTransport,
}

/// Native AI SDK Gateway reranking model.
#[derive(Clone)]
pub struct GatewayRerankingModel {
    model_id: String,
    settings: GatewayProviderSettings,
    transport: GatewayTransport,
}

/// Native AI SDK Gateway video model.
#[derive(Clone)]
pub struct GatewayVideoModel {
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
        let request_body = gateway_language_model_request_body(&options);
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref(), false);
        let auth_method = parse_gateway_auth_method(&request_headers);
        let post_options = PostJsonToApiOptions::new(self.language_model_url(), request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
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
            Err(error) => {
                self.generate_result_from_error(error, request_body_for_error, auth_method)
            }
        }
    }

    async fn do_stream_result(
        &self,
        options: LanguageModelCallOptions,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let include_raw_chunks = options.include_raw_chunks.unwrap_or(false);
        let request_body = gateway_language_model_request_body(&options);
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref(), true);
        let auth_method = parse_gateway_auth_method(&request_headers);
        let post_options = PostJsonToApiOptions::new(self.language_model_url(), request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
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
            Err(error) => self.stream_result_from_error(error, request_body_for_error, auth_method),
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
        let observability_headers = gateway_observability_header_entries(&self.settings);
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

        combine_headers([
            provider_headers,
            call_headers,
            model_headers,
            observability_headers,
        ])
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
        auth_method: Option<GatewayAuthMethod>,
    ) -> LanguageModelGenerateResult {
        let (headers, body) = gateway_error_response_context(&error);
        let response_body = body
            .as_deref()
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .or_else(|| body.map(JsonValue::String));
        let gateway_error = as_gateway_error(error, auth_method);
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

        result = result.with_provider_metadata(gateway_error_metadata_from_error(&gateway_error));
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
        auth_method: Option<GatewayAuthMethod>,
    ) -> LanguageModelStreamResult<Vec<LanguageModelStreamPart>> {
        let (headers, body) = gateway_error_response_context(&error);
        let gateway_error = as_gateway_error(error, auth_method);
        let mut result =
            LanguageModelStreamResult::new(vec![gateway_stream_error_from_gateway_error(
                &gateway_error,
                body.as_deref(),
            )])
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

impl GatewayEmbeddingModel {
    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: GatewayTransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_embed_result(&self, options: EmbeddingModelCallOptions) -> EmbeddingModelResult {
        let request_body = gateway_embedding_request_body(&options);
        let request_body_for_error = request_body.clone();
        let request_body_for_response = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let auth_method = parse_gateway_auth_method(&request_headers);
        let post_options = PostJsonToApiOptions::new(self.embedding_model_url(), request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    gateway_embedding_response,
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
            Ok(response) => embedding_result_from_response(
                response.value,
                response.raw_value,
                response.response_headers,
                request_body_for_response,
            ),
            Err(error) => embedding_result_from_error(error, request_body_for_error, auth_method),
        }
    }

    fn embedding_model_url(&self) -> String {
        format!("{}/embedding-model", self.base_url())
    }

    fn base_url(&self) -> String {
        gateway_base_url(&self.settings)
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        let provider_headers = Some(
            gateway_provider_headers(&self.settings)
                .into_iter()
                .collect::<Vec<_>>(),
        );
        let call_headers = optional_headers(call_headers);
        let observability_headers = gateway_observability_header_entries(&self.settings);
        let model_headers = Some(vec![
            (
                "ai-embedding-model-specification-version".to_string(),
                Some("4".to_string()),
            ),
            ("ai-model-id".to_string(), Some(self.model_id.clone())),
        ]);

        combine_headers([
            provider_headers,
            call_headers,
            model_headers,
            observability_headers,
        ])
    }
}

impl GatewayImageModel {
    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: GatewayTransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_generate_result(&self, options: ImageModelCallOptions) -> ImageModelResult {
        let request_body = gateway_image_request_body(&options);
        let request_headers = self.request_headers(options.headers.as_ref());
        let auth_method = parse_gateway_auth_method(&request_headers);
        let post_options = PostJsonToApiOptions::new(self.image_model_url(), request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    gateway_image_response,
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
            Ok(response) => image_result_from_response(
                &self.model_id,
                response.value,
                response.response_headers,
            ),
            Err(error) => image_result_from_error(&self.model_id, error, auth_method),
        }
    }

    fn image_model_url(&self) -> String {
        format!("{}/image-model", self.base_url())
    }

    fn base_url(&self) -> String {
        gateway_base_url(&self.settings)
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        let provider_headers = Some(
            gateway_provider_headers(&self.settings)
                .into_iter()
                .collect::<Vec<_>>(),
        );
        let call_headers = optional_headers(call_headers);
        let observability_headers = gateway_observability_header_entries(&self.settings);
        let model_headers = Some(vec![
            (
                "ai-image-model-specification-version".to_string(),
                Some("4".to_string()),
            ),
            ("ai-model-id".to_string(), Some(self.model_id.clone())),
        ]);

        combine_headers([
            provider_headers,
            call_headers,
            model_headers,
            observability_headers,
        ])
    }
}

impl GatewayRerankingModel {
    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: GatewayTransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_rerank_result(&self, options: RerankingModelCallOptions) -> RerankingModelResult {
        let request_body = gateway_reranking_request_body(&options);
        let request_body_for_error = request_body.clone();
        let request_headers = self.request_headers(options.headers.as_ref());
        let auth_method = parse_gateway_auth_method(&request_headers);
        let post_options = PostJsonToApiOptions::new(self.reranking_model_url(), request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    gateway_reranking_response,
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
            Ok(response) => reranking_result_from_response(
                response.value,
                response.raw_value,
                response.response_headers,
            ),
            Err(error) => reranking_result_from_error(error, request_body_for_error, auth_method),
        }
    }

    fn reranking_model_url(&self) -> String {
        format!("{}/reranking-model", self.base_url())
    }

    fn base_url(&self) -> String {
        gateway_base_url(&self.settings)
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        let provider_headers = Some(
            gateway_provider_headers(&self.settings)
                .into_iter()
                .collect::<Vec<_>>(),
        );
        let call_headers = optional_headers(call_headers);
        let observability_headers = gateway_observability_header_entries(&self.settings);
        let model_headers = Some(vec![
            (
                "ai-reranking-model-specification-version".to_string(),
                Some("4".to_string()),
            ),
            ("ai-model-id".to_string(), Some(self.model_id.clone())),
        ]);

        combine_headers([
            provider_headers,
            call_headers,
            model_headers,
            observability_headers,
        ])
    }
}

impl GatewayVideoModel {
    /// Returns a copy of this model that uses the supplied HTTP transport.
    pub fn with_transport(mut self, transport: GatewayTransport) -> Self {
        self.transport = transport;
        self
    }

    async fn do_generate_result(&self, options: VideoModelCallOptions) -> VideoModelResult {
        let request_body = gateway_video_request_body(&options);
        let request_headers = self.request_headers(options.headers.as_ref());
        let auth_method = parse_gateway_auth_method(&request_headers);
        let post_options = PostJsonToApiOptions::new(self.video_model_url(), request_body)
            .with_headers(request_headers)
            .with_environment(RuntimeEnvironment::unknown())
            .with_optional_abort_signal(options.abort_signal.clone());
        let transport = Arc::clone(&self.transport);

        match post_json_to_api(
            post_options,
            move |request| (transport)(request),
            gateway_video_response_handler,
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
            Ok(response) => video_result_from_response(
                &self.model_id,
                response.value,
                response.response_headers,
            ),
            Err(error) => video_result_from_error(&self.model_id, error, auth_method),
        }
    }

    fn video_model_url(&self) -> String {
        format!("{}/video-model", self.base_url())
    }

    fn base_url(&self) -> String {
        gateway_base_url(&self.settings)
    }

    fn request_headers(&self, call_headers: Option<&Headers>) -> BTreeMap<String, Option<String>> {
        let provider_headers = Some(
            gateway_provider_headers(&self.settings)
                .into_iter()
                .collect::<Vec<_>>(),
        );
        let call_headers = optional_headers(call_headers);
        let observability_headers = gateway_observability_header_entries(&self.settings);
        let model_headers = Some(vec![
            (
                "ai-video-model-specification-version".to_string(),
                Some("4".to_string()),
            ),
            ("ai-model-id".to_string(), Some(self.model_id.clone())),
        ]);
        let accept_headers = Some(vec![(
            "accept".to_string(),
            Some("text/event-stream".to_string()),
        )]);

        combine_headers([
            provider_headers,
            call_headers,
            model_headers,
            observability_headers,
            accept_headers,
        ])
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

impl EmbeddingModel for GatewayEmbeddingModel {
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

    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    fn provider(&self) -> &str {
        GATEWAY_PROVIDER_ID
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn max_embeddings_per_call(&self) -> Self::MaxEmbeddingsPerCallFuture<'_> {
        ready(Some(2048))
    }

    fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
        ready(true)
    }

    fn do_embed(&self, options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
        Box::pin(self.do_embed_result(options))
    }
}

impl ImageModel for GatewayImageModel {
    type MaxImagesPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = ImageModelResult> + Send + 'a>>
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

    fn max_images_per_call(&self) -> Self::MaxImagesPerCallFuture<'_> {
        ready(Some(usize::MAX))
    }

    fn do_generate(&self, options: ImageModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

impl RerankingModel for GatewayRerankingModel {
    type RerankFuture<'a>
        = Pin<Box<dyn Future<Output = RerankingModelResult> + Send + 'a>>
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

    fn do_rerank(&self, options: RerankingModelCallOptions) -> Self::RerankFuture<'_> {
        Box::pin(self.do_rerank_result(options))
    }
}

impl VideoModel for GatewayVideoModel {
    type MaxVideosPerCallFuture<'a>
        = Ready<Option<usize>>
    where
        Self: 'a;

    type GenerateFuture<'a>
        = Pin<Box<dyn Future<Output = VideoModelResult> + Send + 'a>>
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

    fn max_videos_per_call(&self) -> Self::MaxVideosPerCallFuture<'_> {
        ready(Some(usize::MAX))
    }

    fn do_generate(&self, options: VideoModelCallOptions) -> Self::GenerateFuture<'_> {
        Box::pin(self.do_generate_result(options))
    }
}

fn gateway_base_url(settings: &GatewayProviderSettings) -> String {
    without_trailing_slash(settings.base_url.as_deref())
        .unwrap_or(DEFAULT_GATEWAY_BASE_URL)
        .to_string()
}

fn metadata_cache_refresh_duration(settings: &GatewayProviderSettings) -> Duration {
    Duration::from_millis(
        settings
            .metadata_cache_refresh_millis
            .unwrap_or(DEFAULT_METADATA_CACHE_REFRESH_MILLIS),
    )
}

/// Resolves the Gateway bearer token using upstream precedence:
/// explicit API key, `AI_GATEWAY_API_KEY`, this crate's compatibility
/// `AI_SDK_RUST_AI_GATEWAY_API_KEY`, then Vercel OIDC.
pub fn get_gateway_auth_token(
    settings: &GatewayProviderSettings,
) -> Result<GatewayAuthToken, GatewayError> {
    get_gateway_auth_token_with_env(settings, |name| env::var(name))
}

fn get_gateway_auth_token_with_env(
    settings: &GatewayProviderSettings,
    mut load_env: impl FnMut(&str) -> Result<String, env::VarError>,
) -> Result<GatewayAuthToken, GatewayError> {
    if let Some(api_key) = non_empty_optional_setting(settings.api_key.clone())
        .or_else(|| non_empty_env_setting("AI_GATEWAY_API_KEY", &mut load_env))
        .or_else(|| non_empty_env_setting("AI_SDK_RUST_AI_GATEWAY_API_KEY", &mut load_env))
    {
        return Ok(GatewayAuthToken::new(api_key, GatewayAuthMethod::ApiKey));
    }

    if let Some(oidc_token) = non_empty_env_setting(VERCEL_OIDC_TOKEN_ENV, &mut load_env) {
        return Ok(GatewayAuthToken::new(oidc_token, GatewayAuthMethod::Oidc));
    }

    Err(GatewayAuthenticationError::create_contextual_error(false, false).into())
}

fn gateway_provider_headers(
    settings: &GatewayProviderSettings,
) -> BTreeMap<String, Option<String>> {
    gateway_provider_headers_with_auth(settings, get_gateway_auth_token(settings).ok())
}

fn try_gateway_provider_headers(
    settings: &GatewayProviderSettings,
) -> Result<BTreeMap<String, Option<String>>, GatewayError> {
    let auth = get_gateway_auth_token(settings)?;
    Ok(gateway_provider_headers_with_auth(settings, Some(auth)))
}

fn gateway_provider_headers_with_auth(
    settings: &GatewayProviderSettings,
    auth: Option<GatewayAuthToken>,
) -> BTreeMap<String, Option<String>> {
    let mut headers = BTreeMap::from([(
        "ai-gateway-protocol-version".to_string(),
        Some(AI_GATEWAY_PROTOCOL_VERSION.to_string()),
    )]);

    if let Some(auth) = auth {
        headers.insert(
            "Authorization".to_string(),
            Some(format!("Bearer {}", auth.token)),
        );
        headers.insert(
            GATEWAY_AUTH_METHOD_HEADER.to_string(),
            Some(auth.auth_method.as_str().to_string()),
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

#[cfg(test)]
fn gateway_provider_headers_with_env(
    settings: &GatewayProviderSettings,
    load_env: impl FnMut(&str) -> Result<String, env::VarError>,
) -> BTreeMap<String, Option<String>> {
    gateway_provider_headers_with_auth(
        settings,
        get_gateway_auth_token_with_env(settings, load_env).ok(),
    )
}

/// Builds Gateway observability headers from Vercel deployment metadata.
pub fn gateway_observability_headers(settings: &GatewayProviderSettings) -> Headers {
    gateway_observability_headers_with_env(settings, |name| env::var(name))
}

fn gateway_observability_headers_with_env(
    settings: &GatewayProviderSettings,
    mut load_env: impl FnMut(&str) -> Result<String, env::VarError>,
) -> Headers {
    let mut headers = Headers::new();

    if let Some(value) = non_empty_env_setting("VERCEL_DEPLOYMENT_ID", &mut load_env) {
        headers.insert("ai-o11y-deployment-id".to_string(), value);
    }

    if let Some(value) = non_empty_env_setting("VERCEL_ENV", &mut load_env) {
        headers.insert("ai-o11y-environment".to_string(), value);
    }

    if let Some(value) = non_empty_env_setting("VERCEL_REGION", &mut load_env) {
        headers.insert("ai-o11y-region".to_string(), value);
    }

    if let Some(value) = non_empty_optional_setting(settings.vercel_request_id.clone())
        .or_else(|| non_empty_env_setting(VERCEL_REQUEST_ID_ENV, &mut load_env))
        .or_else(|| non_empty_env_setting(X_VERCEL_ID_ENV, &mut load_env))
    {
        headers.insert("ai-o11y-request-id".to_string(), value);
    }

    if let Some(value) = non_empty_env_setting("VERCEL_PROJECT_ID", &mut load_env) {
        headers.insert("ai-o11y-project-id".to_string(), value);
    }

    headers
}

fn non_empty_optional_setting(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn non_empty_env_setting(
    name: &str,
    load_env: &mut impl FnMut(&str) -> Result<String, env::VarError>,
) -> Option<String> {
    non_empty_optional_setting(load_env(name).ok())
}

fn gateway_observability_header_entries(
    settings: &GatewayProviderSettings,
) -> Option<Vec<(String, Option<String>)>> {
    let headers = gateway_observability_headers(settings);

    if headers.is_empty() {
        None
    } else {
        Some(
            headers
                .into_iter()
                .map(|(name, value)| (name, Some(value)))
                .collect(),
        )
    }
}

fn gateway_origin_url(base_url: &str, path: &str) -> Result<String, GatewayError> {
    let url = Url::parse(base_url).map_err(|error| {
        GatewayInvalidRequestError::with_message(format!("invalid Gateway base URL: {error}"))
    })?;
    let mut origin = url.origin().ascii_serialization();
    origin.push_str(path);

    Ok(origin)
}

fn gateway_spend_report_url(
    base_url: &str,
    params: &GatewaySpendReportParams,
) -> Result<String, GatewayError> {
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
) -> Result<String, GatewayError> {
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
    auth_method: Option<crate::gateway_error::GatewayAuthMethod>,
) -> Result<T, GatewayError>
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
    .map_err(|error| as_gateway_error(error, auth_method))
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

fn gateway_language_model_request_body(options: &LanguageModelCallOptions) -> JsonValue {
    let mut request_body = serde_json::to_value(options).unwrap_or_else(|error| {
        json!({
            "serializationError": error.to_string()
        })
    });
    encode_gateway_prompt_file_bytes(&mut request_body);
    request_body
}

fn encode_gateway_prompt_file_bytes(request_body: &mut JsonValue) {
    let Some(messages) = request_body
        .get_mut("prompt")
        .and_then(JsonValue::as_array_mut)
    else {
        return;
    };

    for message in messages {
        let Some(parts) = message.get_mut("content").and_then(JsonValue::as_array_mut) else {
            continue;
        };

        for part in parts {
            encode_gateway_file_part_bytes(part);
        }
    }
}

fn encode_gateway_file_part_bytes(part: &mut JsonValue) {
    let Some(part) = part.as_object_mut() else {
        return;
    };
    if part.get("type").and_then(JsonValue::as_str) != Some("file") {
        return;
    }

    let Some(data) = part.get_mut("data").and_then(JsonValue::as_object_mut) else {
        return;
    };
    if data.get("type").and_then(JsonValue::as_str) != Some("data") {
        return;
    }

    let Some(bytes) = data
        .get("data")
        .and_then(JsonValue::as_array)
        .and_then(json_array_to_bytes)
    else {
        return;
    };

    data.insert(
        "data".to_string(),
        JsonValue::String(convert_bytes_to_base64(&bytes)),
    );
}

fn json_array_to_bytes(array: &JsonArray) -> Option<Vec<u8>> {
    array
        .iter()
        .map(|value| value.as_u64().and_then(|number| u8::try_from(number).ok()))
        .collect()
}

fn gateway_embedding_request_body(options: &EmbeddingModelCallOptions) -> JsonValue {
    let mut body = JsonObject::new();
    body.insert("values".to_string(), json!(&options.values));

    if let Some(provider_options) = &options.provider_options {
        body.insert(
            "providerOptions".to_string(),
            serde_json::to_value(provider_options).unwrap_or(JsonValue::Null),
        );
    }

    JsonValue::Object(body)
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct GatewayEmbeddingResponse {
    embeddings: Vec<Vec<f64>>,
    #[serde(default)]
    usage: Option<EmbeddingModelUsage>,
    #[serde(default)]
    provider_metadata: Option<ProviderMetadata>,
}

fn gateway_embedding_response(
    value: &JsonValue,
) -> Result<GatewayEmbeddingResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn embedding_result_from_response(
    response: GatewayEmbeddingResponse,
    raw_response: Option<JsonValue>,
    response_headers: Option<Headers>,
    request_body: JsonValue,
) -> EmbeddingModelResult {
    let mut result = EmbeddingModelResult::new(response.embeddings);

    if let Some(usage) = response.usage {
        result = result.with_usage(usage);
    }

    if let Some(provider_metadata) = response.provider_metadata {
        result = result.with_provider_metadata(provider_metadata);
    }

    let mut response_metadata =
        EmbeddingModelResponse::new().with_body(raw_response.unwrap_or(request_body));

    if let Some(headers) = response_headers {
        response_metadata = with_embedding_response_headers(response_metadata, headers);
    }

    result.with_response(response_metadata)
}

fn embedding_result_from_error(
    error: HandledFetchError,
    request_body: JsonValue,
    auth_method: Option<GatewayAuthMethod>,
) -> EmbeddingModelResult {
    let (headers, body) = gateway_error_response_context(&error);
    let response_body = body
        .as_deref()
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
        .or_else(|| body.map(JsonValue::String))
        .unwrap_or(request_body);
    let gateway_error = as_gateway_error(error, auth_method);
    let mut response = EmbeddingModelResponse::new().with_body(response_body);

    if let Some(headers) = headers {
        response = with_embedding_response_headers(response, headers);
    }

    EmbeddingModelResult::new(Vec::new())
        .with_provider_metadata(gateway_error_metadata_from_error(&gateway_error))
        .with_response(response)
}

fn gateway_image_request_body(options: &ImageModelCallOptions) -> JsonValue {
    let mut body = JsonObject::new();

    if let Some(prompt) = &options.prompt {
        body.insert("prompt".to_string(), JsonValue::String(prompt.clone()));
    }

    body.insert("n".to_string(), json!(options.n));

    if let Some(size) = &options.size {
        body.insert("size".to_string(), JsonValue::String(size.clone()));
    }

    if let Some(aspect_ratio) = &options.aspect_ratio {
        body.insert(
            "aspectRatio".to_string(),
            JsonValue::String(aspect_ratio.clone()),
        );
    }

    if let Some(seed) = options.seed {
        body.insert("seed".to_string(), json!(seed));
    }

    body.insert(
        "providerOptions".to_string(),
        serde_json::to_value(&options.provider_options).unwrap_or(JsonValue::Null),
    );

    if let Some(files) = &options.files {
        body.insert(
            "files".to_string(),
            JsonValue::Array(files.iter().map(gateway_image_file_value).collect()),
        );
    }

    if let Some(mask) = &options.mask {
        body.insert("mask".to_string(), gateway_image_file_value(mask));
    }

    JsonValue::Object(body)
}

fn gateway_image_file_value(file: &ImageModelFile) -> JsonValue {
    match file {
        ImageModelFile::File {
            media_type,
            data,
            provider_options,
        } => {
            let mut value = JsonObject::new();
            value.insert("type".to_string(), JsonValue::String("file".to_string()));
            value.insert(
                "mediaType".to_string(),
                JsonValue::String(media_type.clone()),
            );
            value.insert(
                "data".to_string(),
                JsonValue::String(convert_to_base64(data)),
            );

            if let Some(provider_options) = provider_options {
                value.insert(
                    "providerOptions".to_string(),
                    serde_json::to_value(provider_options).unwrap_or(JsonValue::Null),
                );
            }

            JsonValue::Object(value)
        }
        ImageModelFile::Url {
            url,
            provider_options,
        } => {
            let mut value = JsonObject::new();
            value.insert("type".to_string(), JsonValue::String("url".to_string()));
            value.insert("url".to_string(), JsonValue::String(url.to_string()));

            if let Some(provider_options) = provider_options {
                value.insert(
                    "providerOptions".to_string(),
                    serde_json::to_value(provider_options).unwrap_or(JsonValue::Null),
                );
            }

            JsonValue::Object(value)
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct GatewayImageResponse {
    images: Vec<String>,
    #[serde(default)]
    warnings: Vec<Warning>,
    #[serde(default)]
    provider_metadata: Option<JsonValue>,
    #[serde(default)]
    usage: Option<ImageModelUsage>,
}

fn gateway_image_response(value: &JsonValue) -> Result<GatewayImageResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn image_result_from_response(
    model_id: &str,
    response: GatewayImageResponse,
    response_headers: Option<Headers>,
) -> ImageModelResult {
    let mut result = ImageModelResult::new(
        response
            .images
            .into_iter()
            .map(FileDataContent::Base64)
            .collect(),
        image_response(model_id, response_headers),
    );

    for warning in response.warnings {
        result = result.with_warning(warning);
    }

    if let Some(provider_metadata) = response
        .provider_metadata
        .and_then(gateway_image_provider_metadata)
    {
        result = result.with_provider_metadata(provider_metadata);
    }

    if let Some(usage) = response.usage {
        result = result.with_usage(usage);
    }

    result
}

fn image_result_from_error(
    model_id: &str,
    error: HandledFetchError,
    auth_method: Option<GatewayAuthMethod>,
) -> ImageModelResult {
    let (headers, _) = gateway_error_response_context(&error);
    let gateway_error = as_gateway_error(error, auth_method);

    ImageModelResult::new(Vec::new(), image_response(model_id, headers))
        .with_provider_metadata(gateway_image_error_metadata(&gateway_error))
}

fn image_response(model_id: &str, headers: Option<Headers>) -> ImageModelResponse {
    let mut response = ImageModelResponse::new(OffsetDateTime::now_utc(), model_id);

    if let Some(headers) = headers {
        response = with_image_response_headers(response, headers);
    }

    response
}

fn gateway_image_provider_metadata(value: JsonValue) -> Option<ImageModelProviderMetadata> {
    let object = value.as_object()?;
    let mut metadata = ImageModelProviderMetadata::new();

    for (provider_name, entry_value) in object {
        let Some(entry_object) = entry_value.as_object() else {
            continue;
        };
        let images = entry_object
            .get("images")
            .and_then(JsonValue::as_array)
            .cloned()
            .unwrap_or_default();
        let mut extra = JsonObject::new();

        for (key, value) in entry_object {
            if key != "images" {
                extra.insert(key.clone(), value.clone());
            }
        }

        metadata.insert(
            provider_name.clone(),
            ImageModelProviderMetadataEntry { images, extra },
        );
    }

    Some(metadata)
}

fn gateway_image_error_metadata(error: &GatewayError) -> ImageModelProviderMetadata {
    let mut metadata = ImageModelProviderMetadata::new();
    metadata.insert(
        GATEWAY_PROVIDER_ID.to_string(),
        ImageModelProviderMetadataEntry {
            images: JsonArray::new(),
            extra: gateway_error_metadata_entry(error),
        },
    );
    metadata
}

fn gateway_reranking_request_body(options: &RerankingModelCallOptions) -> JsonValue {
    let mut body = JsonObject::new();
    body.insert(
        "documents".to_string(),
        serde_json::to_value(&options.documents).unwrap_or(JsonValue::Null),
    );
    body.insert(
        "query".to_string(),
        JsonValue::String(options.query.clone()),
    );

    if let Some(top_n) = options.top_n {
        body.insert("topN".to_string(), json!(top_n));
    }

    if let Some(provider_options) = &options.provider_options {
        body.insert(
            "providerOptions".to_string(),
            serde_json::to_value(provider_options).unwrap_or(JsonValue::Null),
        );
    }

    JsonValue::Object(body)
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct GatewayRerankingResponse {
    ranking: Vec<RerankingModelRanking>,
    #[serde(default)]
    provider_metadata: Option<ProviderMetadata>,
}

fn gateway_reranking_response(
    value: &JsonValue,
) -> Result<GatewayRerankingResponse, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn reranking_result_from_response(
    response: GatewayRerankingResponse,
    raw_response: Option<JsonValue>,
    response_headers: Option<Headers>,
) -> RerankingModelResult {
    let mut result = RerankingModelResult::new(response.ranking);
    let mut response_metadata = RerankingModelResponse::new();

    if let Some(body) = raw_response {
        response_metadata = response_metadata.with_body(body);
    }

    if let Some(headers) = response_headers {
        response_metadata = with_reranking_response_headers(response_metadata, headers);
    }

    if let Some(provider_metadata) = response.provider_metadata {
        result = result.with_provider_metadata(provider_metadata);
    }

    result.with_response(response_metadata)
}

fn reranking_result_from_error(
    error: HandledFetchError,
    request_body: JsonValue,
    auth_method: Option<GatewayAuthMethod>,
) -> RerankingModelResult {
    let (headers, body) = gateway_error_response_context(&error);
    let response_body = body
        .as_deref()
        .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
        .or_else(|| body.map(JsonValue::String))
        .unwrap_or(request_body);
    let gateway_error = as_gateway_error(error, auth_method);
    let mut response = RerankingModelResponse::new().with_body(response_body);

    if let Some(headers) = headers {
        response = with_reranking_response_headers(response, headers);
    }

    RerankingModelResult::new(Vec::new())
        .with_provider_metadata(gateway_error_metadata_from_error(&gateway_error))
        .with_response(response)
}

fn gateway_video_request_body(options: &VideoModelCallOptions) -> JsonValue {
    let mut body = JsonObject::new();

    if let Some(prompt) = &options.prompt {
        body.insert("prompt".to_string(), JsonValue::String(prompt.clone()));
    }

    body.insert("n".to_string(), json!(options.n));

    if let Some(aspect_ratio) = &options.aspect_ratio
        && !aspect_ratio.is_empty()
    {
        body.insert(
            "aspectRatio".to_string(),
            JsonValue::String(aspect_ratio.clone()),
        );
    }

    if let Some(resolution) = &options.resolution
        && !resolution.is_empty()
    {
        body.insert(
            "resolution".to_string(),
            JsonValue::String(resolution.clone()),
        );
    }

    if let Some(duration) = options.duration
        && is_non_zero_f64(duration)
    {
        body.insert("duration".to_string(), json!(duration));
    }

    if let Some(fps) = options.fps
        && is_non_zero_f64(fps)
    {
        body.insert("fps".to_string(), json!(fps));
    }

    if let Some(seed) = options.seed
        && seed != 0
    {
        body.insert("seed".to_string(), json!(seed));
    }

    body.insert(
        "providerOptions".to_string(),
        serde_json::to_value(&options.provider_options).unwrap_or(JsonValue::Null),
    );

    if let Some(image) = &options.image {
        body.insert("image".to_string(), gateway_video_file_value(image));
    }

    JsonValue::Object(body)
}

fn gateway_video_file_value(file: &VideoModelFile) -> JsonValue {
    match file {
        VideoModelFile::File {
            media_type,
            data,
            provider_options,
        } => {
            let mut value = JsonObject::new();
            value.insert("type".to_string(), JsonValue::String("file".to_string()));
            value.insert(
                "mediaType".to_string(),
                JsonValue::String(media_type.clone()),
            );
            value.insert(
                "data".to_string(),
                JsonValue::String(convert_to_base64(data)),
            );

            if let Some(provider_options) = provider_options {
                value.insert(
                    "providerOptions".to_string(),
                    serde_json::to_value(provider_options).unwrap_or(JsonValue::Null),
                );
            }

            JsonValue::Object(value)
        }
        VideoModelFile::Url {
            url,
            provider_options,
        } => {
            let mut value = JsonObject::new();
            value.insert("type".to_string(), JsonValue::String("url".to_string()));
            value.insert("url".to_string(), JsonValue::String(url.to_string()));

            if let Some(provider_options) = provider_options {
                value.insert(
                    "providerOptions".to_string(),
                    serde_json::to_value(provider_options).unwrap_or(JsonValue::Null),
                );
            }

            JsonValue::Object(value)
        }
    }
}

fn is_non_zero_f64(value: f64) -> bool {
    value.to_bits() & 0x7fff_ffff_ffff_ffff != 0
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct GatewayVideoResponse {
    videos: Vec<VideoModelVideoData>,
    #[serde(default)]
    warnings: Vec<Warning>,
    #[serde(default)]
    provider_metadata: Option<ProviderMetadata>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type")]
enum GatewayVideoEvent {
    #[serde(rename = "result")]
    Result {
        videos: Vec<VideoModelVideoData>,
        #[serde(default)]
        warnings: Vec<Warning>,
        #[serde(default, rename = "providerMetadata")]
        provider_metadata: Option<ProviderMetadata>,
    },
    #[serde(rename = "error")]
    Error {
        message: String,
        #[serde(rename = "errorType")]
        error_type: String,
        #[serde(rename = "statusCode")]
        status_code: u16,
        #[serde(default)]
        param: Option<JsonValue>,
    },
}

fn gateway_video_event(value: &JsonValue) -> Result<GatewayVideoEvent, serde_json::Error> {
    serde_json::from_value(value.clone())
}

fn gateway_video_response_handler(
    request: &ProviderApiRequest,
    response: &ProviderApiResponse,
) -> Result<ResponseHandlerResult<GatewayVideoResponse>, ProviderApiResponseHandlerError> {
    let stream = create_event_source_response_handler(
        response.event_source_response_handler_options(),
        gateway_video_event,
    )
    .map_err(|_| {
        ProviderApiResponseHandlerError::api_call(gateway_video_api_call_error(
            "SSE response body is empty",
            request,
            response,
        ))
    })?;
    let response_headers = stream.response_headers.clone();
    let Some(event) = stream.value.into_iter().next() else {
        return Err(ProviderApiResponseHandlerError::api_call(
            gateway_video_api_call_error(
                "SSE stream ended without a data event",
                request,
                response,
            ),
        ));
    };

    match event {
        ParseJsonResult::Success { value, raw_value } => match value {
            GatewayVideoEvent::Result {
                videos,
                warnings,
                provider_metadata,
            } => {
                let mut result = ResponseHandlerResult::new(GatewayVideoResponse {
                    videos,
                    warnings,
                    provider_metadata,
                })
                .with_raw_value(raw_value);

                if let Some(headers) = response_headers {
                    result = result.with_response_headers(headers);
                }

                Ok(result)
            }
            GatewayVideoEvent::Error {
                message,
                error_type,
                status_code,
                param,
            } => {
                let response_body = raw_value.to_string();
                let data = json!({
                    "error": {
                        "message": message,
                        "type": error_type,
                        "param": param
                    }
                });

                Err(ProviderApiResponseHandlerError::api_call(
                    ApiCallError::new(
                        data.pointer("/error/message")
                            .and_then(JsonValue::as_str)
                            .unwrap_or("Gateway video model error"),
                        request.url.clone(),
                        request.request_body_values.clone(),
                    )
                    .with_status_code(status_code)
                    .with_response_headers(response.headers.clone())
                    .with_response_body(response_body)
                    .with_data(data),
                ))
            }
        },
        ParseJsonResult::Failure { error, raw_value } => {
            let mut api_error =
                gateway_video_api_call_error("Failed to parse video SSE event", request, response);

            if let Some(raw_value) = raw_value {
                api_error = api_error.with_response_body(raw_value.to_string());
            } else {
                api_error = api_error.with_response_body(error.to_string());
            }

            Err(ProviderApiResponseHandlerError::api_call(api_error))
        }
    }
}

fn gateway_video_api_call_error(
    message: impl Into<String>,
    request: &ProviderApiRequest,
    response: &ProviderApiResponse,
) -> ApiCallError {
    ApiCallError::new(
        message,
        request.url.clone(),
        request.request_body_values.clone(),
    )
    .with_status_code(response.status_code)
    .with_response_headers(response.headers.clone())
}

fn video_result_from_response(
    model_id: &str,
    response: GatewayVideoResponse,
    response_headers: Option<Headers>,
) -> VideoModelResult {
    let mut result =
        VideoModelResult::new(response.videos, video_response(model_id, response_headers));

    for warning in response.warnings {
        result = result.with_warning(warning);
    }

    if let Some(provider_metadata) = response.provider_metadata {
        result = result.with_provider_metadata(provider_metadata);
    }

    result
}

fn video_result_from_error(
    model_id: &str,
    error: HandledFetchError,
    auth_method: Option<GatewayAuthMethod>,
) -> VideoModelResult {
    let (headers, _) = gateway_error_response_context(&error);
    let gateway_error = as_gateway_error(error, auth_method);

    VideoModelResult::new(Vec::new(), video_response(model_id, headers))
        .with_provider_metadata(gateway_error_metadata_from_error(&gateway_error))
}

fn video_response(model_id: &str, headers: Option<Headers>) -> VideoModelResponse {
    let mut response = VideoModelResponse::new(OffsetDateTime::now_utc(), model_id);

    if let Some(headers) = headers {
        response = with_video_response_headers(response, headers);
    }

    response
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
    if let Ok(content) = serde_json::from_value::<LanguageModelContent>(value.clone()) {
        return Some(content);
    }

    let part_type = object.get("type").and_then(JsonValue::as_str)?;

    Some(LanguageModelContent::Custom(
        LanguageModelCustomContent::new(format!("gateway.{part_type}")),
    ))
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

fn gateway_error_response_context(error: &HandledFetchError) -> (Option<Headers>, Option<String>) {
    match error {
        HandledFetchError::Original { .. } => (None, None),
        HandledFetchError::ApiCall { error } => (
            error.response_headers().cloned(),
            error.response_body().map(String::from),
        ),
    }
}

fn gateway_error_metadata_from_error(error: &GatewayError) -> ProviderMetadata {
    let mut metadata = ProviderMetadata::new();
    metadata.insert(
        GATEWAY_PROVIDER_ID.to_string(),
        gateway_error_metadata_entry(error),
    );
    metadata
}

fn gateway_error_metadata_entry(error: &GatewayError) -> JsonObject {
    let mut gateway = JsonObject::new();
    gateway.insert(
        "errorMessage".to_string(),
        JsonValue::String(error.message().to_string()),
    );
    gateway.insert(
        "errorType".to_string(),
        JsonValue::String(error.error_type().to_string()),
    );
    gateway.insert("statusCode".to_string(), json!(error.status_code()));
    gateway.insert("isRetryable".to_string(), json!(error.is_retryable()));

    if let Some(generation_id) = error.generation_id() {
        gateway.insert(
            "generationId".to_string(),
            JsonValue::String(generation_id.to_string()),
        );
    }

    gateway
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

fn gateway_stream_error_from_gateway_error(
    gateway_error: &GatewayError,
    raw_body: Option<&str>,
) -> LanguageModelStreamPart {
    let mut error = JsonObject::new();
    error.insert(
        "message".to_string(),
        JsonValue::String(gateway_error.message().to_string()),
    );
    error.insert(
        "type".to_string(),
        JsonValue::String(gateway_error.error_type().to_string()),
    );
    error.insert("statusCode".to_string(), json!(gateway_error.status_code()));
    error.insert(
        "isRetryable".to_string(),
        json!(gateway_error.is_retryable()),
    );

    if let Some(generation_id) = gateway_error.generation_id() {
        error.insert(
            "generationId".to_string(),
            JsonValue::String(generation_id.to_string()),
        );
    }

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

fn with_embedding_response_headers(
    mut response: EmbeddingModelResponse,
    headers: Headers,
) -> EmbeddingModelResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }

    response
}

fn with_image_response_headers(
    mut response: ImageModelResponse,
    headers: Headers,
) -> ImageModelResponse {
    for (name, value) in headers {
        response = response.with_header(name, value);
    }

    response
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

fn with_video_response_headers(
    mut response: VideoModelResponse,
    headers: Headers,
) -> VideoModelResponse {
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
        DEFAULT_GATEWAY_BASE_URL, GatewayAuthMethod, GatewayCredentialType,
        GatewayGenerationInfoParams, GatewayModelType, GatewayProvider, GatewayProviderOptions,
        GatewayProviderOptionsSort, GatewayProviderSettings, GatewayProviderTimeouts,
        GatewaySpendReportDatePart, GatewaySpendReportGroupBy, GatewaySpendReportParams,
        GatewayTransport, GatewayTransportFuture, create_gateway, gateway, gateway_base_url,
        gateway_observability_headers_with_env, gateway_provider_headers_with_env,
        gateway_provider_options, get_gateway_auth_token_with_env, metadata_cache_refresh_duration,
        try_gateway_provider_options,
    };
    use ai_sdk_provider::{
        EmbeddingModel, EmbeddingModelCallOptions, FileData, FileDataContent, FinishReason,
        ImageModel, ImageModelCallOptions, ImageModelFile, JsonObject, JsonValue, LanguageModel,
        LanguageModelAbortController, LanguageModelCallOptions, LanguageModelContent,
        LanguageModelFileData, LanguageModelFilePart, LanguageModelMessage, LanguageModelSource,
        LanguageModelStreamPart, LanguageModelTextPart, LanguageModelUserContentPart,
        LanguageModelUserMessage, Provider, ProviderMetadata, ProviderWithRerankingModel,
        ProviderWithVideoModel, RerankingModel, RerankingModelCallOptions, RerankingModelDocuments,
        SpecificationVersion, VideoModel, VideoModelCallOptions, VideoModelFile, Warning,
    };
    use ai_sdk_provider_utils::{
        FetchErrorInfo, ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod,
        ProviderApiResponse,
    };
    use serde_json::json;
    use std::env;
    use std::fs;
    use std::future::{Future, ready};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};
    use std::time::{Duration, Instant};
    use url::Url;

    fn env_lookup<'a>(
        values: &'a [(&'a str, &'a str)],
    ) -> impl FnMut(&str) -> Result<String, env::VarError> + 'a {
        move |name| {
            values
                .iter()
                .find_map(|(key, value)| (*key == name).then(|| (*value).to_string()))
                .ok_or(env::VarError::NotPresent)
        }
    }

    fn json_object(value: JsonValue) -> JsonObject {
        value.as_object().cloned().expect("JSON value is an object")
    }

    fn metadata_response_for_model(model_id: &str) -> String {
        json!({
            "models": [{
                "id": model_id,
                "name": "Test Model",
                "specification": {
                    "specificationVersion": "v4",
                    "provider": "gateway",
                    "modelId": model_id
                },
                "modelType": "language"
            }]
        })
        .to_string()
    }

    fn counting_metadata_transport(request_count: Arc<Mutex<u32>>) -> GatewayTransport {
        Arc::new(move |_request| -> GatewayTransportFuture {
            let mut count = request_count
                .lock()
                .expect("request count mutex is not poisoned");
            *count += 1;
            let model_id = format!("model-{}", *count);

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                metadata_response_for_model(&model_id),
            ))))
        })
    }

    fn assert_request_tracks_abort_signal(
        request: &ProviderApiRequest,
        abort_controller: &LanguageModelAbortController,
    ) {
        let request_signal = request.abort_signal.clone().expect("abort signal set");
        assert!(!request_signal.is_aborted());

        abort_controller.abort_with_reason("client-disconnected");
        assert!(request_signal.is_aborted());
        assert_eq!(request_signal.reason(), Some(json!("client-disconnected")));
    }

    #[test]
    fn get_gateway_auth_token_matches_upstream_precedence() {
        let token = get_gateway_auth_token_with_env(
            &GatewayProviderSettings::new().with_api_key("options-api-key"),
            env_lookup(&[
                ("AI_GATEWAY_API_KEY", "env-api-key"),
                ("AI_SDK_RUST_AI_GATEWAY_API_KEY", "rust-env-api-key"),
                ("VERCEL_OIDC_TOKEN", "oidc-token"),
            ]),
        )
        .expect("options API key resolves");

        assert_eq!(token.auth_method, GatewayAuthMethod::ApiKey);
        assert_eq!(token.token, "options-api-key");

        let token = get_gateway_auth_token_with_env(
            &GatewayProviderSettings::new(),
            env_lookup(&[
                ("AI_GATEWAY_API_KEY", "env-api-key"),
                ("AI_SDK_RUST_AI_GATEWAY_API_KEY", "rust-env-api-key"),
                ("VERCEL_OIDC_TOKEN", "oidc-token"),
            ]),
        )
        .expect("environment API key resolves");

        assert_eq!(token.auth_method, GatewayAuthMethod::ApiKey);
        assert_eq!(token.token, "env-api-key");

        let token = get_gateway_auth_token_with_env(
            &GatewayProviderSettings::new(),
            env_lookup(&[("VERCEL_OIDC_TOKEN", "oidc-token")]),
        )
        .expect("OIDC token resolves when API keys are absent");

        assert_eq!(token.auth_method, GatewayAuthMethod::Oidc);
        assert_eq!(token.token, "oidc-token");
    }

    #[test]
    fn get_gateway_auth_token_ignores_empty_values_without_trimming_whitespace() {
        let error = get_gateway_auth_token_with_env(
            &GatewayProviderSettings::new().with_api_key(""),
            env_lookup(&[
                ("AI_GATEWAY_API_KEY", ""),
                ("AI_SDK_RUST_AI_GATEWAY_API_KEY", ""),
                ("VERCEL_OIDC_TOKEN", ""),
            ]),
        )
        .expect_err("empty credentials are ignored");

        assert!(error.as_authentication().is_some());
        assert!(error.message().contains("No authentication provided"));

        let token = get_gateway_auth_token_with_env(
            &GatewayProviderSettings::new(),
            env_lookup(&[("AI_GATEWAY_API_KEY", "\t\n ")]),
        )
        .expect("whitespace-only API keys match upstream truthiness");

        assert_eq!(token.auth_method, GatewayAuthMethod::ApiKey);
        assert_eq!(token.token, "\t\n ");
    }

    #[test]
    fn gateway_provider_headers_support_oidc_auth_method() {
        let headers = gateway_provider_headers_with_env(
            &GatewayProviderSettings::new().with_header("custom-header", "value"),
            env_lookup(&[("VERCEL_OIDC_TOKEN", "oidc-token")]),
        );

        assert_eq!(
            headers.get("authorization").and_then(Option::as_deref),
            Some("Bearer oidc-token")
        );
        assert_eq!(
            headers
                .get("ai-gateway-auth-method")
                .and_then(Option::as_deref),
            Some("oidc")
        );
        assert_eq!(
            headers.get("custom-header").and_then(Option::as_deref),
            Some("value")
        );
    }

    #[test]
    fn gateway_observability_headers_map_vercel_environment() {
        let headers = gateway_observability_headers_with_env(
            &GatewayProviderSettings::new().with_vercel_request_id("req_settings"),
            env_lookup(&[
                ("VERCEL_DEPLOYMENT_ID", "dpl_test"),
                ("VERCEL_ENV", "production"),
                ("VERCEL_REGION", "iad1"),
                ("VERCEL_PROJECT_ID", "prj_test"),
                ("VERCEL_REQUEST_ID", "req_env"),
            ]),
        );

        assert_eq!(
            headers.get("ai-o11y-deployment-id").map(String::as_str),
            Some("dpl_test")
        );
        assert_eq!(
            headers.get("ai-o11y-environment").map(String::as_str),
            Some("production")
        );
        assert_eq!(
            headers.get("ai-o11y-region").map(String::as_str),
            Some("iad1")
        );
        assert_eq!(
            headers.get("ai-o11y-project-id").map(String::as_str),
            Some("prj_test")
        );
        assert_eq!(
            headers.get("ai-o11y-request-id").map(String::as_str),
            Some("req_settings")
        );
    }

    #[test]
    fn gateway_observability_headers_skip_empty_values_and_use_request_env_fallback() {
        let headers = gateway_observability_headers_with_env(
            &GatewayProviderSettings::new(),
            env_lookup(&[
                ("VERCEL_DEPLOYMENT_ID", ""),
                ("VERCEL_ENV", "preview"),
                ("VERCEL_REGION", ""),
                ("X_VERCEL_ID", "iad1::req_env"),
            ]),
        );

        assert!(!headers.contains_key("ai-o11y-deployment-id"));
        assert_eq!(
            headers.get("ai-o11y-environment").map(String::as_str),
            Some("preview")
        );
        assert!(!headers.contains_key("ai-o11y-region"));
        assert_eq!(
            headers.get("ai-o11y-request-id").map(String::as_str),
            Some("iad1::req_env")
        );
    }

    #[test]
    fn gateway_provider_options_serialize_upstream_shape() {
        let options = GatewayProviderOptions::new()
            .with_only(["azure", "openai"])
            .with_order(["bedrock", "anthropic"])
            .with_sort(GatewayProviderOptionsSort::Ttft)
            .with_user("user-123")
            .with_tags(["chat", "v2"])
            .with_models(["openai/gpt-5-nano", "zai/glm-4.6"])
            .with_byok_credentials(
                "anthropic",
                [json_object(json!({
                    "apiKey": "test-anthropic-key"
                }))],
            )
            .with_byok_credentials(
                "vertex",
                [
                    json_object(json!({
                        "projectId": "project-1",
                        "privateKey": "private-key-1"
                    })),
                    json_object(json!({
                        "projectId": "project-2",
                        "privateKey": "private-key-2"
                    })),
                ],
            )
            .with_zero_data_retention(true)
            .with_disallow_prompt_training(true)
            .with_hipaa_compliant(true)
            .with_quota_entity_id("entity-123")
            .with_provider_timeouts(
                GatewayProviderTimeouts::new()
                    .with_byok_timeout("openai", 5000)
                    .with_byok_timeout("anthropic", 2000),
            );
        let expected = json!({
            "only": ["azure", "openai"],
            "order": ["bedrock", "anthropic"],
            "sort": "ttft",
            "user": "user-123",
            "tags": ["chat", "v2"],
            "models": ["openai/gpt-5-nano", "zai/glm-4.6"],
            "byok": {
                "anthropic": [
                    {
                        "apiKey": "test-anthropic-key"
                    }
                ],
                "vertex": [
                    {
                        "projectId": "project-1",
                        "privateKey": "private-key-1"
                    },
                    {
                        "projectId": "project-2",
                        "privateKey": "private-key-2"
                    }
                ]
            },
            "zeroDataRetention": true,
            "disallowPromptTraining": true,
            "hipaaCompliant": true,
            "quotaEntityId": "entity-123",
            "providerTimeouts": {
                "byok": {
                    "anthropic": 2000,
                    "openai": 5000
                }
            }
        });

        assert_eq!(
            serde_json::to_value(&options).expect("options serialize"),
            expected
        );
        assert_eq!(
            gateway_provider_options(options).get("gateway"),
            expected.as_object()
        );
    }

    #[test]
    fn gateway_provider_options_validation_matches_timeout_schema() {
        let valid_timeouts = GatewayProviderTimeouts::new()
            .try_with_byok_timeout("openai", 1000)
            .expect("minimum timeout is valid")
            .try_with_byok_timeout("anthropic", 2000)
            .expect("larger timeout is valid");
        let valid_options =
            GatewayProviderOptions::new().with_provider_timeouts(valid_timeouts.clone());

        valid_timeouts.validate().expect("valid timeout map passes");
        valid_options.validate().expect("valid options pass");
        assert_eq!(
            try_gateway_provider_options(valid_options)
                .expect("validated provider options convert")
                .get("gateway")
                .and_then(|options| options.get("providerTimeouts"))
                .and_then(|timeouts| timeouts.get("byok"))
                .and_then(|byok| byok.get("openai"))
                .and_then(JsonValue::as_u64),
            Some(1000)
        );

        let direct_error = GatewayProviderTimeouts::new()
            .try_with_byok_timeout("openai", 999)
            .expect_err("timeout below the upstream minimum is rejected");
        assert!(
            direct_error
                .message()
                .contains("at least 1000 milliseconds")
        );

        let invalid_options = GatewayProviderOptions::new().with_provider_timeouts(
            GatewayProviderTimeouts::new().with_byok_timeout("openai", 999),
        );
        let validation_error = invalid_options
            .validate()
            .expect_err("invalid timeout map is rejected");
        assert_eq!(
            validation_error.to_string(),
            "Gateway providerTimeouts.byok.openai must be at least 1000 milliseconds"
        );

        let conversion_error = try_gateway_provider_options(invalid_options)
            .expect_err("invalid provider options do not convert through the checked helper");
        assert_eq!(conversion_error, validation_error);
    }

    #[test]
    fn gateway_model_passes_typed_gateway_provider_options_for_generate() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let abort_controller = LanguageModelAbortController::new();
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
                        "text": "ok"
                    },
                    "finish_reason": "stop",
                    "usage": {
                        "prompt_tokens": 1,
                        "completion_tokens": 1
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

        let result = poll_ready(
            model.do_generate(
                LanguageModelCallOptions::new(Vec::new())
                    .with_provider_options(
                        GatewayProviderOptions::new()
                            .with_order(["bedrock", "anthropic"])
                            .with_zero_data_retention(true)
                            .with_provider_timeouts(
                                GatewayProviderTimeouts::new().with_byok_timeout("openai", 5000),
                            )
                            .into_provider_options(),
                    )
                    .with_abort_signal(abort_controller.signal()),
            ),
        );
        assert_eq!(result.finish_reason.unified, FinishReason::Stop);

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let request_signal = request.abort_signal.clone().expect("abort signal set");
        abort_controller.abort_with_reason("client-disconnected");
        assert!(request_signal.is_aborted());
        assert_eq!(request_signal.reason(), Some(json!("client-disconnected")));

        let request_body = request
            .body
            .and_then(|body| body.as_text().map(str::to_string))
            .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body.get("providerOptions"),
            Some(&json!({
                "gateway": {
                    "order": ["bedrock", "anthropic"],
                    "zeroDataRetention": true,
                    "providerTimeouts": {
                        "byok": {
                            "openai": 5000
                        }
                    }
                }
            }))
        );
    }

    #[test]
    fn gateway_model_passes_typed_gateway_provider_options_for_stream() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let abort_controller = LanguageModelAbortController::new();
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                format!(
                    "data: {}\n\n",
                    json!({
                        "type": "finish",
                        "finishReason": {
                            "unified": "stop",
                            "raw": "stop"
                        },
                        "usage": {
                            "inputTokens": {
                                "total": 1
                            },
                            "outputTokens": {
                                "total": 1
                            }
                        }
                    })
                ),
            ))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");

        let result = poll_ready(
            model.do_stream(
                LanguageModelCallOptions::new(Vec::new())
                    .with_provider_options(
                        GatewayProviderOptions::new()
                            .with_order(["groq", "openai"])
                            .with_quota_entity_id("entity-123")
                            .into_provider_options(),
                    )
                    .with_abort_signal(abort_controller.signal()),
            ),
        );
        assert!(matches!(
            result.stream.last(),
            Some(LanguageModelStreamPart::Finish(_))
        ));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let request_signal = request.abort_signal.clone().expect("abort signal set");
        abort_controller.abort_with_reason("client-disconnected");
        assert!(request_signal.is_aborted());
        assert_eq!(request_signal.reason(), Some(json!("client-disconnected")));

        let request_body = request
            .body
            .and_then(|body| body.as_text().map(str::to_string))
            .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body.get("providerOptions"),
            Some(&json!({
                "gateway": {
                    "order": ["groq", "openai"],
                    "quotaEntityId": "entity-123"
                }
            }))
        );
    }

    #[test]
    fn gateway_embedding_model_passes_abort_signal_to_provider_api_request() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let abort_controller = LanguageModelAbortController::new();
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "embeddings": [[0.1, 0.2]]
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
        .embedding_model("openai/text-embedding-3-small");

        let result = poll_ready(
            model.do_embed(
                EmbeddingModelCallOptions::new(vec!["hello".to_string()])
                    .with_abort_signal(abort_controller.signal()),
            ),
        );

        assert_eq!(result.embeddings, vec![vec![0.1, 0.2]]);
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_request_tracks_abort_signal(&request, &abort_controller);
    }

    #[test]
    fn gateway_image_model_passes_abort_signal_to_provider_api_request() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let abort_controller = LanguageModelAbortController::new();
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "images": ["base64-image"]
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
        .image_model("openai/gpt-image-1");

        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("A red cube")
                    .with_abort_signal(abort_controller.signal()),
            ),
        );

        assert_eq!(result.images.len(), 1);
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_request_tracks_abort_signal(&request, &abort_controller);
    }

    #[test]
    fn gateway_reranking_model_passes_abort_signal_to_provider_api_request() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let abort_controller = LanguageModelAbortController::new();
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "ranking": [
                        {
                            "index": 0,
                            "relevanceScore": 0.9
                        }
                    ]
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
        .reranking_model("cohere/rerank-v3.5");

        let result = poll_ready(
            model.do_rerank(
                RerankingModelCallOptions::new(
                    RerankingModelDocuments::text(vec!["one".to_string(), "two".to_string()]),
                    "one",
                )
                .with_abort_signal(abort_controller.signal()),
            ),
        );

        assert_eq!(result.ranking.len(), 1);
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_request_tracks_abort_signal(&request, &abort_controller);
    }

    #[test]
    fn gateway_video_model_passes_abort_signal_to_provider_api_request() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let abort_controller = LanguageModelAbortController::new();
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                format!(
                    "data: {}\n\n",
                    json!({
                        "type": "result",
                        "videos": [
                            {
                                "type": "base64",
                                "data": "AAAAIGZ0eXBtcDQy",
                                "mediaType": "video/mp4"
                            }
                        ]
                    })
                ),
            ))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .video_model("google/veo-2.0-generate-001");

        let result = poll_ready(
            model.do_generate(
                VideoModelCallOptions::new(1)
                    .with_prompt("Animate this")
                    .with_abort_signal(abort_controller.signal()),
            ),
        );

        assert_eq!(result.videos.len(), 1);
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_request_tracks_abort_signal(&request, &abort_controller);
    }

    #[test]
    fn gateway_model_maps_standard_generate_content_parts() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "id": "test-id",
                    "created": 1711115037,
                    "model": "openai/gpt-4.1-mini",
                    "content": [
                        {
                            "type": "text",
                            "text": "Summary",
                            "providerMetadata": {
                                "gateway": {
                                    "text": true
                                }
                            }
                        },
                        {
                            "type": "reasoning",
                            "text": "Need search context."
                        },
                        {
                            "type": "source",
                            "sourceType": "url",
                            "id": "src_1",
                            "url": "https://example.com/source",
                            "title": "Example Source"
                        },
                        {
                            "type": "file",
                            "mediaType": "text/plain",
                            "data": {
                                "type": "data",
                                "data": "ZGF0YQ=="
                            }
                        },
                        {
                            "type": "tool-result",
                            "toolCallId": "call_1",
                            "toolName": "search",
                            "result": {
                                "status": "ok"
                            }
                        },
                        {
                            "type": "custom",
                            "kind": "gateway.provider-annotation"
                        }
                    ],
                    "finish_reason": "stop",
                    "usage": {
                        "prompt_tokens": 4,
                        "completion_tokens": 3
                    },
                    "providerMetadata": {
                        "gateway": {
                            "generationId": "gen_123"
                        }
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
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(vec![
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Summarize")),
            ])),
        ])));

        assert_eq!(result.finish_reason.unified, FinishReason::Stop);
        assert_eq!(result.content.len(), 6);
        assert!(matches!(
            &result.content[0],
            LanguageModelContent::Text(text)
                if text.text == "Summary"
                    && text
                        .provider_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("gateway"))
                        .and_then(|metadata| metadata.get("text"))
                        .and_then(JsonValue::as_bool)
                        == Some(true)
        ));
        assert!(matches!(
            &result.content[1],
            LanguageModelContent::Reasoning(reasoning)
                if reasoning.text == "Need search context."
        ));
        assert!(matches!(
            &result.content[2],
            LanguageModelContent::Source(LanguageModelSource::Url(source))
                if source.id == "src_1"
                    && source.url == "https://example.com/source"
                    && source.title.as_deref() == Some("Example Source")
        ));
        assert!(matches!(
            &result.content[3],
            LanguageModelContent::File(file)
                if file.media_type == "text/plain"
                    && matches!(
                        &file.data,
                        LanguageModelFileData::Data { data }
                            if serde_json::to_value(data)
                                .expect("file data serializes")
                                == json!("ZGF0YQ==")
                    )
        ));
        assert!(matches!(
            &result.content[4],
            LanguageModelContent::ToolResult(tool_result)
                if tool_result.tool_call_id == "call_1"
                    && tool_result.tool_name == "search"
                    && tool_result.result.as_value() == &json!({ "status": "ok" })
        ));
        assert!(matches!(
            &result.content[5],
            LanguageModelContent::Custom(custom)
                if custom.kind == "gateway.provider-annotation"
        ));
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("generationId"))
                .and_then(JsonValue::as_str),
            Some("gen_123")
        );
    }

    #[test]
    fn gateway_model_encodes_language_prompt_file_bytes_for_generate() {
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
                        "text": "ok"
                    },
                    "finish_reason": "stop",
                    "usage": {
                        "prompt_tokens": 1,
                        "completion_tokens": 1
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
        let prompt = vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("First text.")),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Bytes(vec![1, 2, 3, 4]),
                    },
                    "image/gif",
                )),
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Second text.")),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Url {
                        url: Url::parse("https://example.com/image2.png").expect("valid URL"),
                    },
                    "image/png",
                )),
                LanguageModelUserContentPart::File(LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Base64("already-base64".to_string()),
                    },
                    "image/jpeg",
                )),
            ],
        ))];

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(prompt)));
        assert_eq!(result.finish_reason.unified, FinishReason::Stop);

        let request_body = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured")
            .body
            .and_then(|body| body.as_text().map(str::to_string))
            .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body
                .get("prompt")
                .and_then(JsonValue::as_array)
                .and_then(|messages| messages.first())
                .and_then(|message| message.get("content")),
            Some(&json!([
                {
                    "type": "text",
                    "text": "First text."
                },
                {
                    "type": "file",
                    "data": {
                        "type": "data",
                        "data": "AQIDBA=="
                    },
                    "mediaType": "image/gif"
                },
                {
                    "type": "text",
                    "text": "Second text."
                },
                {
                    "type": "file",
                    "data": {
                        "type": "url",
                        "url": "https://example.com/image2.png"
                    },
                    "mediaType": "image/png"
                },
                {
                    "type": "file",
                    "data": {
                        "type": "data",
                        "data": "already-base64"
                    },
                    "mediaType": "image/jpeg"
                }
            ]))
        );
    }

    #[test]
    fn gateway_model_encodes_language_prompt_file_bytes_for_stream() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                format!(
                    "data: {}\n\n",
                    json!({
                        "type": "finish",
                        "finishReason": {
                            "unified": "stop",
                            "raw": "stop"
                        },
                        "usage": {
                            "inputTokens": {
                                "total": 1
                            },
                            "outputTokens": {
                                "total": 1
                            }
                        }
                    })
                ),
            ))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");
        let prompt = vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![LanguageModelUserContentPart::File(
                LanguageModelFilePart::new(
                    FileData::Data {
                        data: FileDataContent::Bytes(vec![5, 6, 7, 8]),
                    },
                    "image/png",
                ),
            )],
        ))];

        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(prompt)));
        assert!(matches!(
            result.stream.last(),
            Some(LanguageModelStreamPart::Finish(_))
        ));

        let request_body = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured")
            .body
            .and_then(|body| body.as_text().map(str::to_string))
            .and_then(|body| serde_json::from_str::<JsonValue>(&body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body
                .get("prompt")
                .and_then(JsonValue::as_array)
                .and_then(|messages| messages.first())
                .and_then(|message| message.get("content"))
                .and_then(JsonValue::as_array)
                .and_then(|parts| parts.first())
                .and_then(|part| part.get("data"))
                .and_then(|data| data.get("data"))
                .and_then(JsonValue::as_str),
            Some("BQYHCA==")
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
        let result = poll_ready(
            model.do_generate(ai_sdk_provider::LanguageModelCallOptions::new(Vec::new())),
        );

        assert_eq!(result.content, Vec::<LanguageModelContent>::new());
        assert_eq!(result.finish_reason.unified, FinishReason::Error);
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str)
                .map(|message| message.contains("Invalid API key")),
            Some(true)
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("errorType"))
                .and_then(JsonValue::as_str),
            Some("authentication_error")
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("statusCode"))
                .and_then(JsonValue::as_u64),
            Some(401)
        );
    }

    #[test]
    fn gateway_model_preserves_structured_gateway_error_metadata() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                400,
                "Bad Request",
                json!({
                    "error": {
                        "message": "Invalid prompt",
                        "type": "invalid_request_error"
                    },
                    "generationId": "gen_error_123"
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
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(Vec::new())));
        let gateway_metadata = result
            .provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("gateway"))
            .expect("Gateway error metadata is present");

        assert_eq!(
            gateway_metadata
                .get("errorMessage")
                .and_then(JsonValue::as_str),
            Some("Invalid prompt [gen_error_123]")
        );
        assert_eq!(
            gateway_metadata
                .get("errorType")
                .and_then(JsonValue::as_str),
            Some("invalid_request_error")
        );
        assert_eq!(
            gateway_metadata
                .get("statusCode")
                .and_then(JsonValue::as_u64),
            Some(400)
        );
        assert_eq!(
            gateway_metadata
                .get("isRetryable")
                .and_then(JsonValue::as_bool),
            Some(false)
        );
        assert_eq!(
            gateway_metadata
                .get("generationId")
                .and_then(JsonValue::as_str),
            Some("gen_error_123")
        );
    }

    #[test]
    fn gateway_model_classifies_transport_timeout_errors() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Err(
                FetchErrorInfo::new("headers timed out").with_code("UND_ERR_HEADERS_TIMEOUT")
            )))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");
        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(Vec::new())));
        let gateway_metadata = result
            .provider_metadata
            .as_ref()
            .and_then(|metadata| metadata.get("gateway"))
            .expect("Gateway error metadata is present");

        assert_eq!(result.finish_reason.unified, FinishReason::Error);
        assert!(
            gateway_metadata
                .get("errorMessage")
                .and_then(JsonValue::as_str)
                .is_some_and(|message| message.contains("headers timed out"))
        );
        assert_eq!(
            gateway_metadata
                .get("errorType")
                .and_then(JsonValue::as_str),
            Some("timeout_error")
        );
        assert_eq!(
            gateway_metadata
                .get("statusCode")
                .and_then(JsonValue::as_u64),
            Some(408)
        );
        assert_eq!(
            gateway_metadata
                .get("isRetryable")
                .and_then(JsonValue::as_bool),
            Some(true)
        );
    }

    #[test]
    fn gateway_model_stream_classifies_transport_timeout_errors() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Err(
                FetchErrorInfo::new("body timed out").with_code("UND_ERR_BODY_TIMEOUT")
            )))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .language_model("openai/gpt-4.1-mini");
        let result = poll_ready(model.do_stream(LanguageModelCallOptions::new(Vec::new())));
        let LanguageModelStreamPart::Error(error_part) = result
            .stream
            .first()
            .expect("stream contains an error part")
        else {
            panic!("first stream part should be an error");
        };

        assert!(
            error_part
                .error
                .get("message")
                .and_then(JsonValue::as_str)
                .is_some_and(|message| message.contains("body timed out"))
        );
        assert_eq!(
            error_part.error.get("type").and_then(JsonValue::as_str),
            Some("timeout_error")
        );
        assert_eq!(
            error_part
                .error
                .get("statusCode")
                .and_then(JsonValue::as_u64),
            Some(408)
        );
        assert_eq!(
            error_part
                .error
                .get("isRetryable")
                .and_then(JsonValue::as_bool),
            Some(true)
        );
    }

    #[test]
    fn gateway_embedding_model_maps_gateway_error_to_metadata() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                400,
                "Bad Request",
                json!({
                    "error": {
                        "message": "Invalid input",
                        "type": "invalid_request_error"
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
        .embedding_model("openai/text-embedding-3-small");
        let result = poll_ready(model.do_embed(EmbeddingModelCallOptions::new(vec![
            "bad input".to_string(),
        ])));

        assert!(result.embeddings.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("Invalid input")
        );
    }

    #[test]
    fn gateway_image_model_preserves_metadata_entries_without_images() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "images": ["base64-image"],
                    "providerMetadata": {
                        "openai": {
                            "quality": "high",
                            "nested": {
                                "revisedPrompt": "A brighter cube"
                            }
                        },
                        "gateway": {
                            "routing": {
                                "provider": "openai"
                            }
                        }
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
        .image_model("openai/gpt-image-1");
        let result =
            poll_ready(model.do_generate(ImageModelCallOptions::new(1).with_prompt("A red cube")));
        let metadata = result
            .provider_metadata
            .expect("provider metadata is preserved");
        let openai_metadata = metadata.get("openai").expect("OpenAI metadata exists");
        let gateway_metadata = metadata.get("gateway").expect("Gateway metadata exists");

        assert_eq!(openai_metadata.images, Vec::<JsonValue>::new());
        assert_eq!(
            openai_metadata
                .extra
                .get("quality")
                .and_then(JsonValue::as_str),
            Some("high")
        );
        assert_eq!(
            openai_metadata
                .extra
                .get("nested")
                .and_then(|metadata| metadata.get("revisedPrompt"))
                .and_then(JsonValue::as_str),
            Some("A brighter cube")
        );
        assert_eq!(gateway_metadata.images, Vec::<JsonValue>::new());
        assert_eq!(
            gateway_metadata
                .extra
                .get("routing")
                .and_then(|metadata| metadata.get("provider"))
                .and_then(JsonValue::as_str),
            Some("openai")
        );
    }

    #[test]
    fn gateway_image_model_maps_upstream_request_response_and_metadata() {
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
                    "images": ["base64-1", "base64-2"],
                    "warnings": [
                        {
                            "type": "unsupported",
                            "feature": "size",
                            "details": "Use aspectRatio instead."
                        },
                        {
                            "type": "compatibility",
                            "feature": "seed",
                            "details": "Seed support is approximate."
                        },
                        {
                            "type": "other",
                            "message": "Rate limit approaching."
                        }
                    ],
                    "usage": {
                        "inputTokens": 27,
                        "outputTokens": 6240,
                        "totalTokens": 6267
                    },
                    "providerMetadata": {
                        "vertex": {
                            "images": [
                                { "revisedPrompt": "Revised 1" },
                                { "revisedPrompt": "Revised 2" }
                            ],
                            "usage": { "tokens": 150 }
                        },
                        "gateway": {
                            "routing": {
                                "provider": "vertex",
                                "attempts": [
                                    { "provider": "openai", "success": false },
                                    { "provider": "vertex", "success": true }
                                ]
                            },
                            "generationId": "gen-xyz-789"
                        }
                    }
                })
                .to_string(),
            )
            .with_headers(std::collections::BTreeMap::from([(
                "x-request-id".to_string(),
                "req_image_123".to_string(),
            )])))))
        });
        let provider_options: ProviderMetadata = serde_json::from_value(json!({
            "vertex": {
                "safetySettings": "block_none"
            },
            "openai": {
                "style": "vivid"
            }
        }))
        .expect("provider options deserialize");
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token")
                .with_header("x-provider-header", "provider-value"),
        )
        .with_transport(transport)
        .image_model("google/imagen-4.0-generate");
        let bfl_model = GatewayProvider::new().image_model("bfl/flux-pro-1.1");

        assert_eq!(model.provider(), "gateway");
        assert_eq!(model.model_id(), "google/imagen-4.0-generate");
        assert_eq!(model.specification_version(), SpecificationVersion::V4);
        assert_eq!(poll_ready(model.max_images_per_call()), Some(usize::MAX));
        assert_eq!(
            poll_ready(bfl_model.max_images_per_call()),
            Some(usize::MAX)
        );

        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(2)
                    .with_prompt("A cat playing piano")
                    .with_size("1024x1024")
                    .with_aspect_ratio("16:9")
                    .with_seed(42)
                    .with_header("x-call-header", "call-value")
                    .with_provider_options(provider_options),
            ),
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.test.com/image-model");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-token")
        );
        assert_eq!(
            request.headers.get("ai-model-id").map(String::as_str),
            Some("google/imagen-4.0-generate")
        );
        assert_eq!(
            request
                .headers
                .get("ai-image-model-specification-version")
                .map(String::as_str),
            Some("4")
        );
        assert_eq!(
            request.headers.get("x-provider-header").map(String::as_str),
            Some("provider-value")
        );
        assert_eq!(
            request.headers.get("x-call-header").map(String::as_str),
            Some("call-value")
        );

        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");
        assert_eq!(
            request_body,
            json!({
                "prompt": "A cat playing piano",
                "n": 2,
                "size": "1024x1024",
                "aspectRatio": "16:9",
                "seed": 42,
                "providerOptions": {
                    "openai": {
                        "style": "vivid"
                    },
                    "vertex": {
                        "safetySettings": "block_none"
                    }
                }
            })
        );

        assert_eq!(
            result.images,
            vec![
                FileDataContent::Base64("base64-1".to_string()),
                FileDataContent::Base64("base64-2".to_string())
            ]
        );
        assert_eq!(
            result.warnings,
            vec![
                Warning::Unsupported {
                    feature: "size".to_string(),
                    details: Some("Use aspectRatio instead.".to_string())
                },
                Warning::Compatibility {
                    feature: "seed".to_string(),
                    details: Some("Seed support is approximate.".to_string())
                },
                Warning::Other {
                    message: "Rate limit approaching.".to_string()
                }
            ]
        );
        let usage = result.usage.expect("usage is preserved");
        assert_eq!(usage.input_tokens, Some(27));
        assert_eq!(usage.output_tokens, Some(6240));
        assert_eq!(usage.total_tokens, Some(6267));
        assert_eq!(result.response.model_id, "google/imagen-4.0-generate");
        assert_eq!(
            result
                .response
                .headers
                .as_ref()
                .and_then(|headers| headers.get("x-request-id"))
                .map(String::as_str),
            Some("req_image_123")
        );
        let metadata = result
            .provider_metadata
            .expect("provider metadata is preserved");
        assert_eq!(
            metadata
                .get("vertex")
                .and_then(|entry| entry.images.first())
                .and_then(|image| image.get("revisedPrompt"))
                .and_then(JsonValue::as_str),
            Some("Revised 1")
        );
        assert_eq!(
            metadata
                .get("vertex")
                .and_then(|entry| entry.extra.get("usage"))
                .and_then(|usage| usage.get("tokens"))
                .and_then(JsonValue::as_u64),
            Some(150)
        );
        assert_eq!(
            metadata
                .get("gateway")
                .and_then(|entry| entry.extra.get("routing"))
                .and_then(|routing| routing.get("provider"))
                .and_then(JsonValue::as_str),
            Some("vertex")
        );
        assert_eq!(
            metadata
                .get("gateway")
                .and_then(|entry| entry.extra.get("generationId"))
                .and_then(JsonValue::as_str),
            Some("gen-xyz-789")
        );
    }

    #[test]
    fn gateway_image_model_encodes_files_and_mask() {
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
                    "images": ["base64-image"]
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
        .image_model("openai/gpt-image-1");
        let file_options: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "quality": "hd"
            }
        }))
        .expect("provider metadata deserialize");

        let result = poll_ready(
            model.do_generate(
                ImageModelCallOptions::new(1)
                    .with_prompt("Edit these images")
                    .with_files(vec![
                        ImageModelFile::file(
                            "image/png",
                            FileDataContent::Bytes(b"Hello".to_vec()),
                        )
                        .with_provider_options(file_options),
                        ImageModelFile::file(
                            "image/jpeg",
                            FileDataContent::Base64("already-encoded".to_string()),
                        ),
                        ImageModelFile::url(
                            url::Url::parse("https://example.com/image.png").expect("URL is valid"),
                        ),
                    ])
                    .with_mask(ImageModelFile::file(
                        "image/png",
                        FileDataContent::Bytes(vec![4, 5, 6]),
                    )),
            ),
        );

        assert_eq!(result.images.len(), 1);
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");

        assert_eq!(
            request_body
                .pointer("/files/0/data")
                .and_then(JsonValue::as_str),
            Some("SGVsbG8=")
        );
        assert_eq!(
            request_body
                .pointer("/files/0/providerOptions/openai/quality")
                .and_then(JsonValue::as_str),
            Some("hd")
        );
        assert_eq!(
            request_body
                .pointer("/files/1/data")
                .and_then(JsonValue::as_str),
            Some("already-encoded")
        );
        assert_eq!(
            request_body
                .pointer("/files/2/url")
                .and_then(JsonValue::as_str),
            Some("https://example.com/image.png")
        );
        assert_eq!(
            request_body
                .pointer("/mask/data")
                .and_then(JsonValue::as_str),
            Some("BAUG")
        );
    }

    #[test]
    fn gateway_image_model_maps_gateway_error_to_metadata() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                400,
                "Bad Request",
                json!({
                    "error": {
                        "message": "Invalid image prompt",
                        "type": "invalid_request_error"
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
        .image_model("openai/gpt-image-1");
        let result =
            poll_ready(model.do_generate(ImageModelCallOptions::new(1).with_prompt("bad prompt")));

        assert!(result.images.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.extra.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("Invalid image prompt")
        );
    }

    #[test]
    fn gateway_image_model_maps_partial_and_missing_usage() {
        let call_count = Arc::new(Mutex::new(0usize));
        let call_count_for_transport = Arc::clone(&call_count);
        let transport: GatewayTransport = Arc::new(move |_request| -> GatewayTransportFuture {
            let mut call_count = call_count_for_transport
                .lock()
                .expect("call count mutex is not poisoned");
            *call_count += 1;
            let response_body = match *call_count {
                1 => json!({
                    "images": ["iVBORw0KGgo="],
                    "usage": {
                        "inputTokens": 10
                    }
                }),
                _ => json!({
                    "images": ["iVBORw0KGgo="]
                }),
            };

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                response_body.to_string(),
            ))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .image_model("openai/gpt-image-1");

        let partial_usage =
            poll_ready(model.do_generate(ImageModelCallOptions::new(1).with_prompt("Partial")));
        let usage = partial_usage.usage.expect("partial usage is preserved");
        assert_eq!(usage.input_tokens, Some(10));
        assert_eq!(usage.output_tokens, None);
        assert_eq!(usage.total_tokens, None);

        let missing_usage =
            poll_ready(model.do_generate(ImageModelCallOptions::new(1).with_prompt("Missing")));
        assert_eq!(missing_usage.usage, None);
    }

    #[test]
    fn gateway_image_model_preserves_warning_variants() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "images": ["iVBORw0KGgo="],
                    "warnings": [
                        {
                            "type": "unsupported",
                            "feature": "size"
                        },
                        {
                            "type": "compatibility",
                            "feature": "seed",
                            "details": "Seed support is approximate."
                        },
                        {
                            "type": "other",
                            "message": "Gateway routed request"
                        }
                    ]
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
        .image_model("openai/gpt-image-1");
        let result =
            poll_ready(model.do_generate(ImageModelCallOptions::new(1).with_prompt("Warnings")));

        assert_eq!(
            result.warnings,
            vec![
                Warning::Unsupported {
                    feature: "size".to_string(),
                    details: None,
                },
                Warning::Compatibility {
                    feature: "seed".to_string(),
                    details: Some("Seed support is approximate.".to_string()),
                },
                Warning::Other {
                    message: "Gateway routed request".to_string(),
                },
            ]
        );
    }

    #[test]
    fn gateway_reranking_model_omits_optional_body_fields() {
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
                    "ranking": []
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
        .reranking_model("cohere/rerank-v3.5");
        let result = poll_ready(model.do_rerank(RerankingModelCallOptions::new(
            RerankingModelDocuments::text(vec!["one".to_string(), "two".to_string()]),
            "one",
        )));

        assert!(result.ranking.is_empty());
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");

        assert_eq!(
            request_body,
            json!({
                "documents": {
                    "type": "text",
                    "values": ["one", "two"]
                },
                "query": "one"
            })
        );
    }

    #[test]
    fn gateway_reranking_model_maps_gateway_error_to_metadata() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                500,
                "Internal Server Error",
                json!({
                    "error": {
                        "message": "Internal server error",
                        "type": "internal_server_error"
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
        .reranking_model("cohere/rerank-v3.5");
        let result = poll_ready(model.do_rerank(RerankingModelCallOptions::new(
            RerankingModelDocuments::text(vec!["bad".to_string()]),
            "bad",
        )));

        assert!(result.ranking.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("Internal server error")
        );
    }

    #[test]
    fn gateway_video_model_preserves_empty_and_nested_provider_metadata() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                format!(
                    "data: {}\n\n",
                    json!({
                        "type": "result",
                        "videos": [
                            {
                                "type": "base64",
                                "data": "AAAAIGZ0eXBtcDQy",
                                "mediaType": "video/mp4"
                            }
                        ],
                        "providerMetadata": {
                            "google": {
                                "cost": {
                                    "input": "0.000001",
                                    "output": "0.000003"
                                }
                            },
                            "gateway": {}
                        }
                    })
                ),
            ))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .video_model("google/veo-2.0-generate-001");
        let result =
            poll_ready(model.do_generate(VideoModelCallOptions::new(1).with_prompt("Animate")));
        let metadata = result
            .provider_metadata
            .expect("provider metadata is preserved");

        assert_eq!(
            metadata
                .get("google")
                .and_then(|metadata| metadata.get("cost"))
                .and_then(|metadata| metadata.get("input"))
                .and_then(JsonValue::as_str),
            Some("0.000001")
        );
        assert_eq!(
            metadata
                .get("google")
                .and_then(|metadata| metadata.get("cost"))
                .and_then(|metadata| metadata.get("output"))
                .and_then(JsonValue::as_str),
            Some("0.000003")
        );
        assert_eq!(
            metadata.get("gateway").expect("Gateway metadata exists"),
            &JsonObject::new()
        );
    }

    #[test]
    fn gateway_video_model_encodes_image_inputs_and_returns_url_videos() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                format!(
                    ":\n\ndata: {}\n\n",
                    json!({
                        "type": "result",
                        "videos": [
                            {
                                "type": "url",
                                "url": "https://example.com/video.mp4",
                                "mediaType": "video/mp4"
                            }
                        ]
                    })
                ),
            ))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .video_model("fal/luma-ray-2");
        let file_options: ProviderMetadata = serde_json::from_value(json!({
            "fal": {
                "purpose": "first-frame"
            }
        }))
        .expect("provider metadata deserialize");

        let result = poll_ready(
            model.do_generate(
                VideoModelCallOptions::new(1)
                    .with_prompt("Animate this image")
                    .with_duration(0.0)
                    .with_fps(0.0)
                    .with_seed(0)
                    .with_image(
                        VideoModelFile::file(
                            "image/png",
                            FileDataContent::Bytes(b"Hello".to_vec()),
                        )
                        .with_provider_options(file_options),
                    ),
            ),
        );

        assert_eq!(result.videos.len(), 1);
        assert!(matches!(
            result.videos[0],
            ai_sdk_provider::VideoModelVideoData::Url { .. }
        ));
        assert_eq!(result.warnings, Vec::<Warning>::new());

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let request_body = request
            .body
            .as_ref()
            .and_then(ProviderApiRequestBody::as_text)
            .and_then(|body| serde_json::from_str::<JsonValue>(body).ok())
            .expect("request body is JSON");

        assert_eq!(
            request_body
                .pointer("/image/data")
                .and_then(JsonValue::as_str),
            Some("SGVsbG8=")
        );
        assert_eq!(
            request_body
                .pointer("/image/providerOptions/fal/purpose")
                .and_then(JsonValue::as_str),
            Some("first-frame")
        );
        assert!(request_body.get("duration").is_none());
        assert!(request_body.get("fps").is_none());
        assert!(request_body.get("seed").is_none());
    }

    #[test]
    fn gateway_video_model_preserves_warning_variants() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                format!(
                    "data: {}\n\n",
                    json!({
                        "type": "result",
                        "videos": [
                            {
                                "type": "base64",
                                "data": "AAAAIGZ0eXBtcDQy",
                                "mediaType": "video/mp4"
                            }
                        ],
                        "warnings": [
                            {
                                "type": "unsupported",
                                "feature": "resolution"
                            },
                            {
                                "type": "compatibility",
                                "feature": "seed",
                                "details": "Seed support is approximate."
                            },
                            {
                                "type": "other",
                                "message": "Gateway routed request"
                            }
                        ]
                    })
                ),
            ))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .video_model("google/veo-2.0-generate-001");
        let result =
            poll_ready(model.do_generate(VideoModelCallOptions::new(1).with_prompt("Warnings")));

        assert_eq!(
            result.warnings,
            vec![
                Warning::Unsupported {
                    feature: "resolution".to_string(),
                    details: None,
                },
                Warning::Compatibility {
                    feature: "seed".to_string(),
                    details: Some("Seed support is approximate.".to_string()),
                },
                Warning::Other {
                    message: "Gateway routed request".to_string(),
                },
            ]
        );
    }

    #[test]
    fn gateway_video_model_maps_sse_error_to_metadata() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                format!(
                    "data: {}\n\n",
                    json!({
                        "type": "error",
                        "message": "Rate limit exceeded",
                        "errorType": "rate_limit_exceeded",
                        "statusCode": 429,
                        "param": null
                    })
                ),
            ))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .video_model("google/veo-2.0-generate-001");
        let result =
            poll_ready(model.do_generate(VideoModelCallOptions::new(1).with_prompt("bad prompt")));

        assert!(result.videos.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("Rate limit exceeded")
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("errorType"))
                .and_then(JsonValue::as_str),
            Some("rate_limit_exceeded")
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("statusCode"))
                .and_then(JsonValue::as_u64),
            Some(429)
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("isRetryable"))
                .and_then(JsonValue::as_bool),
            Some(true)
        );
    }

    #[test]
    fn gateway_video_model_maps_heartbeat_only_sse_to_metadata() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                ":\n\n".to_string(),
            ))))
        });
        let model = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com")
                .with_api_key("test-token"),
        )
        .with_transport(transport)
        .video_model("google/veo-2.0-generate-001");
        let result =
            poll_ready(model.do_generate(VideoModelCallOptions::new(1).with_prompt("bad prompt")));

        assert!(result.videos.is_empty());
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("errorMessage"))
                .and_then(JsonValue::as_str),
            Some("Invalid error response format: SSE stream ended without a data event")
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("errorType"))
                .and_then(JsonValue::as_str),
            Some("response_error")
        );
        assert_eq!(
            result
                .provider_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("gateway"))
                .and_then(|metadata| metadata.get("statusCode"))
                .and_then(JsonValue::as_u64),
            Some(200)
        );
    }

    #[test]
    fn gateway_function_uses_default_gateway_provider() {
        let model = gateway("openai/gpt-4.1-mini");

        assert_eq!(model.provider(), "gateway");
        assert_eq!(model.model_id(), "openai/gpt-4.1-mini");
    }

    #[test]
    fn create_gateway_language_model_uses_custom_configuration() {
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
                    "model": "test-model",
                    "content": {
                        "type": "text",
                        "text": "ok"
                    },
                    "finish_reason": "stop",
                    "usage": {
                        "prompt_tokens": 1,
                        "completion_tokens": 1
                    }
                })
                .to_string(),
            ))))
        });
        let provider = create_gateway(
            GatewayProviderSettings::new()
                .with_base_url("https://api.example.com")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value"),
        )
        .with_transport(transport);
        let model = provider.language_model("test-model");

        assert_eq!(model.provider(), "gateway");
        assert_eq!(model.model_id(), "test-model");

        let result = poll_ready(model.do_generate(LanguageModelCallOptions::new(Vec::new())));
        assert!(matches!(
            result.content.first(),
            Some(LanguageModelContent::Text(text)) if text.text == "ok"
        ));

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.example.com/language-model");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            request.headers.get("custom-header").map(String::as_str),
            Some("value")
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
                .get("ai-language-model-id")
                .map(String::as_str),
            Some("test-model")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.starts_with("ai-sdk/gateway/"))
        );
    }

    #[test]
    fn create_gateway_language_model_uses_oidc_when_api_key_is_absent() {
        let provider = create_gateway(
            GatewayProviderSettings::new()
                .with_base_url("https://api.example.com")
                .with_header("custom-header", "value"),
        );
        let model = provider.language_model("test-model");
        let headers = gateway_provider_headers_with_env(
            &model.settings,
            env_lookup(&[("VERCEL_OIDC_TOKEN", "mock-oidc-token")]),
        );

        assert_eq!(model.provider(), "gateway");
        assert_eq!(model.model_id(), "test-model");
        assert_eq!(gateway_base_url(&model.settings), "https://api.example.com");
        assert_eq!(
            headers.get("authorization").and_then(Option::as_deref),
            Some("Bearer mock-oidc-token")
        );
        assert_eq!(
            headers.get("custom-header").and_then(Option::as_deref),
            Some("value")
        );
        assert_eq!(
            headers
                .get("ai-gateway-protocol-version")
                .and_then(Option::as_deref),
            Some("0.0.1")
        );
        assert_eq!(
            headers
                .get("ai-gateway-auth-method")
                .and_then(Option::as_deref),
            Some("oidc")
        );
        assert!(
            headers
                .get("user-agent")
                .and_then(Option::as_deref)
                .is_some_and(|value| value.starts_with("ai-sdk/gateway/"))
        );
    }

    #[test]
    fn create_gateway_embedding_model_returns_gateway_embedding_model() {
        let provider =
            create_gateway(GatewayProviderSettings::new().with_base_url("https://api.example.com"));
        let model = provider.embedding_model("openai/text-embedding-3-small");

        assert_eq!(model.provider(), "gateway");
        assert_eq!(model.model_id(), "openai/text-embedding-3-small");
        assert_eq!(gateway_base_url(&model.settings), "https://api.example.com");
    }

    #[test]
    fn create_gateway_image_model_uses_custom_base_url() {
        let provider = create_gateway(
            GatewayProviderSettings::new()
                .with_base_url("https://api.example.com")
                .with_api_key("test-api-key"),
        );
        let model = provider.image_model("google/imagen-4.0-generate");

        assert_eq!(model.provider(), "gateway");
        assert_eq!(model.model_id(), "google/imagen-4.0-generate");
        assert_eq!(gateway_base_url(&model.settings), "https://api.example.com");
    }

    #[test]
    fn create_gateway_image_model_reuses_headers_transport_and_observability() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", "{}"))))
        });
        let provider = create_gateway(
            GatewayProviderSettings::new()
                .with_base_url("https://api.example.com")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value")
                .with_vercel_request_id("mock-request-id"),
        )
        .with_transport(Arc::clone(&transport));
        let model = provider.image_model("google/imagen-4.0-generate");
        let headers = gateway_provider_headers_with_env(&model.settings, env_lookup(&[]));
        let observability_headers =
            gateway_observability_headers_with_env(&model.settings, env_lookup(&[]));

        assert!(Arc::ptr_eq(&model.transport, &transport));
        assert_eq!(
            headers.get("authorization").and_then(Option::as_deref),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            headers.get("custom-header").and_then(Option::as_deref),
            Some("value")
        );
        assert_eq!(
            headers
                .get("ai-gateway-protocol-version")
                .and_then(Option::as_deref),
            Some("0.0.1")
        );
        assert_eq!(
            headers
                .get("ai-gateway-auth-method")
                .and_then(Option::as_deref),
            Some("api-key")
        );
        assert!(
            headers
                .get("user-agent")
                .and_then(Option::as_deref)
                .is_some_and(|value| value.starts_with("ai-sdk/gateway/"))
        );
        assert_eq!(
            observability_headers
                .get("ai-o11y-request-id")
                .map(String::as_str),
            Some("mock-request-id")
        );
    }

    #[test]
    fn create_gateway_video_model_uses_custom_base_url() {
        let provider = create_gateway(
            GatewayProviderSettings::new()
                .with_base_url("https://api.example.com")
                .with_api_key("test-api-key"),
        );
        let model = provider.video_model("google/veo-2.0-generate-001");

        assert_eq!(model.provider(), "gateway");
        assert_eq!(model.model_id(), "google/veo-2.0-generate-001");
        assert_eq!(gateway_base_url(&model.settings), "https://api.example.com");
    }

    #[test]
    fn create_gateway_video_model_reuses_headers_transport_and_observability() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(200, "OK", "{}"))))
        });
        let provider = create_gateway(
            GatewayProviderSettings::new()
                .with_base_url("https://api.example.com")
                .with_api_key("test-api-key")
                .with_header("custom-header", "value")
                .with_vercel_request_id("mock-request-id"),
        )
        .with_transport(Arc::clone(&transport));
        let model = provider.video_model("google/veo-2.0-generate-001");
        let headers = gateway_provider_headers_with_env(&model.settings, env_lookup(&[]));
        let observability_headers =
            gateway_observability_headers_with_env(&model.settings, env_lookup(&[]));

        assert!(Arc::ptr_eq(&model.transport, &transport));
        assert_eq!(
            headers.get("authorization").and_then(Option::as_deref),
            Some("Bearer test-api-key")
        );
        assert_eq!(
            headers.get("custom-header").and_then(Option::as_deref),
            Some("value")
        );
        assert_eq!(
            headers
                .get("ai-gateway-protocol-version")
                .and_then(Option::as_deref),
            Some("0.0.1")
        );
        assert_eq!(
            headers
                .get("ai-gateway-auth-method")
                .and_then(Option::as_deref),
            Some("api-key")
        );
        assert!(
            headers
                .get("user-agent")
                .and_then(Option::as_deref)
                .is_some_and(|value| value.starts_with("ai-sdk/gateway/"))
        );
        assert_eq!(
            observability_headers
                .get("ai-o11y-request-id")
                .map(String::as_str),
            Some("mock-request-id")
        );
    }

    #[test]
    fn create_gateway_reranking_model_uses_custom_base_url() {
        let provider = create_gateway(
            GatewayProviderSettings::new()
                .with_base_url("https://api.example.com")
                .with_api_key("test-api-key"),
        );
        let model = provider.reranking_model("cohere/rerank-v3.5");

        assert_eq!(model.provider(), "gateway");
        assert_eq!(model.model_id(), "cohere/rerank-v3.5");
        assert_eq!(gateway_base_url(&model.settings), "https://api.example.com");
    }

    #[test]
    fn create_gateway_reranking_alias_returns_gateway_reranking_model() {
        let provider = create_gateway(
            GatewayProviderSettings::new()
                .with_base_url("https://api.example.com")
                .with_api_key("test-api-key"),
        );
        let model = provider.reranking("cohere/rerank-v3.5");

        assert_eq!(model.provider(), "gateway");
        assert_eq!(model.model_id(), "cohere/rerank-v3.5");
        assert_eq!(gateway_base_url(&model.settings), "https://api.example.com");
    }

    #[test]
    fn create_gateway_fetches_available_models_with_custom_base_url() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({ "models": [] }).to_string(),
            ))))
        });
        let provider = create_gateway(
            GatewayProviderSettings::new()
                .with_base_url("https://api.example.com")
                .with_api_key("test-api-key"),
        )
        .with_transport(transport);
        let result = poll_ready(provider.get_available_models()).expect("metadata fetch succeeds");

        assert!(result.models.is_empty());
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.method, ProviderApiRequestMethod::Get);
        assert_eq!(request.url, "https://api.example.com/config");
        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-api-key")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.starts_with("ai-sdk/gateway/"))
        );
    }

    #[test]
    fn create_gateway_caches_metadata_for_configured_refresh_interval() {
        let request_count = Arc::new(Mutex::new(0_u32));
        let provider = create_gateway(
            GatewayProviderSettings::new()
                .with_base_url("https://api.example.com")
                .with_api_key("test-api-key")
                .with_metadata_cache_refresh_millis(10_000),
        )
        .with_transport(counting_metadata_transport(Arc::clone(&request_count)));

        let first = poll_ready(provider.get_available_models()).expect("first fetch succeeds");
        let second = poll_ready(provider.get_available_models()).expect("second fetch is cached");
        {
            let mut cache = provider
                .metadata_cache
                .lock()
                .expect("gateway metadata cache mutex is not poisoned");
            cache.fetched_at = Some(Instant::now() - Duration::from_millis(9_000));
        }
        let third =
            poll_ready(provider.get_available_models()).expect("third fetch is still cached");
        {
            let mut cache = provider
                .metadata_cache
                .lock()
                .expect("gateway metadata cache mutex is not poisoned");
            cache.fetched_at = Some(Instant::now() - Duration::from_millis(11_000));
        }
        let fourth =
            poll_ready(provider.get_available_models()).expect("fourth fetch refreshes cache");

        assert_eq!(
            *request_count
                .lock()
                .expect("request count mutex is not poisoned"),
            2
        );
        assert_eq!(first.models[0].id, "model-1");
        assert_eq!(second.models[0].id, "model-1");
        assert_eq!(third.models[0].id, "model-1");
        assert_eq!(fourth.models[0].id, "model-2");
    }

    #[test]
    fn create_gateway_uses_default_five_minute_metadata_refresh_interval() {
        let request_count = Arc::new(Mutex::new(0_u32));
        let provider = create_gateway(
            GatewayProviderSettings::new()
                .with_base_url("https://api.example.com")
                .with_api_key("test-api-key"),
        )
        .with_transport(counting_metadata_transport(Arc::clone(&request_count)));

        let first = poll_ready(provider.get_available_models()).expect("first fetch succeeds");
        {
            let mut cache = provider
                .metadata_cache
                .lock()
                .expect("gateway metadata cache mutex is not poisoned");
            cache.fetched_at = Some(Instant::now() - Duration::from_secs(4 * 60));
        }
        let second = poll_ready(provider.get_available_models()).expect("second fetch is cached");
        {
            let mut cache = provider
                .metadata_cache
                .lock()
                .expect("gateway metadata cache mutex is not poisoned");
            cache.fetched_at = Some(Instant::now() - Duration::from_secs(6 * 60));
        }
        let third =
            poll_ready(provider.get_available_models()).expect("third fetch refreshes cache");

        assert_eq!(
            *request_count
                .lock()
                .expect("request count mutex is not poisoned"),
            2
        );
        assert_eq!(
            metadata_cache_refresh_duration(&provider.settings),
            Duration::from_secs(5 * 60)
        );
        assert_eq!(first.models[0].id, "model-1");
        assert_eq!(second.models[0].id, "model-1");
        assert_eq!(third.models[0].id, "model-2");
    }

    #[test]
    fn create_gateway_language_model_passes_observability_headers_from_environment() {
        let provider = create_gateway(
            GatewayProviderSettings::new()
                .with_base_url("https://api.example.com")
                .with_api_key("test-api-key")
                .with_vercel_request_id("test-request-id"),
        );
        let model = provider.language_model("test-model");
        let observability_headers = gateway_observability_headers_with_env(
            &model.settings,
            env_lookup(&[
                ("VERCEL_DEPLOYMENT_ID", "test-deployment"),
                ("VERCEL_ENV", "test"),
                ("VERCEL_REGION", "iad1"),
                ("VERCEL_PROJECT_ID", "prj_test123"),
            ]),
        );

        assert_eq!(model.provider(), "gateway");
        assert_eq!(gateway_base_url(&model.settings), "https://api.example.com");
        assert_eq!(
            observability_headers
                .get("ai-o11y-deployment-id")
                .map(String::as_str),
            Some("test-deployment")
        );
        assert_eq!(
            observability_headers
                .get("ai-o11y-environment")
                .map(String::as_str),
            Some("test")
        );
        assert_eq!(
            observability_headers
                .get("ai-o11y-region")
                .map(String::as_str),
            Some("iad1")
        );
        assert_eq!(
            observability_headers
                .get("ai-o11y-request-id")
                .map(String::as_str),
            Some("test-request-id")
        );
        assert_eq!(
            observability_headers
                .get("ai-o11y-project-id")
                .map(String::as_str),
            Some("prj_test123")
        );
    }

    #[test]
    fn create_gateway_language_model_omits_missing_observability_headers() {
        let provider = create_gateway(
            GatewayProviderSettings::new()
                .with_base_url("https://api.example.com")
                .with_api_key("test-api-key"),
        );
        let model = provider.language_model("test-model");
        let observability_headers =
            gateway_observability_headers_with_env(&model.settings, env_lookup(&[]));

        assert_eq!(model.provider(), "gateway");
        assert_eq!(gateway_base_url(&model.settings), "https://api.example.com");
        assert!(observability_headers.is_empty());
    }

    #[test]
    fn default_gateway_export_exposes_provider_instance() {
        let provider = GatewayProvider::new();
        let model = gateway("test-model");

        assert_eq!(model.provider(), "gateway");
        assert_eq!(model.model_id(), "test-model");
        assert_eq!(provider.language_model("test-model").provider(), "gateway");
    }

    #[test]
    fn create_gateway_uses_default_base_url_when_none_is_provided() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({ "models": [] }).to_string(),
            ))))
        });
        let provider = create_gateway(GatewayProviderSettings::new().with_api_key("test-key"))
            .with_transport(transport);
        let result = poll_ready(provider.get_available_models()).expect("metadata fetch succeeds");

        assert!(result.models.is_empty());
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.url, "https://ai-gateway.vercel.sh/v4/ai/config");
    }

    #[test]
    fn create_gateway_accepts_empty_options() {
        let provider = create_gateway(GatewayProviderSettings::new());
        let model = provider.language_model("test-model");

        assert_eq!(model.provider(), "gateway");
        assert_eq!(model.model_id(), "test-model");
        assert_eq!(
            gateway_base_url(&provider.settings),
            DEFAULT_GATEWAY_BASE_URL
        );
    }

    #[test]
    fn default_gateway_export_constructs_image_model() {
        let provider = GatewayProvider::new();
        let model = provider.image_model("google/imagen-4.0-generate");

        assert_eq!(model.provider(), "gateway");
        assert_eq!(model.model_id(), "google/imagen-4.0-generate");
        assert_eq!(gateway_base_url(&model.settings), DEFAULT_GATEWAY_BASE_URL);
    }

    #[test]
    fn default_gateway_export_constructs_video_model() {
        let provider = GatewayProvider::new();
        let model = provider.video_model("google/veo-2.0-generate-001");

        assert_eq!(model.provider(), "gateway");
        assert_eq!(model.model_id(), "google/veo-2.0-generate-001");
        assert_eq!(gateway_base_url(&model.settings), DEFAULT_GATEWAY_BASE_URL);
    }

    #[test]
    fn create_gateway_overrides_default_base_url_when_provided() {
        let captured_request = Arc::new(Mutex::new(None::<ProviderApiRequest>));
        let captured_request_for_transport = Arc::clone(&captured_request);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            *captured_request_for_transport
                .lock()
                .expect("captured request mutex is not poisoned") = Some(request.clone());

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({ "models": [] }).to_string(),
            ))))
        });
        let provider = create_gateway(
            GatewayProviderSettings::new()
                .with_base_url("https://custom-api.example.com")
                .with_api_key("test-key"),
        )
        .with_transport(transport);
        let result = poll_ready(provider.get_available_models()).expect("metadata fetch succeeds");

        assert!(result.models.is_empty());
        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(request.url, "https://custom-api.example.com/config");
    }

    #[test]
    fn create_gateway_prefers_api_key_over_oidc_token() {
        let provider =
            create_gateway(GatewayProviderSettings::new().with_api_key("test-api-key-123"));
        let headers = gateway_provider_headers_with_env(
            &provider.settings,
            env_lookup(&[("VERCEL_OIDC_TOKEN", "mock-oidc-token")]),
        );

        assert_eq!(
            headers.get("authorization").and_then(Option::as_deref),
            Some("Bearer test-api-key-123")
        );
        assert_eq!(
            headers
                .get("ai-gateway-auth-method")
                .and_then(Option::as_deref),
            Some("api-key")
        );
        assert!(
            headers
                .get("user-agent")
                .and_then(Option::as_deref)
                .is_some_and(|value| value.starts_with("ai-sdk/gateway/"))
        );
    }

    #[test]
    fn gateway_provider_creates_embedding_model_aliases() {
        let provider = GatewayProvider::new();

        assert_eq!(
            provider
                .embedding("openai/text-embedding-3-small")
                .model_id(),
            "openai/text-embedding-3-small"
        );
        assert_eq!(
            provider
                .text_embedding_model("openai/text-embedding-3-small")
                .provider(),
            "gateway"
        );
    }

    #[test]
    fn gateway_provider_creates_image_model_aliases() {
        let provider = GatewayProvider::new();

        assert_eq!(
            provider.image("openai/gpt-image-1").model_id(),
            "openai/gpt-image-1"
        );
        assert_eq!(
            provider.image_model("openai/gpt-image-1").provider(),
            "gateway"
        );
        assert_eq!(
            poll_ready(
                provider
                    .image_model("openai/gpt-image-1")
                    .max_images_per_call()
            ),
            Some(usize::MAX)
        );
    }

    #[test]
    fn gateway_provider_creates_reranking_model_aliases() {
        let provider = GatewayProvider::new();

        assert_eq!(
            provider.reranking("cohere/rerank-v3.5").model_id(),
            "cohere/rerank-v3.5"
        );
        assert_eq!(
            provider
                .reranking_model("cohere/rerank-v3.5")
                .specification_version(),
            SpecificationVersion::V4
        );
        assert_eq!(
            provider.reranking_model("cohere/rerank-v3.5").provider(),
            "gateway"
        );
    }

    #[test]
    fn gateway_provider_creates_video_model_aliases() {
        let provider = GatewayProvider::new();

        assert_eq!(
            provider.video("google/veo-2.0-generate-001").model_id(),
            "google/veo-2.0-generate-001"
        );
        assert_eq!(
            provider
                .video_model("google/veo-2.0-generate-001")
                .specification_version(),
            SpecificationVersion::V4
        );
        assert_eq!(
            provider
                .video_model("google/veo-2.0-generate-001")
                .provider(),
            "gateway"
        );
        assert_eq!(
            poll_ready(
                provider
                    .video_model("google/veo-2.0-generate-001")
                    .max_videos_per_call()
            ),
            Some(usize::MAX)
        );
    }

    #[test]
    fn gateway_provider_implements_provider_traits() {
        let provider = GatewayProvider::new();

        assert_eq!(
            Provider::specification_version(&provider),
            SpecificationVersion::V4
        );
        assert_eq!(
            Provider::language_model(&provider, "openai/gpt-4.1-mini")
                .expect("language model exists")
                .provider(),
            "gateway"
        );
        assert_eq!(
            Provider::embedding_model(&provider, "openai/text-embedding-3-small")
                .expect("embedding model exists")
                .model_id(),
            "openai/text-embedding-3-small"
        );
        assert_eq!(
            Provider::image_model(&provider, "openai/gpt-image-1")
                .expect("image model exists")
                .model_id(),
            "openai/gpt-image-1"
        );
        assert_eq!(
            ProviderWithRerankingModel::reranking_model(&provider, "cohere/rerank-v3.5")
                .expect("reranking model exists")
                .provider(),
            "gateway"
        );
        assert_eq!(
            ProviderWithVideoModel::video_model(&provider, "google/veo-2.0-generate-001")
                .expect("video model exists")
                .provider(),
            "gateway"
        );
    }

    #[test]
    fn gateway_provider_exposes_gateway_tools() {
        let tool = GatewayProvider::new().tools().parallel_search(
            "parallelSearch",
            crate::gateway_tools::ParallelSearchConfig::new(),
        );

        assert!(tool.is_provider_executed());
        assert_eq!(tool.provider_tool_id(), Some("gateway.parallel_search"));
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
    fn gateway_provider_metadata_preserves_known_model_types_and_filters_unknown() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "models": [
                        {
                            "id": "model-embedding",
                            "name": "Model embedding",
                            "specification": {
                                "specificationVersion": "v4",
                                "provider": "gateway",
                                "modelId": "model-embedding"
                            },
                            "modelType": "embedding"
                        },
                        {
                            "id": "model-image",
                            "name": "Model image",
                            "specification": {
                                "specificationVersion": "v4",
                                "provider": "gateway",
                                "modelId": "model-image"
                            },
                            "modelType": "image"
                        },
                        {
                            "id": "model-language",
                            "name": "Model language",
                            "specification": {
                                "specificationVersion": "v4",
                                "provider": "gateway",
                                "modelId": "model-language"
                            },
                            "modelType": "language"
                        },
                        {
                            "id": "model-reranking",
                            "name": "Model reranking",
                            "specification": {
                                "specificationVersion": "v4",
                                "provider": "gateway",
                                "modelId": "model-reranking"
                            },
                            "modelType": "reranking"
                        },
                        {
                            "id": "model-video",
                            "name": "Model video",
                            "specification": {
                                "specificationVersion": "v4",
                                "provider": "gateway",
                                "modelId": "model-video"
                            },
                            "modelType": "video"
                        },
                        {
                            "id": "model-future",
                            "name": "Model future",
                            "specification": {
                                "specificationVersion": "v4",
                                "provider": "gateway",
                                "modelId": "model-future"
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
                .with_api_key("test-token"),
        )
        .with_transport(transport);
        let result = poll_ready(provider.get_available_models()).expect("metadata fetch succeeds");

        assert_eq!(result.models.len(), 5);
        assert_eq!(
            result
                .models
                .iter()
                .map(|model| model.model_type)
                .collect::<Vec<_>>(),
            vec![
                Some(GatewayModelType::Embedding),
                Some(GatewayModelType::Image),
                Some(GatewayModelType::Language),
                Some(GatewayModelType::Reranking),
                Some(GatewayModelType::Video),
            ]
        );
    }

    #[test]
    fn gateway_provider_metadata_rejects_invalid_pricing_format() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "models": [
                        {
                            "id": "model-invalid-pricing",
                            "name": "Invalid Pricing Model",
                            "pricing": {
                                "input": 123,
                                "output": "0.000002"
                            },
                            "specification": {
                                "specificationVersion": "v4",
                                "provider": "gateway",
                                "modelId": "model-invalid-pricing"
                            },
                            "modelType": "language"
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

        let error = poll_ready(provider.get_available_models()).expect_err("metadata is rejected");
        let response_error = error
            .as_response()
            .expect("invalid metadata maps to a Gateway response error");

        assert_eq!(response_error.error_type(), "response_error");
        assert_eq!(response_error.status_code(), 200);
        assert!(response_error.validation_error().is_some());
    }

    #[test]
    fn gateway_provider_caches_available_models_until_refresh() {
        let request_count = Arc::new(Mutex::new(0_u32));
        let request_count_for_transport = Arc::clone(&request_count);
        let transport: GatewayTransport = Arc::new(move |_request| -> GatewayTransportFuture {
            let mut count = request_count_for_transport
                .lock()
                .expect("request count mutex is not poisoned");
            *count += 1;
            let model_id = format!("model-{}", *count);

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "models": [{
                        "id": model_id,
                        "name": "Cached Model",
                        "specification": {
                            "specificationVersion": "v4",
                            "provider": "gateway",
                            "modelId": model_id
                        },
                        "modelType": "language"
                    }]
                })
                .to_string(),
            ))))
        });
        let provider = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com/v4/ai")
                .with_api_key("test-token")
                .with_metadata_cache_refresh_millis(60_000),
        )
        .with_transport(transport);

        let first = poll_ready(provider.get_available_models()).expect("first fetch succeeds");
        let second = poll_ready(provider.get_available_models()).expect("second fetch succeeds");

        assert_eq!(
            *request_count
                .lock()
                .expect("request count mutex is not poisoned"),
            1
        );
        assert_eq!(first.models[0].id, "model-1");
        assert_eq!(second.models[0].id, "model-1");
    }

    #[test]
    fn gateway_provider_refreshes_available_models_after_refresh_interval() {
        let request_count = Arc::new(Mutex::new(0_u32));
        let request_count_for_transport = Arc::clone(&request_count);
        let transport: GatewayTransport = Arc::new(move |_request| -> GatewayTransportFuture {
            let mut count = request_count_for_transport
                .lock()
                .expect("request count mutex is not poisoned");
            *count += 1;
            let model_id = format!("model-{}", *count);

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "models": [{
                        "id": model_id,
                        "name": "Refresh Interval Model",
                        "specification": {
                            "specificationVersion": "v4",
                            "provider": "gateway",
                            "modelId": model_id
                        },
                        "modelType": "language"
                    }]
                })
                .to_string(),
            ))))
        });
        let provider = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com/v4/ai")
                .with_api_key("test-token")
                .with_metadata_cache_refresh_millis(5),
        )
        .with_transport(transport);

        let first = poll_ready(provider.get_available_models()).expect("first fetch succeeds");
        let second = poll_ready(provider.get_available_models()).expect("second fetch is cached");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let third =
            poll_ready(provider.get_available_models()).expect("third fetch refreshes cache");

        assert_eq!(
            *request_count
                .lock()
                .expect("request count mutex is not poisoned"),
            2
        );
        assert_eq!(first.models[0].id, "model-1");
        assert_eq!(second.models[0].id, "model-1");
        assert_eq!(third.models[0].id, "model-2");
    }

    #[test]
    fn gateway_provider_uses_default_metadata_cache_refresh_interval() {
        let settings = GatewayProviderSettings::new();

        assert_eq!(
            metadata_cache_refresh_duration(&settings),
            std::time::Duration::from_secs(5 * 60)
        );
    }

    #[test]
    fn gateway_provider_refreshes_available_models_when_cache_disabled() {
        let request_count = Arc::new(Mutex::new(0_u32));
        let request_count_for_transport = Arc::clone(&request_count);
        let transport: GatewayTransport = Arc::new(move |_request| -> GatewayTransportFuture {
            let mut count = request_count_for_transport
                .lock()
                .expect("request count mutex is not poisoned");
            *count += 1;
            let model_id = format!("model-{}", *count);

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                json!({
                    "models": [{
                        "id": model_id,
                        "name": "Refreshed Model",
                        "specification": {
                            "specificationVersion": "v4",
                            "provider": "gateway",
                            "modelId": model_id
                        },
                        "modelType": "language"
                    }]
                })
                .to_string(),
            ))))
        });
        let provider = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com/v4/ai")
                .with_api_key("test-token")
                .with_metadata_cache_refresh_millis(0),
        )
        .with_transport(transport);

        let first = poll_ready(provider.get_available_models()).expect("first fetch succeeds");
        let second = poll_ready(provider.get_available_models()).expect("second fetch succeeds");

        assert_eq!(
            *request_count
                .lock()
                .expect("request count mutex is not poisoned"),
            2
        );
        assert_eq!(first.models[0].id, "model-1");
        assert_eq!(second.models[0].id, "model-2");
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
    fn gateway_provider_get_credits_includes_upstream_headers() {
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
                .with_api_key("test-key")
                .with_header("custom-header", "custom-value"),
        )
        .with_transport(transport);

        let credits = poll_ready(provider.get_credits()).expect("credits fetch succeeds");
        assert_eq!(credits.balance, "150.50");
        assert_eq!(credits.total_used, "75.25");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");

        assert_eq!(
            request.headers.get("authorization").map(String::as_str),
            Some("Bearer test-key")
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
            request.headers.get("custom-header").map(String::as_str),
            Some("custom-value")
        );
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.starts_with("ai-sdk/gateway/"))
        );
    }

    #[test]
    fn gateway_provider_get_credits_surfaces_endpoint_errors() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Err(FetchErrorInfo::new(
                "Credits service unavailable",
            ))))
        });
        let provider =
            GatewayProvider::from_settings(GatewayProviderSettings::new().with_api_key("test-key"))
                .with_transport(transport);

        let error = poll_ready(provider.get_credits()).expect_err("credits request fails");
        let response_error = error
            .as_response()
            .expect("transport error maps to Gateway response error");

        assert!(
            response_error
                .message()
                .contains("Gateway request failed: Credits service unavailable")
        );
        assert_eq!(
            response_error.cause_message(),
            Some("Credits service unavailable")
        );
    }

    #[test]
    fn gateway_provider_account_methods_use_default_gateway_urls() {
        let captured_requests = Arc::new(Mutex::new(Vec::<ProviderApiRequest>::new()));
        let captured_requests_for_transport = Arc::clone(&captured_requests);
        let transport: GatewayTransport = Arc::new(move |request| -> GatewayTransportFuture {
            captured_requests_for_transport
                .lock()
                .expect("captured requests mutex is not poisoned")
                .push(request.clone());

            let body = if request.url.ends_with("/v4/ai/config") {
                json!({
                    "models": []
                })
            } else if request.url.ends_with("/v1/credits") {
                json!({
                    "balance": "100.00",
                    "total_used": "50.00"
                })
            } else {
                json!({
                    "results": []
                })
            };

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                body.to_string(),
            ))))
        });
        let provider = GatewayProvider::from_settings(
            GatewayProviderSettings::new().with_api_key("test-token"),
        )
        .with_transport(transport);

        let metadata =
            poll_ready(provider.get_available_models()).expect("metadata fetch succeeds");
        let credits = poll_ready(provider.get_credits()).expect("credits fetch succeeds");
        let spend_report = poll_ready(
            provider.get_spend_report(GatewaySpendReportParams::new("2026-03-01", "2026-03-25")),
        )
        .expect("spend report fetch succeeds");

        assert!(metadata.models.is_empty());
        assert_eq!(credits.balance, "100.00");
        assert!(spend_report.results.is_empty());

        let requests = captured_requests
            .lock()
            .expect("captured requests mutex is not poisoned");
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].url, "https://ai-gateway.vercel.sh/v4/ai/config");
        assert_eq!(requests[1].url, "https://ai-gateway.vercel.sh/v1/credits");

        let report_url = url::Url::parse(&requests[2].url).expect("report URL is valid");
        assert_eq!(
            report_url.as_str().split('?').next(),
            Some("https://ai-gateway.vercel.sh/v1/report")
        );
        assert_eq!(
            report_url
                .query_pairs()
                .find(|(key, _)| key == "start_date")
                .map(|(_, value)| value.into_owned()),
            Some("2026-03-01".to_string())
        );
        assert_eq!(
            report_url
                .query_pairs()
                .find(|(key, _)| key == "end_date")
                .map(|(_, value)| value.into_owned()),
            Some("2026-03-25".to_string())
        );
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
        let rate_limit_error = error
            .as_rate_limit()
            .expect("gateway metadata errors map to Gateway rate-limit errors");

        assert_eq!(rate_limit_error.status_code(), 429);
        assert_eq!(rate_limit_error.message(), "Rate limit exceeded");
        assert!(rate_limit_error.is_retryable());
    }

    #[test]
    fn gateway_provider_account_apis_surface_malformed_json_error_responses() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Ok(ProviderApiResponse::text(
                500,
                "Internal Server Error",
                "{not json",
            ))))
        });
        let provider = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com/v4/ai")
                .with_api_key("test-token"),
        )
        .with_transport(transport);

        let spend_error = poll_ready(
            provider.get_spend_report(GatewaySpendReportParams::new("2026-03-01", "2026-03-25")),
        )
        .expect_err("malformed spend report error body fails");
        let spend_response_error = spend_error
            .as_response()
            .expect("malformed spend report error maps to response error");

        assert_eq!(spend_response_error.status_code(), 500);
        assert_eq!(
            spend_response_error.response(),
            Some(&JsonValue::String("{not json".to_string()))
        );
        assert!(spend_response_error.validation_error().is_some());

        let generation_error =
            poll_ready(provider.get_generation_info(GatewayGenerationInfoParams::new("gen_123")))
                .expect_err("malformed generation info error body fails");
        let generation_response_error = generation_error
            .as_response()
            .expect("malformed generation info error maps to response error");

        assert_eq!(generation_response_error.status_code(), 500);
        assert_eq!(
            generation_response_error.response(),
            Some(&JsonValue::String("{not json".to_string()))
        );
        assert!(generation_response_error.validation_error().is_some());
    }

    #[test]
    fn gateway_provider_fetches_empty_metadata_and_zero_credits() {
        let request_count = Arc::new(Mutex::new(0_u32));
        let request_count_for_transport = Arc::clone(&request_count);
        let transport: GatewayTransport = Arc::new(move |_request| -> GatewayTransportFuture {
            let mut count = request_count_for_transport
                .lock()
                .expect("request count mutex is not poisoned");
            *count += 1;
            let body = if *count == 1 {
                json!({
                    "models": []
                })
            } else {
                json!({
                    "balance": "0.00",
                    "total_used": "0.00"
                })
            };

            Box::pin(ready(Ok(ProviderApiResponse::text(
                200,
                "OK",
                body.to_string(),
            ))))
        });
        let provider = GatewayProvider::from_settings(
            GatewayProviderSettings::new()
                .with_base_url("https://api.test.com/v4/ai")
                .with_api_key("test-token"),
        )
        .with_transport(transport);

        let metadata =
            poll_ready(provider.get_available_models()).expect("empty metadata fetch succeeds");
        let credits = poll_ready(provider.get_credits()).expect("zero credits fetch succeeds");

        assert!(metadata.models.is_empty());
        assert_eq!(credits.balance, "0.00");
        assert_eq!(credits.total_used, "0.00");
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
    fn gateway_provider_spend_report_omits_optional_query_params_and_metrics() {
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
                            "total_cost": 1.5
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
                    .with_tags(Vec::<String>::new()),
            ),
        )
        .expect("spend report fetch succeeds");

        assert_eq!(result.results.len(), 1);
        assert_eq!(
            serde_json::to_value(&result.results[0]).expect("row serializes"),
            json!({
                "day": "2026-03-01",
                "totalCost": 1.5
            })
        );

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let url = url::Url::parse(&request.url).expect("request URL is valid");
        let absent_params = [
            "group_by",
            "date_part",
            "user_id",
            "model",
            "provider",
            "credential_type",
            "tags",
        ];

        assert_eq!(
            url.as_str().split('?').next(),
            Some("https://api.test.com/v1/report")
        );
        for name in absent_params {
            assert!(!url.query_pairs().any(|(key, _)| key == name));
        }
    }

    #[test]
    fn gateway_provider_get_spend_report_surfaces_endpoint_errors() {
        let transport: GatewayTransport = Arc::new(|_request| -> GatewayTransportFuture {
            Box::pin(ready(Err(FetchErrorInfo::new(
                "Reporting service unavailable",
            ))))
        });
        let provider =
            GatewayProvider::from_settings(GatewayProviderSettings::new().with_api_key("test-key"))
                .with_transport(transport);

        let error = poll_ready(
            provider.get_spend_report(GatewaySpendReportParams::new("2026-03-01", "2026-03-25")),
        )
        .expect_err("spend report request fails");
        let response_error = error
            .as_response()
            .expect("transport error maps to Gateway response error");

        assert!(
            response_error
                .message()
                .contains("Gateway request failed: Reporting service unavailable")
        );
        assert_eq!(
            response_error.cause_message(),
            Some("Reporting service unavailable")
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

    #[test]
    fn gateway_provider_generation_info_encodes_special_ids_and_byok_response() {
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
                        "id": "gen id/with?chars&tag=1",
                        "total_cost": 0.00123,
                        "upstream_inference_cost": 0.0009,
                        "usage": 0.00123,
                        "created_at": "2024-01-01T00:00:00.000Z",
                        "model": "claude-sonnet-4",
                        "is_byok": true,
                        "provider_name": "anthropic",
                        "streamed": false,
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
            provider
                .get_generation_info(GatewayGenerationInfoParams::new("gen id/with?chars&tag=1")),
        )
        .expect("generation info fetch succeeds");

        assert!(result.is_byok);
        assert_eq!(result.provider_name, "anthropic");
        assert_eq!(result.upstream_inference_cost, 0.0009);
        assert_eq!(result.model, "claude-sonnet-4");

        let request = captured_request
            .lock()
            .expect("captured request mutex is not poisoned")
            .clone()
            .expect("request is captured");
        let url = url::Url::parse(&request.url).expect("request URL is valid");

        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "id")
                .map(|(_, value)| value.into_owned()),
            Some("gen id/with?chars&tag=1".to_string())
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
    #[ignore = "requires a Vercel AI Gateway API key and makes a live video generation call"]
    fn live_gateway_generate_video() {
        let Some(api_key) = live_gateway_api_key() else {
            eprintln!("skipping live Gateway video test because no API key is configured");
            return;
        };
        let model_id = env::var("AI_SDK_RUST_GATEWAY_VIDEO_MODEL")
            .or_else(|_| env::var("AI_GATEWAY_VIDEO_MODEL"))
            .unwrap_or_else(|_| "google/veo-2.0-generate-001".to_string());
        let model = GatewayProvider::new()
            .with_api_key(api_key)
            .video_model(model_id);
        let result = poll_ready(
            model.do_generate(
                VideoModelCallOptions::new(1)
                    .with_prompt("A minimal two second abstract color motion test")
                    .with_duration(2.0),
            ),
        );

        assert!(
            !result.videos.is_empty(),
            "gateway video response was empty"
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
