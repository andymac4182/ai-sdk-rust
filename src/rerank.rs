use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::provider::{ProviderMetadata, ProviderOptions};
use crate::provider_utils::{IdGeneratorOptions, create_id_generator};
use crate::reranking_model::{
    RerankingModel, RerankingModelCallOptions, RerankingModelDocuments, RerankingModelRanking,
    RerankingModelResponse,
};
use crate::retry::DEFAULT_MAX_RETRIES;
use crate::telemetry::{TelemetryOptions, create_telemetry_dispatcher};
use crate::warning::Warning;

/// Document accepted by high-level `rerank`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum RerankDocument {
    /// Plain text document.
    Text(String),

    /// JSON object document.
    Object(JsonObject),
}

impl RerankDocument {
    /// Creates a text rerank document.
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text(value.into())
    }

    /// Creates an object rerank document.
    pub fn object(value: JsonObject) -> Self {
        Self::Object(value)
    }
}

impl From<String> for RerankDocument {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for RerankDocument {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

impl From<JsonObject> for RerankDocument {
    fn from(value: JsonObject) -> Self {
        Self::Object(value)
    }
}

/// Homogeneous document batch for a high-level `rerank` call.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RerankDocuments {
    /// Text documents to rerank.
    Text(Vec<String>),

    /// JSON object documents to rerank.
    Object(Vec<JsonObject>),
}

impl RerankDocuments {
    /// Creates text documents.
    pub fn text<T, I>(values: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        Self::Text(values.into_iter().map(Into::into).collect())
    }

    /// Creates JSON object documents.
    pub fn object<I>(values: I) -> Self
    where
        I: IntoIterator<Item = JsonObject>,
    {
        Self::Object(values.into_iter().collect())
    }

    fn len(&self) -> usize {
        match self {
            Self::Text(values) => values.len(),
            Self::Object(values) => values.len(),
        }
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn to_event_documents(&self) -> Vec<RerankDocument> {
        match self {
            Self::Text(values) => values.iter().cloned().map(RerankDocument::Text).collect(),
            Self::Object(values) => values.iter().cloned().map(RerankDocument::Object).collect(),
        }
    }

    fn to_model_documents(&self) -> RerankingModelDocuments {
        match self {
            Self::Text(values) => RerankingModelDocuments::text(values.clone()),
            Self::Object(values) => RerankingModelDocuments::object(values.clone()),
        }
    }

    fn document_at(&self, index: usize) -> RerankDocument {
        match self {
            Self::Text(values) => values
                .get(index)
                .cloned()
                .map(RerankDocument::Text)
                .expect("reranking model returned an out-of-range document index"),
            Self::Object(values) => values
                .get(index)
                .cloned()
                .map(RerankDocument::Object)
                .expect("reranking model returned an out-of-range document index"),
        }
    }
}

/// High-level ranking entry returned by `rerank`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankRanking {
    /// Index of the document in the original input list.
    pub original_index: usize,

    /// Relevance score assigned by the reranking model.
    pub score: f64,

    /// Document at the original index.
    pub document: RerankDocument,
}

impl RerankRanking {
    /// Creates a high-level ranking entry.
    pub fn new(original_index: usize, score: f64, document: RerankDocument) -> Self {
        Self {
            original_index,
            score,
            document,
        }
    }
}

/// High-level response metadata returned by `rerank`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankResponse {
    /// Provider response identifier, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Timestamp for the provider response.
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,

    /// Provider model identifier used for the response.
    pub model_id: String,

    /// Response headers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Provider response body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonValue>,
}

impl RerankResponse {
    /// Creates response metadata with required high-level defaults.
    pub fn new(timestamp: OffsetDateTime, model_id: impl Into<String>) -> Self {
        Self {
            id: None,
            timestamp,
            model_id: model_id.into(),
            headers: None,
            body: None,
        }
    }

    /// Sets the provider response identifier.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
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

/// Result of a high-level `rerank` call.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankResult {
    /// Original documents passed to the call.
    pub original_documents: Vec<RerankDocument>,

    /// Reranked documents sorted by descending relevance.
    pub reranked_documents: Vec<RerankDocument>,

    /// Ranking entries sorted by descending relevance.
    pub ranking: Vec<RerankRanking>,

    /// Optional provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Provider response metadata.
    pub response: RerankResponse,
}

impl RerankResult {
    /// Creates a rerank result.
    pub fn new(
        original_documents: Vec<RerankDocument>,
        ranking: Vec<RerankRanking>,
        response: RerankResponse,
    ) -> Self {
        let reranked_documents = ranking
            .iter()
            .map(|ranking| ranking.document.clone())
            .collect();

        Self {
            original_documents,
            reranked_documents,
            ranking,
            provider_metadata: None,
            response,
        }
    }

