use serde::{Deserialize, Serialize};

use crate::VERSION;
use crate::embedding_model::{
    EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelEmbedding, EmbeddingModelResponse,
    EmbeddingModelResult, EmbeddingModelUsage,
};
use crate::headers::Headers;
use crate::provider::ProviderMetadata;
use crate::provider::ProviderOptions;
use crate::provider_utils::with_user_agent_suffix;
use crate::warning::Warning;

/// Embedding vector returned by high-level embed operations.
pub type Embedding = EmbeddingModelEmbedding;

/// Options for a high-level `embed` call.
pub struct EmbedOptions<'a, M: EmbeddingModel + ?Sized> {
    /// Embedding model used for the call.
    pub model: &'a M,

    /// The value to embed.
    pub value: String,

    /// Provider-specific options passed through to the model.
    pub provider_options: Option<ProviderOptions>,

    /// Additional HTTP headers for HTTP-based providers.
    pub headers: Option<Headers>,
}

impl<'a, M: EmbeddingModel + ?Sized> EmbedOptions<'a, M> {
    /// Creates options for a high-level `embed` call.
    pub fn new(model: &'a M, value: impl Into<String>) -> Self {
        Self {
            model,
            value: value.into(),
            provider_options: None,
            headers: None,
        }
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    /// Sets all additional HTTP headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Adds an additional HTTP header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }
}

/// Options for a high-level `embedMany` call.
pub struct EmbedManyOptions<'a, M: EmbeddingModel + ?Sized> {
    /// Embedding model used for the call.
    pub model: &'a M,

    /// The values to embed.
    pub values: Vec<String>,

    /// Provider-specific options passed through to the model.
    pub provider_options: Option<ProviderOptions>,

    /// Additional HTTP headers for HTTP-based providers.
    pub headers: Option<Headers>,
}

impl<'a, M: EmbeddingModel + ?Sized> EmbedManyOptions<'a, M> {
    /// Creates options for a high-level `embedMany` call.
    pub fn new<T, I>(model: &'a M, values: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        Self {
            model,
            values: values.into_iter().map(Into::into).collect(),
            provider_options: None,
            headers: None,
        }
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    /// Sets all additional HTTP headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Adds an additional HTTP header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }
}

/// Result of a high-level `embed` call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedResult {
    /// The value that was embedded.
    pub value: String,

    /// The embedding of the value.
    pub embedding: Embedding,

    /// Token usage for the embedding operation.
    pub usage: EmbeddingModelUsage,

    /// Warnings for the call, e.g. unsupported settings.
    pub warnings: Vec<Warning>,

    /// Optional provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Optional provider response data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<EmbeddingModelResponse>,
}

impl EmbedResult {
    /// Creates an embed result with no warnings.
    pub fn new(value: impl Into<String>, embedding: Embedding, usage: EmbeddingModelUsage) -> Self {
        Self {
            value: value.into(),
            embedding,
            usage,
            warnings: Vec::new(),
            provider_metadata: None,
            response: None,
        }
    }

    /// Adds a warning returned by the provider.
    pub fn with_warning(mut self, warning: Warning) -> Self {
        self.warnings.push(warning);
        self
    }

    /// Adds provider-specific metadata.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }

    /// Sets optional provider response data.
    pub fn with_response(mut self, response: EmbeddingModelResponse) -> Self {
        self.response = Some(response);
        self
    }
}

/// Embeds one value using an embedding model.
pub async fn embed<M: EmbeddingModel + ?Sized>(options: EmbedOptions<'_, M>) -> EmbedResult {
    let EmbedOptions {
        model,
        value,
        provider_options,
        headers,
    } = options;
    let headers = headers_with_ai_user_agent(headers);
    let EmbeddingModelResult {
        embeddings,
        usage,
        provider_metadata,
        response,
        warnings,
    } = model
        .do_embed(embedding_call_options(
            vec![value.clone()],
            provider_options.as_ref(),
            &headers,
        ))
        .await;

    EmbedResult {
        value,
        embedding: embeddings.into_iter().next().unwrap_or_default(),
        usage: usage.unwrap_or_else(|| EmbeddingModelUsage::new(0)),
        warnings,
        provider_metadata,
        response,
    }
}

