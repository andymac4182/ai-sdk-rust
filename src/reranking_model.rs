use serde::{Deserialize, Serialize};
use std::future::Future;
use time::OffsetDateTime;

use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
use crate::warning::Warning;

/// A provider-v4 reranking model.
///
/// The upstream TypeScript contract exposes a `doRerank` method returning a
/// `PromiseLike<RerankingModelV4Result>`. This Rust trait maps that boundary to
/// an associated [`Future`] without introducing an async-trait dependency.
pub trait RerankingModel {
    /// Future returned by [`RerankingModel::do_rerank`].
    type RerankFuture<'a>: Future<Output = RerankingModelResult> + Send + 'a
    where
        Self: 'a;

    /// Returns the provider/model interface version implemented by this model.
    fn specification_version(&self) -> SpecificationVersion {
        SpecificationVersion::V4
    }

    /// Returns the provider identifier.
    fn provider(&self) -> &str;

    /// Returns the provider-specific model id.
    fn model_id(&self) -> &str;

    /// Reranks documents against the supplied query options.
    fn do_rerank(&self, options: RerankingModelCallOptions) -> Self::RerankFuture<'_>;
}

/// Documents passed to a reranking model provider call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum RerankingModelDocuments {
    /// Text documents to rerank.
    Text { values: Vec<String> },

    /// JSON object documents to rerank.
    Object { values: Vec<JsonObject> },
}

impl RerankingModelDocuments {
    /// Creates text documents for reranking.
    pub fn text(values: Vec<String>) -> Self {
        Self::Text { values }
    }

    /// Creates object documents for reranking.
    pub fn object(values: Vec<JsonObject>) -> Self {
        Self::Object { values }
    }
}

/// Options passed to a reranking model provider call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankingModelCallOptions {
    /// Documents to rerank.
    pub documents: RerankingModelDocuments,

    /// Query to rerank the documents against.
    pub query: String,

    /// Optional limit for returned documents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_n: Option<u64>,

    /// Provider-specific options passed through to the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,

    /// Additional HTTP headers for HTTP-based providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,
}

impl RerankingModelCallOptions {
    /// Creates reranking model call options with the required documents and query.
    pub fn new(documents: RerankingModelDocuments, query: impl Into<String>) -> Self {
        Self {
            documents,
            query: query.into(),
            top_n: None,
            provider_options: None,
            headers: None,
        }
    }

    /// Sets the maximum number of returned ranked documents.
    pub fn with_top_n(mut self, top_n: u64) -> Self {
        self.top_n = Some(top_n);
        self
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    /// Adds an HTTP header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }
}

/// Ranking entry returned by a reranking model.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankingModelRanking {
    /// Index of the document in the original input list before reranking.
    pub index: usize,

    /// Relevance score for the document after reranking.
    pub relevance_score: f64,
}

impl RerankingModelRanking {
    /// Creates a ranking entry.
    pub fn new(index: usize, relevance_score: f64) -> Self {
        Self {
            index,
            relevance_score,
        }
    }
}

/// Optional response information for debugging reranking calls.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankingModelResponse {
    /// Provider response identifier, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Start timestamp for the response, when one is available.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "time::serde::rfc3339::option"
    )]
    pub timestamp: Option<OffsetDateTime>,

    /// Provider model identifier used for the response, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,

    /// Response headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Provider response body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonValue>,
}

impl RerankingModelResponse {
    /// Creates empty reranking response metadata.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the provider response identifier.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the response start timestamp.
    pub fn with_timestamp(mut self, timestamp: OffsetDateTime) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Sets the provider model identifier used for the response.
    pub fn with_model_id(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = Some(model_id.into());
        self
    }

    /// Adds a response header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }

    /// Sets the raw provider response body.
    pub fn with_body(mut self, body: JsonValue) -> Self {
        self.body = Some(body);
        self
    }
}

/// Result of a reranking model provider call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankingModelResult {
    /// Ranked document references sorted by descending relevance.
    pub ranking: Vec<RerankingModelRanking>,

    /// Provider-specific metadata returned by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Warnings for the call, e.g. unsupported settings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<Warning>,

    /// Optional response information for debugging purposes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<RerankingModelResponse>,
}

impl RerankingModelResult {
    /// Creates a reranking model result with no warnings.
    pub fn new(ranking: Vec<RerankingModelRanking>) -> Self {
        Self {
            ranking,
            provider_metadata: None,
            warnings: Vec::new(),
            response: None,
        }
    }

    /// Adds provider-specific metadata.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }

    /// Adds a warning returned by the provider.
    pub fn with_warning(mut self, warning: Warning) -> Self {
        self.warnings.push(warning);
        self
    }

    /// Sets optional response information.
    pub fn with_response(mut self, response: RerankingModelResponse) -> Self {
        self.response = Some(response);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RerankingModel, RerankingModelCallOptions, RerankingModelDocuments, RerankingModelRanking,
        RerankingModelResponse, RerankingModelResult,
    };
    use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
    use crate::warning::Warning;
    use serde_json::json;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    struct StaticRerankingModel;

    impl RerankingModel for StaticRerankingModel {
        type RerankFuture<'a>
            = Ready<RerankingModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "test-provider"
        }

        fn model_id(&self) -> &str {
            "rerank-test"
        }

