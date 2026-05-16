use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::VERSION;
use crate::embedding_model::{
    EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelEmbedding, EmbeddingModelResponse,
    EmbeddingModelResult, EmbeddingModelUsage,
};
use crate::headers::Headers;
use crate::provider::ProviderMetadata;
use crate::provider::ProviderOptions;
use crate::provider_utils::{IdGeneratorOptions, create_id_generator, with_user_agent_suffix};
use crate::retry::DEFAULT_MAX_RETRIES;
use crate::warning::Warning;

/// Embedding vector returned by high-level embed operations.
pub type Embedding = EmbeddingModelEmbedding;

/// Value payload used by high-level embed lifecycle events.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum EmbedEventValue {
    /// One value for `embed`.
    One(String),

    /// Multiple values for `embedMany`.
    Many(Vec<String>),
}

/// Embedding payload used by high-level embed lifecycle events.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum EmbedEventEmbedding {
    /// One embedding vector for `embed`.
    One(Embedding),

    /// Multiple embedding vectors for `embedMany`.
    Many(Vec<Embedding>),
}

/// Response payload used by high-level embed lifecycle events.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum EmbedEventResponse {
    /// One provider response for `embed`.
    One(EmbeddingModelResponse),

    /// Per-call provider responses for `embedMany`.
    Many(Vec<Option<EmbeddingModelResponse>>),
}

/// Event passed to the start callback for `embed` and `embed_many`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedStartEvent {
    /// Unique identifier for this high-level embed call.
    pub call_id: String,

    /// Upstream operation identifier, such as `ai.embed` or `ai.embedMany`.
    pub operation_id: String,

    /// Provider identifier.
    pub provider: String,

    /// Provider-specific model identifier.
    pub model_id: String,

    /// Value or values being embedded.
    pub value: EmbedEventValue,

    /// Maximum number of retries configured for failed requests.
    pub max_retries: usize,

    /// Additional HTTP headers sent to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Additional provider-specific options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

/// Event passed to the end callback for `embed` and `embed_many`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedEndEvent {
    /// Unique identifier for this high-level embed call.
    pub call_id: String,

    /// Upstream operation identifier, such as `ai.embed` or `ai.embedMany`.
    pub operation_id: String,

    /// Provider identifier.
    pub provider: String,

    /// Provider-specific model identifier.
    pub model_id: String,

    /// Value or values that were embedded.
    pub value: EmbedEventValue,

    /// Embedding or embeddings returned by the model.
    pub embedding: EmbedEventEmbedding,

    /// Token usage for the embedding operation.
    pub usage: EmbeddingModelUsage,

    /// Warnings returned by the model.
    pub warnings: Vec<Warning>,

    /// Optional provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Optional response data from the provider call or calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<EmbedEventResponse>,
}

/// Event fired when an individual embedding model call starts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingModelCallStartEvent {
    /// Unique identifier for the high-level embed call.
    pub call_id: String,

    /// Unique identifier for this individual model invocation.
    pub embed_call_id: String,

    /// Upstream inner operation identifier.
    pub operation_id: String,

    /// Provider identifier.
    pub provider: String,

    /// Provider-specific model identifier.
    pub model_id: String,

    /// Values being embedded in this model call.
    pub values: Vec<String>,
}

/// Event fired when an individual embedding model call ends.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingModelCallEndEvent {
    /// Unique identifier for the high-level embed call.
    pub call_id: String,

    /// Unique identifier for this individual model invocation.
    pub embed_call_id: String,

    /// Upstream inner operation identifier.
    pub operation_id: String,

    /// Provider identifier.
    pub provider: String,

    /// Provider-specific model identifier.
    pub model_id: String,

    /// Values embedded in this model call.
    pub values: Vec<String>,

    /// Embeddings returned by this model call.
    pub embeddings: Vec<Embedding>,

    /// Token usage for this model call.
    pub usage: EmbeddingModelUsage,
}

/// Future returned by a high-level embed start callback.
pub type EmbedOnStartFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked before a high-level embed operation calls the model.
pub type EmbedOnStartFunction<'a> = dyn Fn(EmbedStartEvent) -> EmbedOnStartFuture<'a> + 'a;

/// Upstream callback alias for [`EmbedOnStartFunction`].
pub type EmbedOnStartCallback<'a> = EmbedOnStartFunction<'a>;

/// Callback wrapper for upstream embed `experimental_onStart`.
pub struct EmbedOnStart<'a> {
    on_start: Rc<EmbedOnStartFunction<'a>>,
}