/// Result of a high-level `embedMany` call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedManyResult {
    /// The values that were embedded.
    pub values: Vec<String>,

    /// Embeddings in the same order as the values.
    pub embeddings: Vec<Embedding>,

    /// Token usage for the embedding operation.
    pub usage: EmbeddingModelUsage,

    /// Warnings for the call, e.g. unsupported settings.
    pub warnings: Vec<Warning>,

    /// Optional provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Optional raw response data for each provider call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responses: Option<Vec<Option<EmbeddingModelResponse>>>,
}

impl EmbedManyResult {
    /// Creates an embed-many result with no warnings.
    pub fn new(
        values: Vec<String>,
        embeddings: Vec<Embedding>,
        usage: EmbeddingModelUsage,
    ) -> Self {
        Self {
            values,
            embeddings,
            usage,
            warnings: Vec::new(),
            provider_metadata: None,
            responses: None,
        }
    }

    /// Adds a warning returned by the provider.
    pub fn with_warning(mut self, warning: Warning) -> Self {
        self.warnings.push(warning);
        self
    }

    /// Adds provider-specific metadata.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }

    /// Sets optional raw response data for each provider call.
    pub fn with_responses(mut self, responses: Vec<Option<EmbeddingModelResponse>>) -> Self {
        self.responses = Some(responses);
        self
    }
}

/// Embeds several values using an embedding model.
///
/// When the model exposes `max_embeddings_per_call`, values are split into
/// provider calls of that size and the high-level result aggregates embeddings,
/// usage, warnings, provider metadata, and raw responses in call order.
pub async fn embed_many<M: EmbeddingModel + ?Sized>(
    options: EmbedManyOptions<'_, M>,
) -> EmbedManyResult {
    let EmbedManyOptions {
        model,
        values,
        provider_options,
        headers,
    } = options;
    let headers = headers_with_ai_user_agent(headers);
    let max_embeddings_per_call = model.max_embeddings_per_call().await;
    // Upstream resolves this capability before deciding whether chunking is
    // needed. Parallel scheduling can be layered on without changing the public
    // result shape.
    let _supports_parallel_calls = model.supports_parallel_calls().await;

    let Some(chunk_size) = max_embeddings_per_call else {
        let EmbeddingModelResult {
            embeddings,
            usage,
            provider_metadata,
            response,
            warnings,
        } = model
            .do_embed(embedding_call_options(
                values.clone(),
                provider_options.as_ref(),
                &headers,
            ))
            .await;

        return EmbedManyResult {
            values,
            embeddings,
            usage: usage.unwrap_or_else(|| EmbeddingModelUsage::new(0)),
            warnings,
            provider_metadata,
            responses: Some(vec![response]),
        };
    };

    let mut embeddings = Vec::new();
    let mut warnings = Vec::new();
    let mut responses = Vec::new();
    let mut tokens = 0;
    let mut provider_metadata = None;

    for chunk in split_values(&values, chunk_size) {
        let EmbeddingModelResult {
            embeddings: chunk_embeddings,
            usage,
            provider_metadata: chunk_provider_metadata,
            response,
            warnings: chunk_warnings,
        } = model
            .do_embed(embedding_call_options(
                chunk,
                provider_options.as_ref(),
                &headers,
            ))
            .await;

        embeddings.extend(chunk_embeddings);
        warnings.extend(chunk_warnings);
        responses.push(response);
        tokens += usage.map_or(0, |usage| usage.tokens);

        if let Some(chunk_provider_metadata) = chunk_provider_metadata {
            merge_provider_metadata(&mut provider_metadata, chunk_provider_metadata);
        }
    }

    EmbedManyResult {
        values,
        embeddings,
        usage: EmbeddingModelUsage::new(tokens),
        warnings,
        provider_metadata,
        responses: Some(responses),
    }
}

fn embedding_call_options(
    values: Vec<String>,
    provider_options: Option<&ProviderOptions>,
    headers: &Headers,
) -> EmbeddingModelCallOptions {
    EmbeddingModelCallOptions {
        values,
        provider_options: provider_options.cloned(),
        headers: Some(headers.clone()),
    }
}

fn headers_with_ai_user_agent(headers: Option<Headers>) -> Headers {
    let header_entries: Vec<(String, Option<String>)> = headers
        .unwrap_or_default()
        .into_iter()
        .map(|(name, value)| (name, Some(value)))
        .collect();

    with_user_agent_suffix(Some(header_entries), [format!("ai/{VERSION}")])
}

