//! Gateway provider helpers for the Rust port of upstream `@ai-sdk/gateway`.

#![forbid(unsafe_code)]

pub mod gateway_error;
pub mod gateway_tools;

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