    /// Adds provider-specific metadata.
    pub fn with_provider_metadata(mut self, provider_metadata: ProviderMetadata) -> Self {
        self.provider_metadata = Some(provider_metadata);
        self
    }
}

/// Event passed to the start callback for `rerank`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankStartEvent {
    /// Unique identifier for this high-level rerank call.
    pub call_id: String,

    /// Upstream operation identifier.
    pub operation_id: String,

    /// Provider identifier.
    pub provider: String,

    /// Provider-specific model identifier.
    pub model_id: String,

    /// Documents being reranked.
    pub documents: Vec<RerankDocument>,

    /// Query to rerank documents against.
    pub query: String,

    /// Optional top-N limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_n: Option<u64>,

    /// Maximum number of retries configured for failed requests.
    pub max_retries: usize,

    /// Additional HTTP headers sent to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Additional provider-specific options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

/// Event passed to the end callback for `rerank`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankEndEvent {
    /// Unique identifier for this high-level rerank call.
    pub call_id: String,

    /// Upstream operation identifier.
    pub operation_id: String,

    /// Provider identifier.
    pub provider: String,

    /// Provider-specific model identifier.
    pub model_id: String,

    /// Documents that were reranked.
    pub documents: Vec<RerankDocument>,

    /// Query used to rerank the documents.
    pub query: String,

    /// Reranked results sorted by descending relevance.
    pub ranking: Vec<RerankRanking>,

    /// Warnings returned by the model.
    pub warnings: Vec<Warning>,

    /// Optional provider-specific metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<ProviderMetadata>,

    /// Provider response metadata.
    pub response: RerankResponse,
}

/// Event fired when an individual reranking model call starts.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankingModelCallStartEvent {
    /// Unique identifier for this high-level rerank call.
    pub call_id: String,

    /// Upstream inner operation identifier.
    pub operation_id: String,

    /// Provider identifier.
    pub provider: String,

    /// Provider-specific model identifier.
    pub model_id: String,

    /// Documents being reranked.
    pub documents: Vec<RerankDocument>,

    /// Document input type, either `text` or `object`.
    pub documents_type: String,

    /// Query to rerank documents against.
    pub query: String,

    /// Optional top-N limit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_n: Option<u64>,
}

/// Event fired when an individual reranking model call ends.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RerankingModelCallEndEvent {
    /// Unique identifier for this high-level rerank call.
    pub call_id: String,

    /// Upstream inner operation identifier.
    pub operation_id: String,

    /// Provider identifier.
    pub provider: String,

    /// Provider-specific model identifier.
    pub model_id: String,

    /// Document input type, either `text` or `object`.
    pub documents_type: String,

    /// Provider-v4 ranking results from the model.
    pub ranking: Vec<RerankingModelRanking>,
}

/// Future returned by a high-level rerank start callback.
pub type RerankOnStartFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked before a high-level rerank operation calls the model.
pub type RerankOnStartFunction<'a> = dyn Fn(RerankStartEvent) -> RerankOnStartFuture<'a> + 'a;

/// Upstream callback alias for [`RerankOnStartFunction`].
pub type RerankOnStartCallback<'a> = RerankOnStartFunction<'a>;

/// Callback wrapper for upstream rerank `experimental_onStart`.
pub struct RerankOnStart<'a> {
    on_start: Rc<RerankOnStartFunction<'a>>,
}

impl<'a> RerankOnStart<'a> {
    /// Creates a rerank start callback.
    pub fn new<F, Fut>(on_start: F) -> Self
    where
        F: Fn(RerankStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_start: Rc::new(move |event| Box::pin(on_start(event))),
        }
    }

    /// Runs the rerank start callback.
    pub fn start(&self, event: RerankStartEvent) -> RerankOnStartFuture<'a> {
        (self.on_start)(event)
    }
}

impl fmt::Debug for RerankOnStart<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RerankOnStart")
            .finish_non_exhaustive()
    }
}

/// Future returned by a high-level rerank end callback.
pub type RerankOnEndFuture<'a> = Pin<Box<dyn Future<Output = ()> + 'a>>;

/// Callback invoked after a high-level rerank operation receives model output.
pub type RerankOnEndFunction<'a> = dyn Fn(RerankEndEvent) -> RerankOnEndFuture<'a> + 'a;

/// Upstream callback alias for [`RerankOnEndFunction`].
pub type RerankOnEndCallback<'a> = RerankOnEndFunction<'a>;

/// Callback wrapper for upstream rerank `experimental_onEnd`.
pub struct RerankOnEnd<'a> {
    on_end: Rc<RerankOnEndFunction<'a>>,
}

