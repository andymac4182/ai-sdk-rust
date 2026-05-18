use serde::{Deserialize, Serialize};
use std::future::Future;

use crate::headers::Headers;
use crate::json::JsonValue;
use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
use crate::warning::Warning;

/// A text embedding vector returned by an embedding model.
pub type EmbeddingModelEmbedding = Vec<f64>;

/// A provider-v4 embedding model.
///
/// The upstream TypeScript contract exposes capability properties that may be
/// `PromiseLike` plus a `doEmbed` method returning a
/// `PromiseLike<EmbeddingModelV4Result>`. This Rust trait maps those boundaries
/// to associated [`Future`] types without introducing an async-trait dependency.
pub trait EmbeddingModel {
    /// Future returned by [`EmbeddingModel::max_embeddings_per_call`].
    type MaxEmbeddingsPerCallFuture<'a>: Future<Output = Option<usize>> + Send + 'a
    where
        Self: 'a;

    /// Future returned by [`EmbeddingModel::supports_parallel_calls`].
    type SupportsParallelCallsFuture<'a>: Future<Output = bool> + Send + 'a
    where
        Self: 'a;

    /// Future returned by [`EmbeddingModel::do_embed`].
    type EmbedFuture<'a>: Future<Output = EmbeddingModelResult> + Send + 'a
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

    /// Returns the maximum number of embeddings supported in one call.
    ///
    /// `None` represents the upstream `undefined` or unbounded case.
    fn max_embeddings_per_call(&self) -> Self::MaxEmbeddingsPerCallFuture<'_>;

    /// Returns whether the model can handle multiple embedding calls in parallel.
    fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_>;

    /// Generates embeddings for the supplied text values.
    fn do_embed(&self, options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_>;
}

/// Options passed to an embedding model provider call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingModelCallOptions {
    /// Text values to generate embeddings for.
    pub values: Vec<String>,

    /// Provider-specific options passed through to the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,

    /// Additional HTTP headers for HTTP-based providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,
}

impl EmbeddingModelCallOptions {
    /// Creates embedding model call options with the required text values.
    pub fn new(values: Vec<String>) -> Self {
        Self {
            values,
            provider_options: None,
            headers: None,
        }
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

/// Token usage for an embedding model call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EmbeddingModelUsage {
    /// Input tokens used to generate the embeddings.
    pub tokens: u64,
}

impl EmbeddingModelUsage {
    /// Creates embedding usage from the input token count.
    pub fn new(tokens: u64) -> Self {
        Self { tokens }
    }
}

/// Optional response information for debugging embedding calls.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingModelResponse {
    /// Response headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Provider response body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonValue>,
}

impl EmbeddingModelResponse {
    /// Creates empty embedding response metadata.
    pub fn new() -> Self {
        Self::default()
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

/// Result of an embedding model provider call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingModelResult {
    /// Generated embeddings in the same order as the input values.
    pub embeddings: Vec<EmbeddingModelEmbedding>,

    /// Token usage for the embedding call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<EmbeddingModelUsage>,

    /// Provider-specific metadata returned by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Optional response information for debugging purposes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<EmbeddingModelResponse>,

    /// Warnings for the call, e.g. unsupported settings.
    pub warnings: Vec<Warning>,
}

impl EmbeddingModelResult {
    /// Creates an embedding model result with no warnings.
    pub fn new(embeddings: Vec<EmbeddingModelEmbedding>) -> Self {
        Self {
            embeddings,
            usage: None,
            provider_metadata: None,
            response: None,
            warnings: Vec::new(),
        }
    }

    /// Sets token usage for the embedding call.
    pub fn with_usage(mut self, usage: EmbeddingModelUsage) -> Self {
        self.usage = Some(usage);
        self
    }

    /// Adds provider-specific metadata.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }

    /// Sets optional response information.
    pub fn with_response(mut self, response: EmbeddingModelResponse) -> Self {
        self.response = Some(response);
        self
    }

    /// Adds a warning returned by the provider.
    pub fn with_warning(mut self, warning: Warning) -> Self {
        self.warnings.push(warning);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResponse, EmbeddingModelResult,
        EmbeddingModelUsage,
    };
    use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
    use crate::warning::Warning;
    use serde_json::json;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    struct StaticEmbeddingModel;

    impl EmbeddingModel for StaticEmbeddingModel {
        type MaxEmbeddingsPerCallFuture<'a>
            = Ready<Option<usize>>
        where
            Self: 'a;