fn split_values(values: &[String], chunk_size: usize) -> Vec<Vec<String>> {
    if chunk_size == 0 {
        return vec![values.to_vec()];
    }

    values.chunks(chunk_size).map(<[String]>::to_vec).collect()
}

fn merge_provider_metadata(
    provider_metadata: &mut Option<ProviderMetadata>,
    chunk_provider_metadata: ProviderMetadata,
) {
    let provider_metadata = provider_metadata.get_or_insert_with(ProviderMetadata::new);

    for (provider_name, metadata) in chunk_provider_metadata {
        provider_metadata
            .entry(provider_name)
            .or_default()
            .extend(metadata);
    }
}

#[cfg(test)]
mod tests {
    use super::{EmbedManyOptions, EmbedManyResult, EmbedOptions, EmbedResult, Embedding};
    use crate::embedding_model::{
        EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResponse, EmbeddingModelResult,
        EmbeddingModelUsage,
    };
    use crate::provider::{ProviderMetadata, ProviderOptions};
    use crate::warning::Warning;
    use serde_json::json;
    use std::collections::VecDeque;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::sync::Mutex;
    use std::task::{Context, Poll, Waker};

    struct RecordingEmbeddingModel {
        max_embeddings_per_call: Option<usize>,
        supports_parallel_calls: bool,
        calls: Mutex<Vec<EmbeddingModelCallOptions>>,
        results: Mutex<VecDeque<EmbeddingModelResult>>,
    }

    impl RecordingEmbeddingModel {
        fn new(
            max_embeddings_per_call: Option<usize>,
            supports_parallel_calls: bool,
            results: Vec<EmbeddingModelResult>,
        ) -> Self {
            Self {
                max_embeddings_per_call,
                supports_parallel_calls,
                calls: Mutex::new(Vec::new()),
                results: Mutex::new(results.into()),
            }
        }

        fn calls(&self) -> Vec<EmbeddingModelCallOptions> {
            self.calls
                .lock()
                .expect("calls lock is not poisoned")
                .clone()
        }
    }

    impl EmbeddingModel for RecordingEmbeddingModel {
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
            ready(self.max_embeddings_per_call)
        }