impl<'a> EmbedOnStart<'a> {
    /// Creates an embed start callback.
    pub fn new<F, Fut>(on_start: F) -> Self
    where
        F: Fn(EmbedStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_start: Rc::new(move |event| Box::pin(on_start(event))),
        }
    }

    /// Runs the embed start callback.
    pub fn start(&self, event: EmbedStartEvent) -> EmbedOnStartFuture<'a> {
        (self.on_start)(event)
    }
}

impl fmt::Debug for EmbedOnStart<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EmbedOnStart")
            .finish_non_exhaustive()
    }
}

/// Future returned by a high-level embed end callback.
pub type EmbedOnEndFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked after a high-level embed operation receives model output.
pub type EmbedOnEndFunction<'a> = dyn Fn(EmbedEndEvent) -> EmbedOnEndFuture<'a> + 'a;

/// Upstream callback alias for [`EmbedOnEndFunction`].
pub type EmbedOnEndCallback<'a> = EmbedOnEndFunction<'a>;

/// Callback wrapper for upstream embed `experimental_onEnd`.
pub struct EmbedOnEnd<'a> {
    on_end: Rc<EmbedOnEndFunction<'a>>,
}

impl<'a> EmbedOnEnd<'a> {
    /// Creates an embed end callback.
    pub fn new<F, Fut>(on_end: F) -> Self
    where
        F: Fn(EmbedEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_end: Rc::new(move |event| Box::pin(on_end(event))),
        }
    }

    /// Runs the embed end callback.
    pub fn end(&self, event: EmbedEndEvent) -> EmbedOnEndFuture<'a> {
        (self.on_end)(event)
    }
}

impl fmt::Debug for EmbedOnEnd<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("EmbedOnEnd").finish_non_exhaustive()
    }
}

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

    /// Callback invoked before the model is called.
    pub on_start: Option<EmbedOnStart<'a>>,

    /// Callback invoked after the model returns.
    pub on_end: Option<EmbedOnEnd<'a>>,
}