        type SupportsParallelCallsFuture<'a>
            = Ready<bool>
        where
            Self: 'a;

        type EmbedFuture<'a>
            = Ready<EmbeddingModelResult>
        where
            Self: 'a;

        fn provider(&self) -> &str {
            "test-provider"
        }

        fn model_id(&self) -> &str {
            "embedding-test"
        }

        fn max_embeddings_per_call(&self) -> Self::MaxEmbeddingsPerCallFuture<'_> {
            ready(Some(2))
        }

        fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
            ready(true)
        }

        fn do_embed(&self, _options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
            ready(EmbeddingModelResult::new(vec![vec![0.1, 0.2]]))
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
    fn call_options_serializes_upstream_shape_with_headers_and_provider_options() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "dimensions": 512
            }
        }))
        .expect("provider options deserialize");

        let options = EmbeddingModelCallOptions::new(vec![
            "first value".to_string(),
            "second value".to_string(),
        ])
        .with_provider_options(provider_options)
        .with_header("x-request-id", "req_123");

        assert_eq!(
            serde_json::to_value(options).expect("call options serialize"),
            json!({
                "values": ["first value", "second value"],
                "providerOptions": {
                    "openai": {
                        "dimensions": 512
                    }
                },
                "headers": {
                    "x-request-id": "req_123"
                }
            })
        );
    }

    #[test]
    fn call_options_deserializes_minimal_values_and_omits_missing_options() {
        let options: EmbeddingModelCallOptions = serde_json::from_value(json!({
            "values": ["search query"]
        }))
        .expect("call options deserialize");

        assert_eq!(
            options,
            EmbeddingModelCallOptions::new(vec!["search query".to_string()])
        );
        assert_eq!(
            serde_json::to_value(options).expect("call options serialize"),
            json!({
                "values": ["search query"]
            })
        );
    }

    #[test]
    fn embedding_model_trait_exposes_upstream_v4_identity_capabilities_and_embed_boundary() {
        let model = StaticEmbeddingModel;
        let options = EmbeddingModelCallOptions::new(vec!["search query".to_string()]);

        let result = poll_ready(model.do_embed(options));

        assert_eq!(model.specification_version(), SpecificationVersion::V4);
        assert_eq!(model.provider(), "test-provider");
        assert_eq!(model.model_id(), "embedding-test");
        assert_eq!(poll_ready(model.max_embeddings_per_call()), Some(2));
        assert!(poll_ready(model.supports_parallel_calls()));
        assert_eq!(result.embeddings, vec![vec![0.1, 0.2]]);
    }

    #[test]
    fn result_serializes_embeddings_usage_response_metadata_and_warnings() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "model": "text-embedding-3-small"
            }
        }))
        .expect("provider metadata deserialize");

        let result = EmbeddingModelResult::new(vec![vec![0.1, 0.2], vec![0.3, 0.4]])
            .with_usage(EmbeddingModelUsage::new(42))
            .with_provider_metadata(provider_metadata)
            .with_response(
                EmbeddingModelResponse::new()
                    .with_header("x-ratelimit-remaining", "99")
                    .with_body(json!({
                        "id": "emb_123"
                    })),
            )
            .with_warning(Warning::Unsupported {
                feature: "dimensions".to_string(),
                details: Some("The selected model uses a fixed dimension count.".to_string()),
            });

        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "embeddings": [[0.1, 0.2], [0.3, 0.4]],
                "usage": {
                    "tokens": 42
                },
                "providerMetadata": {
                    "openai": {
                        "model": "text-embedding-3-small"
                    }
                },
                "response": {
                    "headers": {
                        "x-ratelimit-remaining": "99"
                    },
                    "body": {
                        "id": "emb_123"
                    }
                },
                "warnings": [
                    {
                        "type": "unsupported",
                        "feature": "dimensions",
                        "details": "The selected model uses a fixed dimension count."
                    }
                ]
            })
        );
    }

    #[test]
    fn result_deserializes_empty_warnings_and_omits_optional_fields() {
        let result: EmbeddingModelResult = serde_json::from_value(json!({
            "embeddings": [[1.0, 2.0, 3.0]],
            "warnings": []
        }))
        .expect("result deserializes");

        assert_eq!(result, EmbeddingModelResult::new(vec![vec![1.0, 2.0, 3.0]]));
        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "embeddings": [[1.0, 2.0, 3.0]],
                "warnings": []
            })
        );
    }
}