impl<'a> RerankOnEnd<'a> {
    /// Creates a rerank end callback.
    pub fn new<F, Fut>(on_end: F) -> Self
    where
        F: Fn(RerankEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self {
            on_end: Rc::new(move |event| Box::pin(on_end(event))),
        }
    }

    /// Runs the rerank end callback.
    pub fn end(&self, event: RerankEndEvent) -> RerankOnEndFuture<'a> {
        (self.on_end)(event)
    }
}

impl fmt::Debug for RerankOnEnd<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RerankOnEnd")
            .finish_non_exhaustive()
    }
}

/// Options for a high-level `rerank` call.
pub struct RerankOptions<'a, M: RerankingModel + ?Sized> {
    /// Reranking model used for the call.
    pub model: &'a M,

    /// Documents to rerank.
    pub documents: RerankDocuments,

    /// Query to rerank the documents against.
    pub query: String,

    /// Optional top-N limit.
    pub top_n: Option<u64>,

    /// Maximum number of retries per reranking model call.
    pub max_retries: Option<usize>,

    /// Provider-specific options passed through to the model.
    pub provider_options: Option<ProviderOptions>,

    /// Additional HTTP headers for HTTP-based providers.
    pub headers: Option<Headers>,

    /// Callback invoked before reranking begins.
    pub on_start: Option<RerankOnStart<'a>>,

    /// Callback invoked after reranking completes.
    pub on_end: Option<RerankOnEnd<'a>>,

    /// Optional telemetry dispatcher settings.
    pub telemetry: Option<TelemetryOptions>,
}

impl<'a, M: RerankingModel + ?Sized> RerankOptions<'a, M> {
    /// Creates options for a high-level `rerank` call.
    pub fn new(model: &'a M, documents: RerankDocuments, query: impl Into<String>) -> Self {
        Self {
            model,
            documents,
            query: query.into(),
            top_n: None,
            max_retries: None,
            provider_options: None,
            headers: None,
            on_start: None,
            on_end: None,
            telemetry: None,
        }
    }

    /// Sets the maximum number of returned ranked documents.
    pub fn with_top_n(mut self, top_n: u64) -> Self {
        self.top_n = Some(top_n);
        self
    }

    /// Sets the maximum number of retries used in lifecycle metadata.
    pub fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = Some(max_retries);
        self
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

    /// Sets a callback that is invoked before reranking begins.
    pub fn with_on_start<F, Fut>(mut self, on_start: F) -> Self
    where
        F: Fn(RerankStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_start = Some(RerankOnStart::new(on_start));
        self
    }

    /// Upstream experimental alias for [`RerankOptions::with_on_start`].
    pub fn with_experimental_on_start<F, Fut>(self, on_start: F) -> Self
    where
        F: Fn(RerankStartEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.with_on_start(on_start)
    }

    /// Sets a callback that is invoked after reranking completes.
    pub fn with_on_end<F, Fut>(mut self, on_end: F) -> Self
    where
        F: Fn(RerankEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.on_end = Some(RerankOnEnd::new(on_end));
        self
    }

    /// Upstream experimental alias for [`RerankOptions::with_on_end`].
    pub fn with_experimental_on_end<F, Fut>(self, on_end: F) -> Self
    where
        F: Fn(RerankEndEvent) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        self.with_on_end(on_end)
    }

    /// Sets telemetry options for this rerank operation.
    pub fn with_telemetry(mut self, telemetry: TelemetryOptions) -> Self {
        self.telemetry = Some(telemetry);
        self
    }
}