        fn supports_parallel_calls(&self) -> Self::SupportsParallelCallsFuture<'_> {
            ready(self.supports_parallel_calls)
        }

        fn do_embed(&self, options: EmbeddingModelCallOptions) -> Self::EmbedFuture<'_> {
            self.calls
                .lock()
                .expect("calls lock is not poisoned")
                .push(options.clone());
            let result = self
                .results
                .lock()
                .expect("results lock is not poisoned")
                .pop_front()
                .unwrap_or_else(|| {
                    EmbeddingModelResult::new(vec![Vec::new(); options.values.len()])
                });

            ready(result)
        }
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

    #[test]
    fn embed_calls_model_with_single_value_and_maps_result() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "openai": {
                "dimensions": 3
            }
        }))
        .expect("provider options deserialize");
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "embeddingModel": "text-embedding-3-small"
            }
        }))
        .expect("provider metadata deserialize");
        let response = EmbeddingModelResponse::new().with_header("x-request-id", "embed-request-1");
        let model = RecordingEmbeddingModel::new(
            None,
            true,
            vec![
                EmbeddingModelResult::new(vec![vec![0.1, 0.2, 0.3]])
                    .with_usage(EmbeddingModelUsage::new(7))
                    .with_warning(Warning::Unsupported {
                        feature: "truncate".to_string(),
                        details: None,
                    })
                    .with_provider_metadata(provider_metadata.clone())
                    .with_response(response.clone()),
            ],
        );

        let result = poll_ready(super::embed(
            EmbedOptions::new(&model, "sunrise")
                .with_provider_options(provider_options.clone())
                .with_header("User-Agent", "caller/1")
                .with_header("X-Test", "true"),
        ));

        assert_eq!(result.value, "sunrise");
        assert_eq!(result.embedding, vec![0.1, 0.2, 0.3]);
        assert_eq!(result.usage, EmbeddingModelUsage::new(7));
        assert_eq!(
            result.warnings,
            vec![Warning::Unsupported {
                feature: "truncate".to_string(),
                details: None,
            }]
        );
        assert_eq!(result.provider_metadata, Some(provider_metadata));
        assert_eq!(result.response, Some(response));

        let calls = model.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].values, vec!["sunrise"]);
        assert_eq!(calls[0].provider_options, Some(provider_options));

        let headers = calls[0].headers.as_ref().expect("headers are forwarded");
        assert_eq!(
            headers.get("user-agent").map(String::as_str),
            Some(concat!("caller/1 ai/", env!("CARGO_PKG_VERSION")))
        );
        assert_eq!(headers.get("x-test").map(String::as_str), Some("true"));
    }

    #[test]
    fn embed_many_without_model_limit_uses_one_model_call() {
        let response =
            EmbeddingModelResponse::new().with_header("x-request-id", "embed-many-request-1");
        let model = RecordingEmbeddingModel::new(
            None,
            true,
            vec![
                EmbeddingModelResult::new(vec![vec![0.1, 0.2], vec![0.3, 0.4]])
                    .with_usage(EmbeddingModelUsage::new(11))
                    .with_response(response.clone()),
            ],
        );

        let result = poll_ready(super::embed_many(EmbedManyOptions::new(
            &model,
            ["alpha", "beta"],
        )));

        assert_eq!(result.values, vec!["alpha", "beta"]);
        assert_eq!(result.embeddings, vec![vec![0.1, 0.2], vec![0.3, 0.4]]);
        assert_eq!(result.usage, EmbeddingModelUsage::new(11));
        assert_eq!(result.responses, Some(vec![Some(response)]));

        let calls = model.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].values, vec!["alpha", "beta"]);
        assert_eq!(
            calls[0]
                .headers
                .as_ref()
                .and_then(|headers| headers.get("user-agent"))
                .map(String::as_str),
            Some(concat!("ai/", env!("CARGO_PKG_VERSION")))
        );
    }

    #[test]
    fn embed_many_splits_limited_models_and_aggregates_results() {
        let first_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "first": true
            }
        }))
        .expect("provider metadata deserialize");
        let second_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "second": 2
            },
            "cohere": {
                "third": 3
            }
        }))
        .expect("provider metadata deserialize");
        let model = RecordingEmbeddingModel::new(
            Some(2),
            false,
            vec![
                EmbeddingModelResult::new(vec![vec![0.1], vec![0.2]])
                    .with_usage(EmbeddingModelUsage::new(3))
                    .with_warning(Warning::Other {
                        message: "first warning".to_string(),
                    })
                    .with_provider_metadata(first_metadata),
                EmbeddingModelResult::new(vec![vec![0.3], vec![0.4]])
                    .with_usage(EmbeddingModelUsage::new(5))
                    .with_provider_metadata(second_metadata)
                    .with_response(
                        EmbeddingModelResponse::new()
                            .with_header("x-request-id", "embed-many-request-2"),
                    ),
                EmbeddingModelResult::new(vec![vec![0.5]]),
            ],
        );

        let result = poll_ready(super::embed_many(
            EmbedManyOptions::new(&model, ["a", "b", "c", "d", "e"]).with_header("X-Trace", "1"),
        ));

        assert_eq!(result.values, vec!["a", "b", "c", "d", "e"]);
        assert_eq!(
            result.embeddings,
            vec![vec![0.1], vec![0.2], vec![0.3], vec![0.4], vec![0.5]]
        );
        assert_eq!(result.usage, EmbeddingModelUsage::new(8));
        assert_eq!(
            result.warnings,
            vec![Warning::Other {
                message: "first warning".to_string(),
            }]
        );
        assert_eq!(
            serde_json::to_value(result.provider_metadata).expect("metadata serializes"),
            json!({
                "openai": {
                    "first": true,
                    "second": 2
                },
                "cohere": {
                    "third": 3
                }
            })
        );
        assert_eq!(
            result.responses,
            Some(vec![
                None,
                Some(
                    EmbeddingModelResponse::new()
                        .with_header("x-request-id", "embed-many-request-2")
                ),
                None,
            ])
        );

        let calls = model.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].values, vec!["a", "b"]);
        assert_eq!(calls[1].values, vec!["c", "d"]);
        assert_eq!(calls[2].values, vec!["e"]);
        assert!(calls.iter().all(|call| {
            call.headers
                .as_ref()
                .is_some_and(|headers| headers.get("x-trace").map(String::as_str) == Some("1"))
        }));
    }

    #[test]
    fn embed_result_serializes_upstream_shape_with_metadata_and_response() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "embeddingModel": "text-embedding-3-small"
            }
        }))
        .expect("provider metadata deserializes");

        let result = EmbedResult::new("sunrise", vec![0.1, 0.2, 0.3], EmbeddingModelUsage::new(7))
            .with_warning(Warning::Unsupported {
                feature: "truncate".to_string(),
                details: Some("The selected model ignores truncate.".to_string()),
            })
            .with_provider_metadata(provider_metadata)
            .with_response(
                EmbeddingModelResponse::new()
                    .with_header("x-request-id", "req_123")
                    .with_body(json!({ "id": "emb_123" })),
            );

        assert_eq!(
            serde_json::to_value(result).expect("embed result serializes"),
            json!({
                "value": "sunrise",
                "embedding": [0.1, 0.2, 0.3],
                "usage": {
                    "tokens": 7
                },
                "warnings": [
                    {
                        "type": "unsupported",
                        "feature": "truncate",
                        "details": "The selected model ignores truncate."
                    }
                ],
                "providerMetadata": {
                    "openai": {
                        "embeddingModel": "text-embedding-3-small"
                    }
                },
                "response": {
                    "headers": {
                        "x-request-id": "req_123"
                    },
                    "body": {
                        "id": "emb_123"
                    }
                }
            })
        );
    }

    #[test]
    fn embed_result_deserializes_minimal_upstream_shape_and_omits_options() {
        let result: EmbedResult = serde_json::from_value(json!({
            "value": "sunrise",
            "embedding": [0.1, 0.2, 0.3],
            "usage": {
                "tokens": 7
            },
            "warnings": []
        }))
        .expect("embed result deserializes");

        assert_eq!(
            result,
            EmbedResult::new("sunrise", vec![0.1, 0.2, 0.3], EmbeddingModelUsage::new(7))
        );
        assert_eq!(
            serde_json::to_value(result).expect("embed result serializes"),
            json!({
                "value": "sunrise",
                "embedding": [0.1, 0.2, 0.3],
                "usage": {
                    "tokens": 7
                },
                "warnings": []
            })
        );
    }

    #[test]
    fn embed_many_result_serializes_upstream_shape_with_optional_responses() {
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "openai": {
                "dimensions": 3
            }
        }))
        .expect("provider metadata deserializes");
        let embeddings: Vec<Embedding> = vec![vec![0.1, 0.2, 0.3], vec![0.4, 0.5, 0.6]];

        let result = EmbedManyResult::new(
            vec!["sunrise".to_string(), "sunset".to_string()],
            embeddings,
            EmbeddingModelUsage::new(12),
        )
        .with_warning(Warning::Other {
            message: "Provider chunked the request.".to_string(),
        })
        .with_provider_metadata(provider_metadata)
        .with_responses(vec![
            Some(EmbeddingModelResponse::new().with_header("x-request-id", "req_123")),
            None,
        ]);

        assert_eq!(
            serde_json::to_value(result).expect("embed many result serializes"),
            json!({
                "values": ["sunrise", "sunset"],
                "embeddings": [
                    [0.1, 0.2, 0.3],
                    [0.4, 0.5, 0.6]
                ],
                "usage": {
                    "tokens": 12
                },
                "warnings": [
                    {
                        "type": "other",
                        "message": "Provider chunked the request."
                    }
                ],
                "providerMetadata": {
                    "openai": {
                        "dimensions": 3
                    }
                },
                "responses": [
                    {
                        "headers": {
                            "x-request-id": "req_123"
                        }
                    },
                    null
                ]
            })
        );
    }

    #[test]
    fn embed_many_result_deserializes_minimal_upstream_shape_and_omits_options() {
        let result: EmbedManyResult = serde_json::from_value(json!({
            "values": ["sunrise", "sunset"],
            "embeddings": [
                [0.1, 0.2, 0.3],
                [0.4, 0.5, 0.6]
            ],
            "usage": {
                "tokens": 12
            },
            "warnings": []
        }))
        .expect("embed many result deserializes");

        assert_eq!(
            result,
            EmbedManyResult::new(
                vec!["sunrise".to_string(), "sunset".to_string()],
                vec![vec![0.1, 0.2, 0.3], vec![0.4, 0.5, 0.6]],
                EmbeddingModelUsage::new(12)
            )
        );
        assert_eq!(
            serde_json::to_value(result).expect("embed many result serializes"),
            json!({
                "values": ["sunrise", "sunset"],
                "embeddings": [
                    [0.1, 0.2, 0.3],
                    [0.4, 0.5, 0.6]
                ],
                "usage": {
                    "tokens": 12
                },
                "warnings": []
            })
        );
    }
}