impl<'a, M: EmbeddingModel + ?Sized> EmbedOptions<'a, M> {
    /// Creates options for a high-level `embed` call.
    pub fn new(model: &'a M, value: impl Into<String>) -> Self {
        Self {
            model,
            value: value.into(),
            provider_options: None,
            headers: None,
            on_start: None,
            on_end: None,
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

    /// Sets a callback that is invoked before the embedding model is called.
    pub fn with_on_start<F, Fut>(mut self, on_start: F) -> Self
    where
        F: Fn(EmbedStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_start = Some(EmbedOnStart::new(on_start));
        self
    }

    /// Upstream experimental alias for [`EmbedOptions::with_on_start`].
    pub fn with_experimental_on_start<F, Fut>(self, on_start: F) -> Self
    where
        F: Fn(EmbedStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.with_on_start(on_start)
    }

    /// Sets a callback that is invoked after the embedding model returns.
    pub fn with_on_end<F, Fut>(mut self, on_end: F) -> Self
    where
        F: Fn(EmbedEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_end = Some(EmbedOnEnd::new(on_end));
        self
    }

    /// Upstream experimental alias for [`EmbedOptions::with_on_end`].
    pub fn with_experimental_on_end<F, Fut>(self, on_end: F) -> Self
    where
        F: Fn(EmbedEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.with_on_end(on_end)
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

    /// Callback invoked before embedding begins.
    pub on_start: Option<EmbedOnStart<'a>>,

    /// Callback invoked after all model calls return.
    pub on_end: Option<EmbedOnEnd<'a>>,
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
            on_start: None,
            on_end: None,
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

    /// Sets a callback that is invoked before embedding begins.
    pub fn with_on_start<F, Fut>(mut self, on_start: F) -> Self
    where
        F: Fn(EmbedStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_start = Some(EmbedOnStart::new(on_start));
        self
    }

    /// Upstream experimental alias for [`EmbedManyOptions::with_on_start`].
    pub fn with_experimental_on_start<F, Fut>(self, on_start: F) -> Self
    where
        F: Fn(EmbedStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.with_on_start(on_start)
    }

    /// Sets a callback that is invoked after all embeddings are available.
    pub fn with_on_end<F, Fut>(mut self, on_end: F) -> Self
    where
        F: Fn(EmbedEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_end = Some(EmbedOnEnd::new(on_end));
        self
    }

    /// Upstream experimental alias for [`EmbedManyOptions::with_on_end`].
    pub fn with_experimental_on_end<F, Fut>(self, on_end: F) -> Self
    where
        F: Fn(EmbedEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.with_on_end(on_end)
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
        on_start,
        on_end,
    } = options;
    let headers = headers_with_ai_user_agent(headers);
    let call_id = embed_call_id();

    if let Some(on_start) = &on_start {
        on_start
            .start(EmbedStartEvent {
                call_id: call_id.clone(),
                operation_id: "ai.embed".to_string(),
                provider: model.provider().to_string(),
                model_id: model.model_id().to_string(),
                value: EmbedEventValue::One(value.clone()),
                max_retries: DEFAULT_MAX_RETRIES,
                headers: Some(headers.clone()),
                provider_options: provider_options.clone(),
            })
            .await;
    }

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

    let result = EmbedResult {
        value,
        embedding: embeddings.into_iter().next().unwrap_or_default(),
        usage: usage.unwrap_or_else(|| EmbeddingModelUsage::new(0)),
        warnings,
        provider_metadata,
        response,
    };

    if let Some(on_end) = &on_end {
        on_end
            .end(EmbedEndEvent {
                call_id,
                operation_id: "ai.embed".to_string(),
                provider: model.provider().to_string(),
                model_id: model.model_id().to_string(),
                value: EmbedEventValue::One(result.value.clone()),
                embedding: EmbedEventEmbedding::One(result.embedding.clone()),
                usage: result.usage.clone(),
                warnings: result.warnings.clone(),
                provider_metadata: result.provider_metadata.clone(),
                response: result.response.clone().map(EmbedEventResponse::One),
            })
            .await;
    }

    result
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
        on_start,
        on_end,
    } = options;
    let headers = headers_with_ai_user_agent(headers);
    let call_id = embed_call_id();

    if let Some(on_start) = &on_start {
        on_start
            .start(EmbedStartEvent {
                call_id: call_id.clone(),
                operation_id: "ai.embedMany".to_string(),
                provider: model.provider().to_string(),
                model_id: model.model_id().to_string(),
                value: EmbedEventValue::Many(values.clone()),
                max_retries: DEFAULT_MAX_RETRIES,
                headers: Some(headers.clone()),
                provider_options: provider_options.clone(),
            })
            .await;
    }

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

        let result = EmbedManyResult {
            values,
            embeddings,
            usage: usage.unwrap_or_else(|| EmbeddingModelUsage::new(0)),
            warnings,
            provider_metadata,
            responses: Some(vec![response]),
        };

        if let Some(on_end) = &on_end {
            on_end
                .end(embed_many_end_event(
                    call_id,
                    model.provider(),
                    model.model_id(),
                    &result,
                ))
                .await;
        }

        return result;
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

    let result = EmbedManyResult {
        values,
        embeddings,
        usage: EmbeddingModelUsage::new(tokens),
        warnings,
        provider_metadata,
        responses: Some(responses),
    };

    if let Some(on_end) = &on_end {
        on_end
            .end(embed_many_end_event(
                call_id,
                model.provider(),
                model.model_id(),
                &result,
            ))
            .await;
    }

    result
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

fn embed_call_id() -> String {
    let generate_call_id =
        create_id_generator(IdGeneratorOptions::new().with_prefix("call").with_size(24))
            .expect("default embed call id configuration is valid");

    generate_call_id()
}

fn embed_many_end_event(
    call_id: String,
    provider: &str,
    model_id: &str,
    result: &EmbedManyResult,
) -> EmbedEndEvent {
    EmbedEndEvent {
        call_id,
        operation_id: "ai.embedMany".to_string(),
        provider: provider.to_string(),
        model_id: model_id.to_string(),
        value: EmbedEventValue::Many(result.values.clone()),
        embedding: EmbedEventEmbedding::Many(result.embeddings.clone()),
        usage: result.usage.clone(),
        warnings: result.warnings.clone(),
        provider_metadata: result.provider_metadata.clone(),
        response: result.responses.clone().map(EmbedEventResponse::Many),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EmbedEndEvent, EmbedEventEmbedding, EmbedEventResponse, EmbedEventValue, EmbedManyOptions,
        EmbedManyResult, EmbedOptions, EmbedResult, EmbedStartEvent, Embedding,
        EmbeddingModelCallEndEvent, EmbeddingModelCallStartEvent,
    };
    use crate::embedding_model::{
        EmbeddingModel, EmbeddingModelCallOptions, EmbeddingModelResponse, EmbeddingModelResult,
        EmbeddingModelUsage,
    };
    use crate::headers::Headers;
    use crate::provider::{ProviderMetadata, ProviderOptions};
    use crate::retry::DEFAULT_MAX_RETRIES;
    use crate::warning::Warning;
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::rc::Rc;
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
    fn embed_lifecycle_events_serialize_upstream_shapes() {
        let start = EmbedStartEvent {
            call_id: "call_123".to_string(),
            operation_id: "ai.embed".to_string(),
            provider: "openai".to_string(),
            model_id: "text-embedding-3-small".to_string(),
            value: EmbedEventValue::One("sunrise".to_string()),
            max_retries: DEFAULT_MAX_RETRIES,
            headers: Some(Headers::from([(
                "user-agent".to_string(),
                "ai-sdk-rust-test".to_string(),
            )])),
            provider_options: Some(
                serde_json::from_value(json!({
                    "openai": {
                        "dimensions": 3
                    }
                }))
                .expect("provider options deserialize"),
            ),
        };

        assert_eq!(
            serde_json::to_value(start).expect("start event serializes"),
            json!({
                "callId": "call_123",
                "operationId": "ai.embed",
                "provider": "openai",
                "modelId": "text-embedding-3-small",
                "value": "sunrise",
                "maxRetries": 2,
                "headers": {
                    "user-agent": "ai-sdk-rust-test"
                },
                "providerOptions": {
                    "openai": {
                        "dimensions": 3
                    }
                }
            })
        );

        let end = EmbedEndEvent {
            call_id: "call_123".to_string(),
            operation_id: "ai.embedMany".to_string(),
            provider: "openai".to_string(),
            model_id: "text-embedding-3-small".to_string(),
            value: EmbedEventValue::Many(vec!["sunrise".to_string(), "sunset".to_string()]),
            embedding: EmbedEventEmbedding::Many(vec![vec![0.1, 0.2], vec![0.3, 0.4]]),
            usage: EmbeddingModelUsage::new(12),
            warnings: vec![Warning::Other {
                message: "chunked".to_string(),
            }],
            provider_metadata: Some(
                serde_json::from_value(json!({
                    "openai": {
                        "dimensions": 2
                    }
                }))
                .expect("provider metadata deserialize"),
            ),
            response: Some(EmbedEventResponse::Many(vec![
                Some(EmbeddingModelResponse::new().with_header("x-request-id", "req_123")),
                None,
            ])),
        };

        assert_eq!(
            serde_json::to_value(end).expect("end event serializes"),
            json!({
                "callId": "call_123",
                "operationId": "ai.embedMany",
                "provider": "openai",
                "modelId": "text-embedding-3-small",
                "value": ["sunrise", "sunset"],
                "embedding": [
                    [0.1, 0.2],
                    [0.3, 0.4]
                ],
                "usage": {
                    "tokens": 12
                },
                "warnings": [
                    {
                        "type": "other",
                        "message": "chunked"
                    }
                ],
                "providerMetadata": {
                    "openai": {
                        "dimensions": 2
                    }
                },
                "response": [
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
    fn embedding_model_call_events_round_trip_upstream_shapes() {
        let start = EmbeddingModelCallStartEvent {
            call_id: "call_123".to_string(),
            embed_call_id: "call_456".to_string(),
            operation_id: "ai.embedMany.doEmbed".to_string(),
            provider: "openai".to_string(),
            model_id: "text-embedding-3-small".to_string(),
            values: vec!["sunrise".to_string(), "sunset".to_string()],
        };

        let serialized = serde_json::to_value(&start).expect("start event serializes");
        assert_eq!(
            serialized,
            json!({
                "callId": "call_123",
                "embedCallId": "call_456",
                "operationId": "ai.embedMany.doEmbed",
                "provider": "openai",
                "modelId": "text-embedding-3-small",
                "values": ["sunrise", "sunset"]
            })
        );
        assert_eq!(
            serde_json::from_value::<EmbeddingModelCallStartEvent>(serialized)
                .expect("start event deserializes"),
            start
        );

        let end = EmbeddingModelCallEndEvent {
            call_id: "call_123".to_string(),
            embed_call_id: "call_456".to_string(),
            operation_id: "ai.embedMany.doEmbed".to_string(),
            provider: "openai".to_string(),
            model_id: "text-embedding-3-small".to_string(),
            values: vec!["sunrise".to_string(), "sunset".to_string()],
            embeddings: vec![vec![0.1, 0.2], vec![0.3, 0.4]],
            usage: EmbeddingModelUsage::new(12),
        };

        let serialized = serde_json::to_value(&end).expect("end event serializes");
        assert_eq!(
            serialized,
            json!({
                "callId": "call_123",
                "embedCallId": "call_456",
                "operationId": "ai.embedMany.doEmbed",
                "provider": "openai",
                "modelId": "text-embedding-3-small",
                "values": ["sunrise", "sunset"],
                "embeddings": [
                    [0.1, 0.2],
                    [0.3, 0.4]
                ],
                "usage": {
                    "tokens": 12
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<EmbeddingModelCallEndEvent>(serialized)
                .expect("end event deserializes"),
            end
        );
    }

    #[test]
    fn embed_invokes_start_and_end_callbacks() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let start_events = Rc::clone(&events);
        let end_events = Rc::clone(&events);
        let response = EmbeddingModelResponse::new().with_header("x-request-id", "embed-callback");
        let model = RecordingEmbeddingModel::new(
            None,
            true,
            vec![
                EmbeddingModelResult::new(vec![vec![0.1, 0.2, 0.3]])
                    .with_usage(EmbeddingModelUsage::new(7))
                    .with_response(response.clone()),
            ],
        );

        let result = poll_ready(super::embed(
            EmbedOptions::new(&model, "sunrise")
                .with_header("User-Agent", "caller/1")
                .with_experimental_on_start(move |event| {
                    start_events
                        .borrow_mut()
                        .push(serde_json::to_value(event).expect("event serializes"));
                    ready(())
                })
                .with_experimental_on_end(move |event| {
                    end_events
                        .borrow_mut()
                        .push(serde_json::to_value(event).expect("event serializes"));
                    ready(())
                }),
        ));

        assert_eq!(result.embedding, vec![0.1, 0.2, 0.3]);

        let events = events.borrow();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["operationId"], "ai.embed");
        assert_eq!(events[0]["provider"], "test-provider");
        assert_eq!(events[0]["modelId"], "embedding-test");
        assert_eq!(events[0]["value"], "sunrise");
        assert_eq!(events[0]["maxRetries"], json!(DEFAULT_MAX_RETRIES));
        assert!(
            events[0]["callId"]
                .as_str()
                .expect("call id is a string")
                .starts_with("call-")
        );
        assert_eq!(
            events[0]["headers"]["user-agent"],
            concat!("caller/1 ai/", env!("CARGO_PKG_VERSION"))
        );

        assert_eq!(events[1]["operationId"], "ai.embed");
        assert_eq!(events[1]["value"], "sunrise");
        assert_eq!(events[1]["embedding"], json!([0.1, 0.2, 0.3]));
        assert_eq!(events[1]["usage"], json!({ "tokens": 7 }));
        assert_eq!(
            events[1]["response"],
            json!({
                "headers": {
                    "x-request-id": "embed-callback"
                }
            })
        );
    }

    #[test]
    fn embed_many_invokes_start_and_end_callbacks_with_array_payloads() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let start_events = Rc::clone(&events);
        let end_events = Rc::clone(&events);
        let response =
            EmbeddingModelResponse::new().with_header("x-request-id", "embed-many-callback");
        let model = RecordingEmbeddingModel::new(
            None,
            true,
            vec![
                EmbeddingModelResult::new(vec![vec![0.1], vec![0.2]])
                    .with_usage(EmbeddingModelUsage::new(11))
                    .with_response(response),
            ],
        );

        let result = poll_ready(super::embed_many(
            EmbedManyOptions::new(&model, ["alpha", "beta"])
                .with_on_start(move |event| {
                    start_events
                        .borrow_mut()
                        .push(serde_json::to_value(event).expect("event serializes"));
                    ready(())
                })
                .with_on_end(move |event| {
                    end_events
                        .borrow_mut()
                        .push(serde_json::to_value(event).expect("event serializes"));
                    ready(())
                }),
        ));

        assert_eq!(result.embeddings, vec![vec![0.1], vec![0.2]]);

        let events = events.borrow();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["operationId"], "ai.embedMany");
        assert_eq!(events[0]["value"], json!(["alpha", "beta"]));
        assert_eq!(events[1]["operationId"], "ai.embedMany");
        assert_eq!(events[1]["value"], json!(["alpha", "beta"]));
        assert_eq!(events[1]["embedding"], json!([[0.1], [0.2]]));
        assert_eq!(
            events[1]["response"],
            json!([
                {
                    "headers": {
                        "x-request-id": "embed-many-callback"
                    }
                }
            ])
        );
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