/// Reranks documents using a reranking model.
pub async fn rerank<M: RerankingModel + ?Sized>(options: RerankOptions<'_, M>) -> RerankResult {
    let RerankOptions {
        model,
        documents,
        query,
        top_n,
        max_retries,
        provider_options,
        headers,
        on_start,
        on_end,
        telemetry,
    } = options;

    let call_id = rerank_call_id();
    let max_retries = max_retries.unwrap_or(DEFAULT_MAX_RETRIES);
    let start_documents = documents.to_event_documents();
    let telemetry_dispatcher = create_telemetry_dispatcher(telemetry);

    if on_start.is_some() || telemetry_dispatcher.is_enabled() {
        let start_event = RerankStartEvent {
            call_id: call_id.clone(),
            operation_id: "ai.rerank".to_string(),
            provider: model.provider().to_string(),
            model_id: model.model_id().to_string(),
            documents: start_documents.clone(),
            query: query.clone(),
            top_n,
            max_retries,
            headers: headers.clone(),
            provider_options: provider_options.clone(),
        };
        if let Some(on_start) = &on_start {
            on_start.start(start_event.clone()).await;
        }
        telemetry_dispatcher.on_rerank_start(&start_event);
    }

    if documents.is_empty() {
        let response = RerankResponse::new(OffsetDateTime::now_utc(), model.model_id());
        let result = RerankResult::new(start_documents, Vec::new(), response.clone());

        if on_end.is_some() || telemetry_dispatcher.is_enabled() {
            let end_event = RerankEndEvent {
                call_id,
                operation_id: "ai.rerank".to_string(),
                provider: model.provider().to_string(),
                model_id: model.model_id().to_string(),
                documents: result.original_documents.clone(),
                query,
                ranking: Vec::new(),
                warnings: Vec::new(),
                provider_metadata: None,
                response,
            };
            if let Some(on_end) = &on_end {
                on_end.end(end_event.clone()).await;
            }
            telemetry_dispatcher.on_rerank_end(&end_event);
        }

        return result;
    }

    let model_result = model
        .do_rerank(RerankingModelCallOptions {
            documents: documents.to_model_documents(),
            query: query.clone(),
            top_n,
            abort_signal: None,
            provider_options: provider_options.clone(),
            headers: headers.clone(),
        })
        .await;

    let ranking = model_result
        .ranking
        .iter()
        .map(|ranking| {
            RerankRanking::new(
                ranking.index,
                ranking.relevance_score,
                documents.document_at(ranking.index),
            )
        })
        .collect::<Vec<_>>();
    let response = response_with_defaults(model_result.response, model.model_id());
    let mut result = RerankResult::new(
        documents.to_event_documents(),
        ranking.clone(),
        response.clone(),
    );

    if let Some(provider_metadata) = model_result.provider_metadata.clone() {
        result = result.with_provider_metadata(provider_metadata);
    }

    if on_end.is_some() || telemetry_dispatcher.is_enabled() {
        let end_event = RerankEndEvent {
            call_id,
            operation_id: "ai.rerank".to_string(),
            provider: model.provider().to_string(),
            model_id: model.model_id().to_string(),
            documents: result.original_documents.clone(),
            query,
            ranking,
            warnings: model_result.warnings,
            provider_metadata: model_result.provider_metadata,
            response,
        };
        if let Some(on_end) = &on_end {
            on_end.end(end_event.clone()).await;
        }
        telemetry_dispatcher.on_rerank_end(&end_event);
    }

    result
}

fn response_with_defaults(
    response: Option<RerankingModelResponse>,
    model_id: &str,
) -> RerankResponse {
    match response {
        Some(response) => RerankResponse {
            id: response.id,
            timestamp: response.timestamp.unwrap_or_else(OffsetDateTime::now_utc),
            model_id: response.model_id.unwrap_or_else(|| model_id.to_string()),
            headers: response.headers,
            body: response.body,
        },
        None => RerankResponse::new(OffsetDateTime::now_utc(), model_id),
    }
}

fn rerank_call_id() -> String {
    let generate_call_id =
        create_id_generator(IdGeneratorOptions::new().with_prefix("call").with_size(24))
            .expect("default rerank call id configuration is valid");

    generate_call_id()
}

