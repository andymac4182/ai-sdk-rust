use serde::{Deserialize, Serialize};
use serde_json::json;

use ai_sdk_provider::json::{JsonObject, JsonSchema, JsonValue};
use ai_sdk_provider_utils::{
    ProviderExecutedToolFactory, Tool, create_provider_executed_tool_factory,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PerplexitySearchRecencyFilter {
    Day,
    Week,
    Month,
    Year,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerplexitySearchConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens_per_page: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_domain_filter: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_language_filter: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_recency_filter: Option<PerplexitySearchRecencyFilter>,
}

impl PerplexitySearchConfig {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum PerplexitySearchQuery {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PerplexitySearchInput {
    pub query: PerplexitySearchQuery,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens_per_page: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_domain_filter: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_language_filter: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_after_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_before_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated_after_filter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated_before_filter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_recency_filter: Option<PerplexitySearchRecencyFilter>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerplexitySearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PerplexitySearchResponse {
    pub results: Vec<PerplexitySearchResult>,
    pub id: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PerplexitySearchErrorType {
    ApiError,
    RateLimit,
    Timeout,
    InvalidInput,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PerplexitySearchError {
    pub error: PerplexitySearchErrorType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum PerplexitySearchOutput {
    Response(PerplexitySearchResponse),
    Error(PerplexitySearchError),
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParallelSearchSourcePolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_domains: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_domains: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_date: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParallelSearchExcerpts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars_per_result: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars_total: Option<u32>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParallelSearchFetchPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age_seconds: Option<u32>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParallelSearchMode {
    OneShot,
    Agentic,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParallelSearchConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ParallelSearchMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_policy: Option<ParallelSearchSourcePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpts: Option<ParallelSearchExcerpts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetch_policy: Option<ParallelSearchFetchPolicy>,
}

impl ParallelSearchConfig {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ParallelSearchInputSourcePolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_domains: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_domains: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_date: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ParallelSearchInputExcerpts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars_per_result: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars_total: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ParallelSearchInputFetchPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age_seconds: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ParallelSearchInput {
    pub objective: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_queries: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ParallelSearchMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_policy: Option<ParallelSearchInputSourcePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpts: Option<ParallelSearchInputExcerpts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetch_policy: Option<ParallelSearchInputFetchPolicy>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParallelSearchResult {
    pub url: String,
    pub title: String,
    pub excerpt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publish_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relevance_score: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParallelSearchResponse {
    pub search_id: String,
    pub results: Vec<ParallelSearchResult>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ParallelSearchErrorType {
    ApiError,
    RateLimit,
    Timeout,
    InvalidInput,
    ConfigurationError,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParallelSearchError {
    pub error: ParallelSearchErrorType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ParallelSearchOutput {
    Response(ParallelSearchResponse),
    Error(ParallelSearchError),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GatewayTools;

impl GatewayTools {
    pub const fn new() -> Self {
        Self
    }

    pub fn perplexity_search(
        self,
        name: impl Into<String>,
        config: PerplexitySearchConfig,
    ) -> Tool {
        perplexity_search(name, config)
    }

    pub fn parallel_search(self, name: impl Into<String>, config: ParallelSearchConfig) -> Tool {
        parallel_search(name, config)
    }
}

pub fn gateway_tools() -> GatewayTools {
    GatewayTools::new()
}

pub fn perplexity_search_tool_factory() -> ProviderExecutedToolFactory {
    create_provider_executed_tool_factory(
        "gateway.perplexity_search",
        perplexity_search_input_schema(),
        perplexity_search_output_schema(),
    )
}

pub fn perplexity_search(name: impl Into<String>, config: PerplexitySearchConfig) -> Tool {
    perplexity_search_tool_factory().tool(name, serialized_object(config))
}

pub fn parallel_search_tool_factory() -> ProviderExecutedToolFactory {
    create_provider_executed_tool_factory(
        "gateway.parallel_search",
        parallel_search_input_schema(),
        parallel_search_output_schema(),
    )
}

pub fn parallel_search(name: impl Into<String>, config: ParallelSearchConfig) -> Tool {
    parallel_search_tool_factory().tool(name, serialized_object(config))
}

fn serialized_object(value: impl Serialize) -> JsonObject {
    serde_json::to_value(value)
        .expect("gateway tool config serializes")
        .as_object()
        .expect("gateway tool config serializes to an object")
        .clone()
}

fn schema(value: JsonValue) -> JsonSchema {
    value
        .as_object()
        .expect("gateway tool schema is an object")
        .clone()
}

fn perplexity_search_input_schema() -> JsonSchema {
    schema(json!({
        "type": "object",
        "properties": {
            "query": {
                "anyOf": [
                    { "type": "string" },
                    { "type": "array", "items": { "type": "string" } }
                ],
                "description": "Search query (string) or multiple queries (array of up to 5 strings). Multi-query searches return combined results from all queries."
            },
            "max_results": {
                "type": "number",
                "description": "Maximum number of search results to return (1-20, default: 10)"
            },
            "max_tokens_per_page": {
                "type": "number",
                "description": "Maximum number of tokens to extract per search result page (256-2048, default: 2048)"
            },
            "max_tokens": {
                "type": "number",
                "description": "Maximum total tokens across all search results (default: 25000, max: 1000000)"
            },
            "country": {
                "type": "string",
                "description": "Two-letter ISO 3166-1 alpha-2 country code for regional search results (e.g., 'US', 'GB', 'FR')"
            },
            "search_domain_filter": {
                "type": "array",
                "items": { "type": "string" },
                "description": "List of domains to include or exclude from search results (max 20)."
            },
            "search_language_filter": {
                "type": "array",
                "items": { "type": "string" },
                "description": "List of ISO 639-1 language codes to filter results (max 10, lowercase)."
            },
            "search_after_date": { "type": "string" },
            "search_before_date": { "type": "string" },
            "last_updated_after_filter": { "type": "string" },
            "last_updated_before_filter": { "type": "string" },
            "search_recency_filter": {
                "type": "string",
                "enum": ["day", "week", "month", "year"]
            }
        },
        "required": ["query"]
    }))
}

fn perplexity_search_output_schema() -> JsonSchema {
    schema(json!({
        "anyOf": [
            {
                "type": "object",
                "properties": {
                    "results": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "title": { "type": "string" },
                                "url": { "type": "string" },
                                "snippet": { "type": "string" },
                                "date": { "type": "string" },
                                "lastUpdated": { "type": "string" }
                            },
                            "required": ["title", "url", "snippet"]
                        }
                    },
                    "id": { "type": "string" }
                },
                "required": ["results", "id"]
            },
            {
                "type": "object",
                "properties": {
                    "error": {
                        "type": "string",
                        "enum": ["api_error", "rate_limit", "timeout", "invalid_input", "unknown"]
                    },
                    "statusCode": { "type": "number" },
                    "message": { "type": "string" }
                },
                "required": ["error", "message"]
            }
        ]
    }))
}

fn parallel_search_input_schema() -> JsonSchema {
    schema(json!({
        "type": "object",
        "properties": {
            "objective": {
                "type": "string",
                "description": "Natural-language description of the web research goal, including source or freshness guidance and broader context from the task. Maximum 5000 characters."
            },
            "search_queries": {
                "type": "array",
                "items": { "type": "string" },
                "description": "Optional search queries to supplement the objective. Maximum 200 characters per query."
            },
            "mode": {
                "type": "string",
                "enum": ["one-shot", "agentic"],
                "description": "Mode preset: \"one-shot\" for comprehensive results with longer excerpts (default), \"agentic\" for concise, token-efficient results for multi-step workflows."
            },
            "max_results": {
                "type": "number",
                "description": "Maximum number of results to return (1-20). Defaults to 10 if not specified."
            },
            "source_policy": {
                "type": "object",
                "properties": {
                    "include_domains": { "type": "array", "items": { "type": "string" } },
                    "exclude_domains": { "type": "array", "items": { "type": "string" } },
                    "after_date": { "type": "string" }
                }
            },
            "excerpts": {
                "type": "object",
                "properties": {
                    "max_chars_per_result": { "type": "number" },
                    "max_chars_total": { "type": "number" }
                }
            },
            "fetch_policy": {
                "type": "object",
                "properties": {
                    "max_age_seconds": { "type": "number" }
                }
            }
        },
        "required": ["objective"]
    }))
}

fn parallel_search_output_schema() -> JsonSchema {
    schema(json!({
        "anyOf": [
            {
                "type": "object",
                "properties": {
                    "searchId": { "type": "string" },
                    "results": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "url": { "type": "string" },
                                "title": { "type": "string" },
                                "excerpt": { "type": "string" },
                                "publishDate": { "type": ["string", "null"] },
                                "relevanceScore": { "type": "number" }
                            },
                            "required": ["url", "title", "excerpt"]
                        }
                    }
                },
                "required": ["searchId", "results"]
            },
            {
                "type": "object",
                "properties": {
                    "error": {
                        "type": "string",
                        "enum": [
                            "api_error",
                            "rate_limit",
                            "timeout",
                            "invalid_input",
                            "configuration_error",
                            "unknown"
                        ]
                    },
                    "statusCode": { "type": "number" },
                    "message": { "type": "string" }
                },
                "required": ["error", "message"]
            }
        ]
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        GatewayTools, ParallelSearchConfig, ParallelSearchExcerpts, ParallelSearchFetchPolicy,
        ParallelSearchMode, ParallelSearchSourcePolicy, PerplexitySearchConfig,
        PerplexitySearchRecencyFilter, gateway_tools, parallel_search_tool_factory,
        perplexity_search_tool_factory,
    };

    #[test]
    fn perplexity_search_tool_factory_matches_gateway_provider_tool_contract() {
        let factory = perplexity_search_tool_factory();

        assert_eq!(factory.id, "gateway.perplexity_search");
        assert_eq!(
            factory
                .input_schema
                .get("required")
                .and_then(|value| value.as_array())
                .and_then(|required| required.first())
                .and_then(|value| value.as_str()),
            Some("query")
        );
        assert!(
            factory
                .input_schema
                .get("properties")
                .and_then(|value| value.as_object())
                .is_some_and(|properties| properties.contains_key("search_recency_filter"))
        );
        assert!(
            factory
                .output_schema
                .get("anyOf")
                .and_then(|value| value.as_array())
                .is_some_and(|variants| variants.len() == 2)
        );
    }

    #[test]
    fn parallel_search_tool_factory_matches_gateway_provider_tool_contract() {
        let factory = parallel_search_tool_factory();

        assert_eq!(factory.id, "gateway.parallel_search");
        assert_eq!(
            factory
                .input_schema
                .get("required")
                .and_then(|value| value.as_array())
                .and_then(|required| required.first())
                .and_then(|value| value.as_str()),
            Some("objective")
        );
        assert!(
            factory
                .input_schema
                .get("properties")
                .and_then(|value| value.as_object())
                .is_some_and(|properties| properties.contains_key("source_policy"))
        );
        assert!(
            factory
                .output_schema
                .get("anyOf")
                .and_then(|value| value.as_array())
                .is_some_and(|variants| variants.len() == 2)
        );
    }

    #[test]
    fn gateway_tools_create_provider_executed_perplexity_search_tool() {
        let tool = GatewayTools::new().perplexity_search(
            "perplexitySearch",
            PerplexitySearchConfig {
                max_results: Some(7),
                max_tokens_per_page: Some(1024),
                max_tokens: Some(50_000),
                country: Some("US".to_string()),
                search_domain_filter: Some(vec!["nature.com".to_string()]),
                search_language_filter: Some(vec!["en".to_string()]),
                search_recency_filter: Some(PerplexitySearchRecencyFilter::Week),
            },
        );

        assert!(tool.is_provider_tool());
        assert!(tool.is_provider_executed());
        assert_eq!(tool.provider_tool_id(), Some("gateway.perplexity_search"));
        assert_eq!(
            tool.provider_tool_args(),
            json!({
                "maxResults": 7,
                "maxTokensPerPage": 1024,
                "maxTokens": 50000,
                "country": "US",
                "searchDomainFilter": ["nature.com"],
                "searchLanguageFilter": ["en"],
                "searchRecencyFilter": "week"
            })
            .as_object()
        );
    }

    #[test]
    fn gateway_tools_create_provider_executed_parallel_search_tool() {
        let tool = gateway_tools().parallel_search(
            "parallelSearch",
            ParallelSearchConfig {
                mode: Some(ParallelSearchMode::Agentic),
                max_results: Some(5),
                source_policy: Some(ParallelSearchSourcePolicy {
                    include_domains: Some(vec!["docs.rs".to_string()]),
                    exclude_domains: Some(vec!["example.com".to_string()]),
                    after_date: Some("2024-01-01".to_string()),
                }),
                excerpts: Some(ParallelSearchExcerpts {
                    max_chars_per_result: Some(1200),
                    max_chars_total: Some(6000),
                }),
                fetch_policy: Some(ParallelSearchFetchPolicy {
                    max_age_seconds: Some(0),
                }),
            },
        );

        assert!(tool.is_provider_tool());
        assert!(tool.is_provider_executed());
        assert_eq!(tool.provider_tool_id(), Some("gateway.parallel_search"));
        assert_eq!(
            tool.provider_tool_args(),
            json!({
                "mode": "agentic",
                "maxResults": 5,
                "sourcePolicy": {
                    "includeDomains": ["docs.rs"],
                    "excludeDomains": ["example.com"],
                    "afterDate": "2024-01-01"
                },
                "excerpts": {
                    "maxCharsPerResult": 1200,
                    "maxCharsTotal": 6000
                },
                "fetchPolicy": {
                    "maxAgeSeconds": 0
                }
            })
            .as_object()
        );
    }
}
