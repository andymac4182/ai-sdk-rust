//! Gateway provider helpers for the Rust port of upstream `@ai-sdk/gateway`.

#![forbid(unsafe_code)]

/// The Gateway crate version compiled into the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod gateway;
pub mod gateway_error;
pub mod gateway_tools;
pub mod vercel_ai_gateway;

pub use gateway::{
    DEFAULT_GATEWAY_BASE_URL, GatewayAuthToken, GatewayCredentialType, GatewayCreditsResponse,
    GatewayEmbeddingModel, GatewayFetchMetadataResponse, GatewayGenerationInfo,
    GatewayGenerationInfoParams, GatewayImageModel, GatewayLanguageModel,
    GatewayLanguageModelEntry, GatewayLanguageModelPricing, GatewayLanguageModelSpecification,
    GatewayModelType, GatewayProvider, GatewayProviderOptions, GatewayProviderOptionsSort,
    GatewayProviderOptionsValidationError, GatewayProviderSettings, GatewayProviderTimeouts,
    GatewayRerankingModel, GatewaySpendReportDatePart, GatewaySpendReportGroupBy,
    GatewaySpendReportParams, GatewaySpendReportResponse, GatewaySpendReportRow, GatewayTransport,
    GatewayTransportFuture, GatewayVideoModel, create_gateway, create_gateway_provider, gateway,
    gateway_observability_headers, gateway_provider_options, get_gateway_auth_token,
    try_gateway_provider_options,
};

pub use gateway_error::{
    GATEWAY_AUTH_METHOD_HEADER, GatewayAuthMethod, GatewayAuthenticationError, GatewayError,
    GatewayInternalServerError, GatewayInvalidRequestError, GatewayModelNotFoundError,
    GatewayRateLimitError, GatewayResponseError, GatewayTimeoutError, as_gateway_error,
    create_gateway_error_from_api_call, create_gateway_error_from_response,
    extract_gateway_api_call_response, gateway_headers_from_auth_method, parse_gateway_auth_method,
};
pub use gateway_tools::{
    GatewayTools, ParallelSearchConfig, ParallelSearchError, ParallelSearchErrorType,
    ParallelSearchExcerpts, ParallelSearchFetchPolicy, ParallelSearchInput,
    ParallelSearchInputExcerpts, ParallelSearchInputFetchPolicy, ParallelSearchInputSourcePolicy,
    ParallelSearchMode, ParallelSearchOutput, ParallelSearchResponse, ParallelSearchResult,
    ParallelSearchSourcePolicy, PerplexitySearchConfig, PerplexitySearchError,
    PerplexitySearchErrorType, PerplexitySearchInput, PerplexitySearchOutput,
    PerplexitySearchQuery, PerplexitySearchRecencyFilter, PerplexitySearchResponse,
    PerplexitySearchResult, gateway_tools, parallel_search, parallel_search_tool_factory,
    perplexity_search, perplexity_search_tool_factory,
};
pub use vercel_ai_gateway::{
    VERCEL_AI_GATEWAY_OPENAI_COMPATIBLE_BASE_URL, VercelAiGatewayOpenAICompatibleProvider,
    VercelAiGatewayOpenAICompatibleSettings, create_vercel_ai_gateway_openai_compatible,
    vercel_ai_gateway_auth_token_with_env, vercel_ai_gateway_openai_compatible,
    vercel_ai_gateway_openai_compatible_embedding, vercel_ai_gateway_openai_compatible_image,
    vercel_ai_gateway_openai_responses,
};