#[cfg(test)]
mod tests {
    use super::{
        RerankDocument, RerankDocuments, RerankEndEvent, RerankOptions, RerankRanking,
        RerankResponse, RerankResult, RerankStartEvent, RerankingModelCallEndEvent,
        RerankingModelCallStartEvent,
    };
    use crate::headers::Headers;
    use crate::json::JsonObject;
    use crate::provider::{ProviderMetadata, ProviderOptions, SpecificationVersion};
    use crate::reranking_model::{
        RerankingModel, RerankingModelCallOptions, RerankingModelRanking, RerankingModelResponse,
        RerankingModelResult,
    };
    use crate::retry::DEFAULT_MAX_RETRIES;
    use crate::telemetry::{
        TelemetryEvent, TelemetryEventKind, TelemetryIntegration, TelemetryOptions,
    };
    use crate::warning::Warning;
    use serde_json::json;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::future::{Future, Ready, ready};
    use std::pin::Pin;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};
    use time::OffsetDateTime;

    struct RecordingRerankingModel {
        calls: Mutex<Vec<RerankingModelCallOptions>>,
        results: Mutex<VecDeque<RerankingModelResult>>,
    }

    impl RecordingRerankingModel {
        fn new(results: Vec<RerankingModelResult>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                results: Mutex::new(results.into()),
            }
        }

        fn calls(&self) -> Vec<RerankingModelCallOptions> {
            self.calls
                .lock()
                .expect("calls lock is not poisoned")
                .clone()
        }
    }

    impl RerankingModel for RecordingRerankingModel {
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

        fn do_rerank(&self, options: RerankingModelCallOptions) -> Self::RerankFuture<'_> {
            self.calls
                .lock()
                .expect("calls lock is not poisoned")
                .push(options.clone());
            let result = self
                .results
                .lock()
                .expect("results lock is not poisoned")
                .pop_front()
                .unwrap_or_else(|| RerankingModelResult::new(Vec::new()));

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

    fn object(value: serde_json::Value) -> JsonObject {
        serde_json::from_value(value).expect("object deserializes")
    }

    fn timestamp() -> OffsetDateTime {
        OffsetDateTime::parse(
            "2025-01-01T00:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("timestamp parses")
    }

    #[test]
    fn rerank_calls_model_with_text_documents_and_maps_result() {
        let provider_options: ProviderOptions = serde_json::from_value(json!({
            "cohere": {
                "returnDocuments": true
            }
        }))
        .expect("provider options deserialize");
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "cohere": {
                "searchUnits": 1
            }
        }))
        .expect("provider metadata deserialize");
        let response = RerankingModelResponse::new()
            .with_id("rerank-response-id")
            .with_timestamp(timestamp())
            .with_model_id("provider-rerank-model")
            .with_header("content-type", "application/json")
            .with_body(json!({ "id": "raw-rerank-response" }));
        let model = RecordingRerankingModel::new(vec![
            RerankingModelResult::new(vec![
                RerankingModelRanking::new(2, 0.9),
                RerankingModelRanking::new(0, 0.8),
                RerankingModelRanking::new(1, 0.7),
            ])
            .with_provider_metadata(provider_metadata.clone())
            .with_warning(Warning::Other {
                message: "test warning".to_string(),
            })
            .with_response(response.clone()),
        ]);

        let result = poll_ready(super::rerank(
            RerankOptions::new(
                &model,
                RerankDocuments::text([
                    "sunny day at the beach",
                    "rainy day in the city",
                    "cloudy day in the mountains",
                ]),
                "rainy day",
            )
            .with_top_n(3)
            .with_provider_options(provider_options.clone())
            .with_header("x-custom", "header-value"),
        ));

        assert_eq!(
            result.original_documents,
            vec![
                RerankDocument::text("sunny day at the beach"),
                RerankDocument::text("rainy day in the city"),
                RerankDocument::text("cloudy day in the mountains"),
            ]
        );
        assert_eq!(
            result.reranked_documents,
            vec![
                RerankDocument::text("cloudy day in the mountains"),
                RerankDocument::text("sunny day at the beach"),
                RerankDocument::text("rainy day in the city"),
            ]
        );
        assert_eq!(
            result.ranking,
            vec![
                RerankRanking::new(2, 0.9, RerankDocument::text("cloudy day in the mountains")),
                RerankRanking::new(0, 0.8, RerankDocument::text("sunny day at the beach")),
                RerankRanking::new(1, 0.7, RerankDocument::text("rainy day in the city")),
            ]
        );
        assert_eq!(result.provider_metadata, Some(provider_metadata));
        assert_eq!(
            result.response,
            RerankResponse {
                id: Some("rerank-response-id".to_string()),
                timestamp: timestamp(),
                model_id: "provider-rerank-model".to_string(),
                headers: Some(Headers::from([(
                    "content-type".to_string(),
                    "application/json".to_string()
                )])),
                body: Some(json!({ "id": "raw-rerank-response" })),
            }
        );

        let calls = model.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].query, "rainy day");
        assert_eq!(calls[0].top_n, Some(3));
        assert_eq!(calls[0].provider_options, Some(provider_options));
        assert_eq!(
            calls[0].headers,
            Some(Headers::from([(
                "x-custom".to_string(),
                "header-value".to_string()
            )]))
        );
        assert_eq!(
            calls[0].documents,
            crate::reranking_model::RerankingModelDocuments::text(vec![
                "sunny day at the beach".to_string(),
                "rainy day in the city".to_string(),
                "cloudy day in the mountains".to_string(),
            ])
        );
    }

    #[test]
    fn rerank_calls_model_with_object_documents() {
        let first = object(json!({ "id": "123", "name": "sunny day at the beach" }));
        let second = object(json!({ "id": "456", "name": "rainy day in the city" }));
        let third = object(json!({ "id": "789", "name": "cloudy day in the mountains" }));
        let model = RecordingRerankingModel::new(vec![RerankingModelResult::new(vec![
            RerankingModelRanking::new(2, 0.9),
            RerankingModelRanking::new(0, 0.8),
        ])]);

        let result = poll_ready(super::rerank(RerankOptions::new(
            &model,
            RerankDocuments::object([first.clone(), second.clone(), third.clone()]),
            "rainy day",
        )));

        assert_eq!(
            result.reranked_documents,
            vec![
                RerankDocument::object(third.clone()),
                RerankDocument::object(first.clone()),
            ]
        );

        let calls = model.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].documents,
            crate::reranking_model::RerankingModelDocuments::object(vec![first, second, third])
        );
    }

    #[test]
    fn rerank_skips_model_call_for_empty_documents_and_fires_callbacks() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let start_events = Rc::clone(&events);
        let end_events = Rc::clone(&events);
        let model = RecordingRerankingModel::new(Vec::new());

        let result = poll_ready(super::rerank(
            RerankOptions::new(
                &model,
                RerankDocuments::text(Vec::<String>::new()),
                "rainy day",
            )
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

        assert!(model.calls().is_empty());
        assert!(result.original_documents.is_empty());
        assert!(result.ranking.is_empty());
        assert_eq!(result.response.model_id, "rerank-test");

        let events = events.borrow();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["operationId"], "ai.rerank");
        assert_eq!(events[0]["documents"], json!([]));
        assert_eq!(events[0]["maxRetries"], json!(DEFAULT_MAX_RETRIES));
        assert_eq!(events[1]["operationId"], "ai.rerank");
        assert_eq!(events[1]["documents"], json!([]));
        assert_eq!(events[1]["ranking"], json!([]));
        assert_eq!(events[1]["response"]["modelId"], "rerank-test");
    }

    #[test]
    fn rerank_invokes_start_and_end_callbacks_with_upstream_events() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let start_events = Rc::clone(&events);
        let end_events = Rc::clone(&events);
        let provider_metadata: ProviderMetadata = serde_json::from_value(json!({
            "cohere": {
                "searchUnits": 1
            }
        }))
        .expect("provider metadata deserialize");
        let model = RecordingRerankingModel::new(vec![
            RerankingModelResult::new(vec![RerankingModelRanking::new(1, 0.95)])
                .with_provider_metadata(provider_metadata)
                .with_warning(Warning::Other {
                    message: "test warning".to_string(),
                })
                .with_response(
                    RerankingModelResponse::new()
                        .with_timestamp(timestamp())
                        .with_model_id("provider-rerank-model")
                        .with_header("x-request-id", "rerank-callback"),
                ),
        ]);

        let result = poll_ready(super::rerank(
            RerankOptions::new(
                &model,
                RerankDocuments::text(["sunny day", "rainy day"]),
                "rainy day",
            )
            .with_top_n(1)
            .with_max_retries(4)
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

        assert_eq!(
            result.reranked_documents,
            vec![RerankDocument::text("rainy day")]
        );

        let events = events.borrow();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["operationId"], "ai.rerank");
        assert_eq!(events[0]["provider"], "test-provider");
        assert_eq!(events[0]["modelId"], "rerank-test");
        assert_eq!(events[0]["documents"], json!(["sunny day", "rainy day"]));
        assert_eq!(events[0]["query"], "rainy day");
        assert_eq!(events[0]["topN"], json!(1));
        assert_eq!(events[0]["maxRetries"], json!(4));
        assert!(
            events[0]["callId"]
                .as_str()
                .expect("call id is a string")
                .starts_with("call-")
        );

        assert_eq!(events[1]["operationId"], "ai.rerank");
        assert_eq!(
            events[1]["ranking"],
            json!([
                {
                    "originalIndex": 1,
                    "score": 0.95,
                    "document": "rainy day"
                }
            ])
        );
        assert_eq!(
            events[1]["warnings"],
            json!([
                {
                    "type": "other",
                    "message": "test warning"
                }
            ])
        );
        assert_eq!(
            events[1]["providerMetadata"],
            json!({
                "cohere": {
                    "searchUnits": 1
                }
            })
        );
        assert_eq!(
            events[1]["response"],
            json!({
                "timestamp": "2025-01-01T00:00:00Z",
                "modelId": "provider-rerank-model",
                "headers": {
                    "x-request-id": "rerank-callback"
                }
            })
        );
    }

    #[test]
    fn rerank_dispatches_telemetry_lifecycle_events() {
        let model = RecordingRerankingModel::new(vec![RerankingModelResult::new(vec![
            RerankingModelRanking::new(1, 0.95),
        ])]);
        let events = Arc::new(Mutex::new(Vec::<TelemetryEvent>::new()));
        let start_events = Arc::clone(&events);
        let end_events = Arc::clone(&events);
        let integration = TelemetryIntegration::new()
            .with_callback(TelemetryEventKind::OnRerankStart, move |event| {
                start_events
                    .lock()
                    .expect("telemetry event lock")
                    .push(event);
            })
            .with_callback(TelemetryEventKind::OnRerankEnd, move |event| {
                end_events.lock().expect("telemetry event lock").push(event);
            });

        let result = poll_ready(super::rerank(
            RerankOptions::new(
                &model,
                RerankDocuments::text(["sunny day", "rainy day"]),
                "rainy day",
            )
            .with_top_n(1)
            .with_telemetry(
                TelemetryOptions::new()
                    .with_function_id("rerank-test")
                    .with_record_inputs(false)
                    .with_record_outputs(true)
                    .with_integration(integration),
            ),
        ));

        assert_eq!(
            result.reranked_documents,
            vec![RerankDocument::text("rainy day")]
        );
        let events = events.lock().expect("telemetry event lock");
        assert_eq!(
            events.iter().map(|event| event.kind).collect::<Vec<_>>(),
            vec![
                TelemetryEventKind::OnRerankStart,
                TelemetryEventKind::OnRerankEnd,
            ]
        );
        assert!(
            events
                .iter()
                .all(|event| event.function_id.as_deref() == Some("rerank-test"))
        );
        assert!(
            events
                .iter()
                .all(|event| event.record_inputs == Some(false))
        );
        assert!(
            events
                .iter()
                .all(|event| event.record_outputs == Some(true))
        );
        assert_eq!(events[0].event["operationId"], json!("ai.rerank"));
        assert_eq!(
            events[0].event["documents"],
            json!(["sunny day", "rainy day"])
        );
        assert_eq!(events[0].event["query"], json!("rainy day"));
        assert_eq!(
            events[1].event["ranking"][0]["document"],
            json!("rainy day")
        );
        assert_eq!(events[1].event["ranking"][0]["score"], json!(0.95));
    }

    #[test]
    fn rerank_contracts_serialize_upstream_shapes() {
        let response = RerankResponse::new(timestamp(), "rerank-model")
            .with_id("rerank-response-id")
            .with_header("content-type", "application/json")
            .with_body(json!({ "id": "raw-rerank-response" }));
        let result = RerankResult::new(
            vec![
                RerankDocument::text("sunny day"),
                RerankDocument::text("rainy day"),
            ],
            vec![RerankRanking::new(
                1,
                0.95,
                RerankDocument::text("rainy day"),
            )],
            response,
        )
        .with_provider_metadata(
            serde_json::from_value(json!({
                "cohere": {
                    "searchUnits": 1
                }
            }))
            .expect("provider metadata deserialize"),
        );

        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "originalDocuments": ["sunny day", "rainy day"],
                "rerankedDocuments": ["rainy day"],
                "ranking": [
                    {
                        "originalIndex": 1,
                        "score": 0.95,
                        "document": "rainy day"
                    }
                ],
                "providerMetadata": {
                    "cohere": {
                        "searchUnits": 1
                    }
                },
                "response": {
                    "id": "rerank-response-id",
                    "timestamp": "2025-01-01T00:00:00Z",
                    "modelId": "rerank-model",
                    "headers": {
                        "content-type": "application/json"
                    },
                    "body": {
                        "id": "raw-rerank-response"
                    }
                }
            })
        );
    }

    #[test]
    fn rerank_result_deserializes_minimal_upstream_shape() {
        let result: RerankResult = serde_json::from_value(json!({
            "originalDocuments": [
                {
                    "id": "123",
                    "name": "sunny day"
                },
                {
                    "id": "456",
                    "name": "rainy day"
                }
            ],
            "rerankedDocuments": [
                {
                    "id": "456",
                    "name": "rainy day"
                }
            ],
            "ranking": [
                {
                    "originalIndex": 1,
                    "score": 0.95,
                    "document": {
                        "id": "456",
                        "name": "rainy day"
                    }
                }
            ],
            "response": {
                "timestamp": "2025-01-01T00:00:00Z",
                "modelId": "rerank-model"
            }
        }))
        .expect("result deserializes");

        assert_eq!(result.original_documents.len(), 2);
        assert_eq!(
            result.reranked_documents,
            vec![RerankDocument::object(object(json!({
                "id": "456",
                "name": "rainy day"
            })))]
        );
        assert_eq!(result.provider_metadata, None);
        assert_eq!(
            serde_json::to_value(result).expect("result serializes"),
            json!({
                "originalDocuments": [
                    {
                        "id": "123",
                        "name": "sunny day"
                    },
                    {
                        "id": "456",
                        "name": "rainy day"
                    }
                ],
                "rerankedDocuments": [
                    {
                        "id": "456",
                        "name": "rainy day"
                    }
                ],
                "ranking": [
                    {
                        "originalIndex": 1,
                        "score": 0.95,
                        "document": {
                            "id": "456",
                            "name": "rainy day"
                        }
                    }
                ],
                "response": {
                    "timestamp": "2025-01-01T00:00:00Z",
                    "modelId": "rerank-model"
                }
            })
        );
    }

    #[test]
    fn rerank_events_round_trip_upstream_shapes() {
        let start = RerankStartEvent {
            call_id: "call_123".to_string(),
            operation_id: "ai.rerank".to_string(),
            provider: "cohere".to_string(),
            model_id: "rerank-v3.5".to_string(),
            documents: vec![
                RerankDocument::text("sunny day"),
                RerankDocument::text("rainy day"),
            ],
            query: "rainy day".to_string(),
            top_n: Some(1),
            max_retries: DEFAULT_MAX_RETRIES,
            headers: Some(Headers::from([(
                "x-custom".to_string(),
                "header-value".to_string(),
            )])),
            provider_options: Some(
                serde_json::from_value(json!({
                    "cohere": {
                        "returnDocuments": true
                    }
                }))
                .expect("provider options deserialize"),
            ),
        };

        let serialized = serde_json::to_value(&start).expect("start event serializes");
        assert_eq!(
            serialized,
            json!({
                "callId": "call_123",
                "operationId": "ai.rerank",
                "provider": "cohere",
                "modelId": "rerank-v3.5",
                "documents": ["sunny day", "rainy day"],
                "query": "rainy day",
                "topN": 1,
                "maxRetries": 2,
                "headers": {
                    "x-custom": "header-value"
                },
                "providerOptions": {
                    "cohere": {
                        "returnDocuments": true
                    }
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<RerankStartEvent>(serialized)
                .expect("start event deserializes"),
            start
        );

        let end = RerankEndEvent {
            call_id: "call_123".to_string(),
            operation_id: "ai.rerank".to_string(),
            provider: "cohere".to_string(),
            model_id: "rerank-v3.5".to_string(),
            documents: vec![
                RerankDocument::text("sunny day"),
                RerankDocument::text("rainy day"),
            ],
            query: "rainy day".to_string(),
            ranking: vec![RerankRanking::new(
                1,
                0.95,
                RerankDocument::text("rainy day"),
            )],
            warnings: vec![Warning::Other {
                message: "test warning".to_string(),
            }],
            provider_metadata: None,
            response: RerankResponse::new(timestamp(), "rerank-v3.5"),
        };

        let serialized = serde_json::to_value(&end).expect("end event serializes");
        assert_eq!(
            serialized,
            json!({
                "callId": "call_123",
                "operationId": "ai.rerank",
                "provider": "cohere",
                "modelId": "rerank-v3.5",
                "documents": ["sunny day", "rainy day"],
                "query": "rainy day",
                "ranking": [
                    {
                        "originalIndex": 1,
                        "score": 0.95,
                        "document": "rainy day"
                    }
                ],
                "warnings": [
                    {
                        "type": "other",
                        "message": "test warning"
                    }
                ],
                "response": {
                    "timestamp": "2025-01-01T00:00:00Z",
                    "modelId": "rerank-v3.5"
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<RerankEndEvent>(serialized).expect("end event deserializes"),
            end
        );
    }

    #[test]
    fn reranking_model_call_events_round_trip_upstream_shapes() {
        let start = RerankingModelCallStartEvent {
            call_id: "call_123".to_string(),
            operation_id: "ai.rerank.doRerank".to_string(),
            provider: "cohere".to_string(),
            model_id: "rerank-v3.5".to_string(),
            documents: vec![RerankDocument::text("sunny day")],
            documents_type: "text".to_string(),
            query: "rainy day".to_string(),
            top_n: Some(1),
        };

        let serialized = serde_json::to_value(&start).expect("start event serializes");
        assert_eq!(
            serialized,
            json!({
                "callId": "call_123",
                "operationId": "ai.rerank.doRerank",
                "provider": "cohere",
                "modelId": "rerank-v3.5",
                "documents": ["sunny day"],
                "documentsType": "text",
                "query": "rainy day",
                "topN": 1
            })
        );
        assert_eq!(
            serde_json::from_value::<RerankingModelCallStartEvent>(serialized)
                .expect("start event deserializes"),
            start
        );

        let end = RerankingModelCallEndEvent {
            call_id: "call_123".to_string(),
            operation_id: "ai.rerank.doRerank".to_string(),
            provider: "cohere".to_string(),
            model_id: "rerank-v3.5".to_string(),
            documents_type: "text".to_string(),
            ranking: vec![RerankingModelRanking::new(0, 0.95)],
        };

        let serialized = serde_json::to_value(&end).expect("end event serializes");
        assert_eq!(
            serialized,
            json!({
                "callId": "call_123",
                "operationId": "ai.rerank.doRerank",
                "provider": "cohere",
                "modelId": "rerank-v3.5",
                "documentsType": "text",
                "ranking": [
                    {
                        "index": 0,
                        "relevanceScore": 0.95
                    }
                ]
            })
        );
        assert_eq!(
            serde_json::from_value::<RerankingModelCallEndEvent>(serialized)
                .expect("end event deserializes"),
            end
        );
    }

    #[test]
    fn reranking_trait_identity_remains_v4() {
        let model = RecordingRerankingModel::new(Vec::new());

        assert_eq!(
            model.specification_version(),
            SpecificationVersion::V4,
            "high-level rerank still calls the provider-v4 model trait"
        );
    }
}