        fn do_rerank(&self, _options: RerankingModelCallOptions) -> Self::RerankFuture<'_> {
            ready(RerankingModelResult::new(vec![
                RerankingModelRanking::new(1, 0.91),
                RerankingModelRanking::new(0, 0.82),
            ]))
        }
    }

    fn poll_ready<T>(mut future: Ready<T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("std::future::Ready never returns pending"),
        }
    }

    #[test]
    fn call_options_serializes_text_documents_with_top_n_and_provider_options() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "cohere": {
                "returnDocuments": true
            }
        }))
        .expect("provider options deserialize");

        let options = RerankingModelCallOptions::new(
            RerankingModelDocuments::text(vec![
                "First document".to_string(),
                "Second document".to_string(),
            ]),
            "search query",
        )
        .with_top_n(1)
        .with_provider_options(provider_options)
        .with_header("x-request-id", "req_123");

        assert_eq!(
            serde_json::to_value(options).expect("call options serialize"),
            json!({
                "documents": {
                    "type": "text",
                    "values": ["First document", "Second document"]
                },
                "query": "search query",
                "topN": 1,
                "providerOptions": {
                    "cohere": {
                        "returnDocuments": true
                    }
                },
                "headers": {
                    "x-request-id": "req_123"
                }
            })
        );
    }

    #[test]
    fn call_options_deserializes_object_documents_and_omits_missing_options() {
        let options: RerankingModelCallOptions = serde_json::from_value(json!({
            "documents": {
                "type": "object",
                "values": [
                    {
                        "id": "doc_1",
                        "text": "First document"
                    }
                ]
            },
            "query": "search query"
        }))
        .expect("call options deserialize");

        assert_eq!(
            options,
            RerankingModelCallOptions::new(
                RerankingModelDocuments::object(vec![serde_json::Map::from_iter([
                    ("id".to_string(), json!("doc_1")),
                    ("text".to_string(), json!("First document")),
                ])]),
                "search query",
            )
        );
        assert_eq!(
            serde_json::to_value(options).expect("call options serialize"),
            json!({
                "documents": {
                    "type": "object",
                    "values": [
                        {
                            "id": "doc_1",
                            "text": "First document"
                        }
                    ]
                },
                "query": "search query"
            })
        );
    }

    #[test]
    fn reranking_model_trait_exposes_upstream_v4_identity_and_rerank_boundary() {
        let model = StaticRerankingModel;
        let options = RerankingModelCallOptions::new(
            RerankingModelDocuments::text(vec![
                "First document".to_string(),
                "Second document".to_string(),
            ]),
            "search query",
        );

        let result = poll_ready(model.do_rerank(options));

        assert_eq!(model.specification_version(), SpecificationVersion::V4);
        assert_eq!(model.provider(), "test-provider");
        assert_eq!(model.model_id(), "rerank-test");
        assert_eq!(
            result.ranking,
            vec![
                RerankingModelRanking::new(1, 0.91),
                RerankingModelRanking::new(0, 0.82)
            ]
        );
    }

    #[test]
    fn result_serializes_ranking_response_metadata_and_warnings() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "cohere": {
                "searchUnits": 1
            }
        }))
        .expect("provider metadata deserialize");
        let response: RerankingModelResponse = serde_json::from_value(json!({
            "id": "rerank_123",
            "timestamp": "2024-01-02T03:04:05Z",
            "modelId": "rerank-english-v3.0",
            "headers": {
                "x-ratelimit-remaining": "99"
            },
            "body": {
                "meta": {
                    "apiVersion": "v1"
                }
            }
        }))
        .expect("response deserialize");

        let result = RerankingModelResult::new(vec![
            RerankingModelRanking::new(1, 0.91),
            RerankingModelRanking::new(0, 0.82),
        ])
        .with_provider_metadata(provider_metadata)
        .with_response(response)
        .with_warning(Warning::Unsupported {
            feature: "topN".to_string(),
            details: Some("The selected model returns all documents.".to_string()),
        });

        assert_eq!(
            serde_json::to_value(result).expect("result serialize"),
            json!({
                "ranking": [
                    {
                        "index": 1,
                        "relevanceScore": 0.91
                    },
                    {
                        "index": 0,
                        "relevanceScore": 0.82
                    }
                ],
                "providerMetadata": {
                    "cohere": {
                        "searchUnits": 1
                    }
                },
                "warnings": [
                    {
                        "type": "unsupported",
                        "feature": "topN",
                        "details": "The selected model returns all documents."
                    }
                ],
                "response": {
                    "id": "rerank_123",
                    "timestamp": "2024-01-02T03:04:05Z",
                    "modelId": "rerank-english-v3.0",
                    "headers": {
                        "x-ratelimit-remaining": "99"
                    },
                    "body": {
                        "meta": {
                            "apiVersion": "v1"
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn result_deserializes_without_optional_warnings_and_omits_empty_warnings() {
        let result: RerankingModelResult = serde_json::from_value(json!({
            "ranking": [
                {
                    "index": 0,
                    "relevanceScore": 0.77
                }
            ]
        }))
        .expect("result deserialize");

        assert_eq!(
            result,
            RerankingModelResult::new(vec![RerankingModelRanking::new(0, 0.77)])
        );
        assert_eq!(
            serde_json::to_value(result).expect("result serialize"),
            json!({
                "ranking": [
                    {
                        "index": 0,
                        "relevanceScore": 0.77
                    }
                ]
            })
        );
    }
}
