use std::collections::BTreeMap;
use std::env::{self, VarError};
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::pin::Pin;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU64, Ordering},
};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, DeserializeOwned},
    ser::SerializeStruct,
};
use url::{Host, Url};

use ai_sdk_provider::file_data::{
    FileData, FileDataContent, NoSuchProviderReferenceError, ProviderReference,
};
use ai_sdk_provider::headers::Headers;
use ai_sdk_provider::image_model::ImageModelFile;
use ai_sdk_provider::json::{JsonObject, JsonSchema, JsonValue};
use ai_sdk_provider::language_model::{
    LanguageModelAbortSignal, LanguageModelFileData, LanguageModelFilePart,
    LanguageModelFunctionTool, LanguageModelMessage, LanguageModelPrompt,
    LanguageModelProviderTool, LanguageModelReasoningEffort, LanguageModelStreamPart,
    LanguageModelSupportedUrls, LanguageModelSystemMessage, LanguageModelTool,
    LanguageModelToolApprovalRequestPart, LanguageModelToolApprovalResponsePart,
    LanguageModelToolCall, LanguageModelToolInputDelta, LanguageModelToolInputEnd,
    LanguageModelToolInputExample, LanguageModelToolInputStart, LanguageModelToolResultOutput,
};
use ai_sdk_provider::provider::{
    ApiCallError, EmptyResponseBodyError, InvalidArgumentError, InvalidResponseDataError,
    JsonParseError, LoadApiKeyError, LoadSettingError, ProviderMetadata, ProviderOptions,
    TypeValidationContext, TypeValidationError, UnsupportedFunctionalityError,
};
use ai_sdk_provider::warning::Warning;

pub use ai_sdk_provider::provider::get_error_message;

const DEFAULT_JSON_SCHEMA_INSTRUCTION_PREFIX: &str = "JSON schema:";
const DEFAULT_JSON_SCHEMA_INSTRUCTION_SUFFIX: &str =
    "You MUST answer with a JSON object that matches the JSON schema above.";
const DEFAULT_JSON_INSTRUCTION_SUFFIX: &str = "You MUST answer with JSON.";
const FETCH_FAILED_ERROR_MESSAGES: [&str; 2] = ["fetch failed", "failed to fetch"];
const BUN_NETWORK_ERROR_CODES: [&str; 7] = [
    "ConnectionRefused",
    "ConnectionClosed",
    "FailedToOpenSocket",
    "ECONNRESET",
    "ECONNREFUSED",
    "ETIMEDOUT",
    "EPIPE",
];
const DEFAULT_ID_ALPHABET: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const DEFAULT_ID_SEPARATOR: &str = "-";
const DEFAULT_ID_SIZE: usize = 16;
static ID_GENERATOR_COUNTER: AtomicU64 = AtomicU64::new(0x9e37_79b9_7f4a_7c15);

/// Default maximum response download size used by upstream provider-utils: 2 GiB.
pub const DEFAULT_MAX_DOWNLOAD_SIZE: usize = 2 * 1024 * 1024 * 1024;

/// Boxed future used by [`Resolvable`] for async values.
pub type ResolvableFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Lazy producer used by [`Resolvable`] for values that should be resolved on demand.
pub type ResolvableFunction<'a, T> = Box<dyn FnOnce() -> ResolvableFuture<'a, T> + 'a>;

/// A value or lazy provider of a value, either synchronous or asynchronous.
///
/// This mirrors upstream provider-utils `Resolvable<T>` while using Rust's
/// [`Future`] boundary instead of JavaScript `PromiseLike`.
pub enum Resolvable<'a, T> {
    /// Already available value.
    Value(T),

    /// Future that resolves to the value.
    Future(ResolvableFuture<'a, T>),

    /// Lazy producer that is invoked only when [`resolve`] is called.
    Function(ResolvableFunction<'a, T>),
}

impl<'a, T: 'a> Resolvable<'a, T> {
    /// Creates a resolvable from an already available value.
    pub fn value(value: T) -> Self {
        Self::Value(value)
    }

    /// Creates a resolvable from a future.
    pub fn future<F>(future: F) -> Self
    where
        F: Future<Output = T> + 'a,
    {
        Self::Future(Box::pin(future))
    }

    /// Creates a resolvable from a lazy future producer.
    pub fn function<F, Fut>(function: F) -> Self
    where
        F: FnOnce() -> Fut + 'a,
        Fut: Future<Output = T> + 'a,
    {
        Self::Function(Box::new(|| Box::pin(function())))
    }

    /// Creates a resolvable from a lazy synchronous value producer.
    pub fn lazy_value<F>(function: F) -> Self
    where
        F: FnOnce() -> T + 'a,
    {
        Self::function(|| std::future::ready(function()))
    }
}

impl<'a, T: 'a> From<T> for Resolvable<'a, T> {
    fn from(value: T) -> Self {
        Self::value(value)
    }
}

/// Resolves a raw value, future, lazy value, or lazy future.
///
/// Upstream `resolve` accepts values, promises, functions returning values, and
/// functions returning promises. Rust models thrown or rejected errors by making
/// the resolved type a `Result`.
pub async fn resolve<T>(value: Resolvable<'_, T>) -> T {
    match value {
        Resolvable::Value(value) => value,
        Resolvable::Future(future) => future.await,
        Resolvable::Function(function) => function().await,
    }
}

enum DelayedPromiseStatus<T, E> {
    Pending,
    Resolved(Arc<T>),
    Rejected(Arc<E>),
}

struct DelayedPromiseInner<T, E> {
    status: DelayedPromiseStatus<T, E>,
    promise_created: bool,
    promise_result: Option<Result<Arc<T>, Arc<E>>>,
    wakers: Vec<Waker>,
}

impl<T, E> DelayedPromiseInner<T, E> {
    fn new() -> Self {
        Self {
            status: DelayedPromiseStatus::Pending,
            promise_created: false,
            promise_result: None,
            wakers: Vec::new(),
        }
    }

    fn result_from_status(&self) -> Option<Result<Arc<T>, Arc<E>>> {
        match &self.status {
            DelayedPromiseStatus::Pending => None,
            DelayedPromiseStatus::Resolved(value) => Some(Ok(Arc::clone(value))),
            DelayedPromiseStatus::Rejected(error) => Some(Err(Arc::clone(error))),
        }
    }

    fn wake_pending(&mut self) {
        for waker in self.wakers.drain(..) {
            waker.wake();
        }
    }
}

/// Future returned by [`DelayedPromise::promise`].
pub struct DelayedPromiseFuture<T, E = String> {
    inner: Arc<Mutex<DelayedPromiseInner<T, E>>>,
}

impl<T: Clone, E: Clone> Future for DelayedPromiseFuture<T, E> {
    type Output = Result<T, E>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self
            .inner
            .lock()
            .expect("delayed promise state mutex is not poisoned");

        match &inner.promise_result {
            Some(Ok(value)) => Poll::Ready(Ok((**value).clone())),
            Some(Err(error)) => Poll::Ready(Err((**error).clone())),
            None => {
                if !inner
                    .wakers
                    .iter()
                    .any(|waker| waker.will_wake(context.waker()))
                {
                    inner.wakers.push(context.waker().clone());
                }

                Poll::Pending
            }
        }
    }
}

/// Lazily created externally resolved future.
///
/// This mirrors upstream provider-utils `DelayedPromise`: the future returned by
/// [`promise`](Self::promise) is only materialized when accessed, so resolving or
/// rejecting before access stores the latest state without creating pending
/// async work.
#[derive(Clone)]
pub struct DelayedPromise<T, E = String> {
    inner: Arc<Mutex<DelayedPromiseInner<T, E>>>,
}

impl<T, E> DelayedPromise<T, E> {
    /// Creates a pending delayed promise.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(DelayedPromiseInner::new())),
        }
    }

    /// Returns a future for the delayed result, creating it on first access.
    pub fn promise(&self) -> DelayedPromiseFuture<T, E> {
        let mut inner = self
            .inner
            .lock()
            .expect("delayed promise state mutex is not poisoned");

        if !inner.promise_created {
            inner.promise_created = true;
            inner.promise_result = inner.result_from_status();
        }

        DelayedPromiseFuture {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Resolves the delayed promise.
    pub fn resolve(&self, value: T) {
        let value = Arc::new(value);
        let mut inner = self
            .inner
            .lock()
            .expect("delayed promise state mutex is not poisoned");

        inner.status = DelayedPromiseStatus::Resolved(Arc::clone(&value));

        if inner.promise_created && inner.promise_result.is_none() {
            inner.promise_result = Some(Ok(value));
            inner.wake_pending();
        }
    }

    /// Rejects the delayed promise.
    pub fn reject(&self, error: E) {
        let error = Arc::new(error);
        let mut inner = self
            .inner
            .lock()
            .expect("delayed promise state mutex is not poisoned");

        inner.status = DelayedPromiseStatus::Rejected(Arc::clone(&error));

        if inner.promise_created && inner.promise_result.is_none() {
            inner.promise_result = Some(Err(error));
            inner.wake_pending();
        }
    }

    /// Returns whether the latest delayed promise status is resolved.
    pub fn is_resolved(&self) -> bool {
        matches!(
            self.inner
                .lock()
                .expect("delayed promise state mutex is not poisoned")
                .status,
            DelayedPromiseStatus::Resolved(_)
        )
    }

    /// Returns whether the latest delayed promise status is rejected.
    pub fn is_rejected(&self) -> bool {
        matches!(
            self.inner
                .lock()
                .expect("delayed promise state mutex is not poisoned")
                .status,
            DelayedPromiseStatus::Rejected(_)
        )
    }

    /// Returns whether the latest delayed promise status is pending.
    pub fn is_pending(&self) -> bool {
        matches!(
            self.inner
                .lock()
                .expect("delayed promise state mutex is not poisoned")
                .status,
            DelayedPromiseStatus::Pending
        )
    }
}

impl<T, E> Default for DelayedPromise<T, E> {
    fn default() -> Self {
        Self::new()
    }
}

/// Error returned when an async-iterator stream read fails.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AsyncIteratorReadableStreamError {
    message: String,
}

impl AsyncIteratorReadableStreamError {
    fn new(message: String) -> Self {
        Self { message }
    }

    /// Returns the original iterator error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for AsyncIteratorReadableStreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AsyncIteratorReadableStreamError {}

/// Future returned when an async-iterator stream source reads the next item.
pub type AsyncIteratorReadableStreamNextFuture<'a, T, E> =
    Pin<Box<dyn Future<Output = Result<Option<T>, E>> + 'a>>;

/// Future returned when an async-iterator stream source handles cancellation.
pub type AsyncIteratorReadableStreamReturnFuture<'a, E> =
    Pin<Box<dyn Future<Output = Result<(), E>> + 'a>>;

/// Rust source interface for [`convert_async_iterator_to_readable_stream`].
pub trait AsyncIteratorReadableStreamSource<T> {
    /// Error returned by the source iterator.
    type Error: fmt::Display + 'static;

    /// Pulls the next item from the iterator.
    fn next_item(&mut self) -> AsyncIteratorReadableStreamNextFuture<'_, T, Self::Error>;

    /// Mirrors JavaScript `AsyncIterator.return(reason)`.
    ///
    /// Sources without a return hook can use this default implementation.
    fn return_iterator<'a>(
        &'a mut self,
        _reason: Option<&'a str>,
    ) -> AsyncIteratorReadableStreamReturnFuture<'a, Self::Error> {
        Box::pin(std::future::ready(Ok(())))
    }
}

/// Rust analogue of upstream `convertAsyncIteratorToReadableStream`.
pub struct AsyncIteratorReadableStream<T, I> {
    iterator: I,
    cancelled: bool,
    closed: bool,
    _item: PhantomData<T>,
}

impl<T, I> AsyncIteratorReadableStream<T, I>
where
    I: AsyncIteratorReadableStreamSource<T>,
{
    /// Reads one item from the underlying iterator.
    pub async fn read(&mut self) -> Result<Option<T>, AsyncIteratorReadableStreamError> {
        if self.cancelled || self.closed {
            return Ok(None);
        }

        match self.iterator.next_item().await {
            Ok(Some(value)) => Ok(Some(value)),
            Ok(None) => {
                self.closed = true;
                Ok(None)
            }
            Err(error) => {
                self.closed = true;
                Err(AsyncIteratorReadableStreamError::new(error.to_string()))
            }
        }
    }

    /// Cancels the stream and invokes the source return hook when one exists.
    pub async fn cancel(&mut self, reason: Option<&str>) {
        if self.cancelled {
            self.closed = true;
            return;
        }

        self.cancelled = true;
        self.closed = true;

        let _ = self.iterator.return_iterator(reason).await;
    }

    /// Returns whether the stream has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Returns whether the stream has been closed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Returns the underlying iterator for post-read inspection.
    pub fn iterator(&self) -> &I {
        &self.iterator
    }

    /// Consumes the stream and returns the underlying iterator.
    pub fn into_iterator(self) -> I {
        self.iterator
    }
}

/// Converts an async iterator-like source into a readable stream-like reader.
pub fn convert_async_iterator_to_readable_stream<T, I>(
    iterator: I,
) -> AsyncIteratorReadableStream<T, I>
where
    I: AsyncIteratorReadableStreamSource<T>,
{
    AsyncIteratorReadableStream {
        iterator,
        cancelled: false,
        closed: false,
        _item: PhantomData,
    }
}

/// Error returned when an abortable delay is cancelled.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DelayError {
    message: String,
}

impl DelayError {
    /// Upstream-compatible abort error name.
    pub const NAME: &'static str = "AbortError";

    fn aborted() -> Self {
        Self {
            message: "Delay was aborted".to_string(),
        }
    }

    /// Returns the upstream-compatible error name.
    pub fn name(&self) -> &'static str {
        Self::NAME
    }

    /// Returns the abort error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for DelayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DelayError {}

/// Options for [`delay_with_options`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DelayOptions {
    /// Caller-controlled abort signal for cancelling a pending delay.
    pub abort_signal: Option<LanguageModelAbortSignal>,
}

impl DelayOptions {
    /// Creates delay options with upstream defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the abort signal used to cancel the delay.
    pub fn with_abort_signal(mut self, abort_signal: LanguageModelAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }
}

struct DelayState {
    completed: bool,
    waker: Option<Waker>,
}

struct DelayFuture {
    delay: Option<Duration>,
    abort_signal: Option<LanguageModelAbortSignal>,
    state: Option<Arc<Mutex<DelayState>>>,
}

impl DelayFuture {
    fn new(delay_in_ms: Option<i64>, abort_signal: Option<LanguageModelAbortSignal>) -> Self {
        Self {
            delay: delay_in_ms.map(|delay| Duration::from_millis(delay.max(0) as u64)),
            abort_signal,
            state: None,
        }
    }
}

impl Future for DelayFuture {
    type Output = Result<(), DelayError>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(delay) = self.delay else {
            return Poll::Ready(Ok(()));
        };

        if let Some(state) = &self.state {
            let mut state = state.lock().expect("delay state mutex is not poisoned");
            if state.completed {
                Poll::Ready(Ok(()))
            } else {
                if self
                    .abort_signal
                    .as_ref()
                    .is_some_and(|abort_signal| abort_signal.poll_aborted(context).is_ready())
                {
                    return Poll::Ready(Err(DelayError::aborted()));
                }
                state.waker = Some(context.waker().clone());
                Poll::Pending
            }
        } else {
            if self
                .abort_signal
                .as_ref()
                .is_some_and(|abort_signal| abort_signal.poll_aborted(context).is_ready())
            {
                return Poll::Ready(Err(DelayError::aborted()));
            }

            let state = Arc::new(Mutex::new(DelayState {
                completed: false,
                waker: Some(context.waker().clone()),
            }));
            let sleeper_state = Arc::clone(&state);

            let _sleeper = std::thread::spawn(move || {
                if !delay.is_zero() {
                    std::thread::sleep(delay);
                }

                let waker = {
                    let mut state = sleeper_state
                        .lock()
                        .expect("delay state mutex is not poisoned");
                    state.completed = true;
                    state.waker.take()
                };

                if let Some(waker) = waker {
                    waker.wake();
                }
            });

            self.state = Some(state);
            Poll::Pending
        }
    }
}

/// Creates a future that resolves after a delay in milliseconds.
///
/// This mirrors upstream provider-utils `delay`: `None` resolves immediately,
/// while numeric delays use timer-like deferred completion.
pub async fn delay(delay_in_ms: Option<i64>) {
    let _ = delay_with_options(delay_in_ms, DelayOptions::default()).await;
}

/// Creates a future that resolves after a delay or fails when aborted.
pub fn delay_with_options(
    delay_in_ms: Option<i64>,
    options: DelayOptions,
) -> impl Future<Output = Result<(), DelayError>> {
    DelayFuture::new(delay_in_ms, options.abort_signal)
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn default_true() -> bool {
    true
}

fn is_streaming_tool_call_type_validation_none(value: &StreamingToolCallTypeValidation) -> bool {
    matches!(value, StreamingToolCallTypeValidation::None)
}

/// Error returned when inline file data cannot be converted to raw bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InlineFileDataBytesError {
    /// The supplied file data is a URL or provider reference rather than inline content.
    NonInlineFileData,

    /// The supplied inline data is not valid base64.
    InvalidBase64Data,
}

impl fmt::Display for InlineFileDataBytesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonInlineFileData => formatter.write_str("file data must be inline data or text"),
            Self::InvalidBase64Data => formatter.write_str("invalid base64 inline file data"),
        }
    }
}

impl std::error::Error for InlineFileDataBytesError {}

/// Error returned when base64 data cannot be decoded into bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Base64DecodeError;

impl fmt::Display for Base64DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid base64 data")
    }
}

impl std::error::Error for Base64DecodeError {}

/// Error returned when a URL is unsafe or failed to download.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadError {
    url: String,
    status_code: Option<u16>,
    status_text: Option<String>,
    cause_message: Option<String>,
    message: String,
}

impl DownloadError {
    /// Upstream JavaScript error name.
    pub const NAME: &'static str = "AI_DownloadError";

    /// Creates a download error with a caller-supplied message.
    pub fn new(url: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            status_code: None,
            status_text: None,
            cause_message: None,
            message: message.into(),
        }
    }

    /// Creates a download error from an HTTP response status.
    pub fn with_status(
        url: impl Into<String>,
        status_code: u16,
        status_text: impl Into<String>,
    ) -> Self {
        let url = url.into();
        let status_text = status_text.into();
        Self {
            message: format!("Failed to download {url}: {status_code} {status_text}"),
            url,
            status_code: Some(status_code),
            status_text: Some(status_text),
            cause_message: None,
        }
    }

    /// Creates a download error from a lower-level failure message.
    pub fn with_cause_message(url: impl Into<String>, cause_message: impl fmt::Display) -> Self {
        let url = url.into();
        let cause_message = cause_message.to_string();
        Self {
            message: format!("Failed to download {url}: {cause_message}"),
            url,
            status_code: None,
            status_text: None,
            cause_message: Some(cause_message),
        }
    }

    /// Returns the upstream JavaScript error name.
    pub fn name(&self) -> &'static str {
        Self::NAME
    }

    /// Returns the URL that failed validation or download.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns the response status code when one was available.
    pub fn status_code(&self) -> Option<u16> {
        self.status_code
    }

    /// Returns the response status text when one was available.
    pub fn status_text(&self) -> Option<&str> {
        self.status_text.as_deref()
    }

    /// Returns the lower-level failure message when one was supplied.
    pub fn cause_message(&self) -> Option<&str> {
        self.cause_message.as_deref()
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its URL.
    pub fn into_url(self) -> String {
        self.url
    }
}

impl fmt::Display for DownloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DownloadError {}

/// Options for creating upstream-style provider-utils ID generators.
///
/// Upstream `createIdGenerator` creates non-cryptographic random IDs with an
/// optional prefix. Rust represents the generator configuration as explicit
/// data, while [`create_id_generator`] returns the callable generator.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdGeneratorOptions {
    /// Optional ID prefix. When present, generated IDs are
    /// `{prefix}{separator}{random_part}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,

    /// Separator between the prefix and random part.
    #[serde(default = "default_id_separator")]
    pub separator: String,

    /// Length of the random ID part.
    #[serde(default = "default_id_size")]
    pub size: usize,

    /// Alphabet used for the random ID part.
    #[serde(default = "default_id_alphabet")]
    pub alphabet: String,
}

impl Default for IdGeneratorOptions {
    fn default() -> Self {
        Self {
            prefix: None,
            separator: default_id_separator(),
            size: DEFAULT_ID_SIZE,
            alphabet: default_id_alphabet(),
        }
    }
}

impl IdGeneratorOptions {
    /// Creates ID generator options with upstream defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the optional generated ID prefix.
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Sets the separator between the prefix and random part.
    pub fn with_separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Sets the length of the random ID part.
    pub fn with_size(mut self, size: usize) -> Self {
        self.size = size;
        self
    }

    /// Sets the alphabet used for the random ID part.
    pub fn with_alphabet(mut self, alphabet: impl Into<String>) -> Self {
        self.alphabet = alphabet.into();
        self
    }
}

/// Serialized provider model options for workflow step boundaries.
///
/// This mirrors the upstream `serializeModelOptions` result shape: the model
/// identifier is preserved and the config contains only JSON-serializable
/// entries. Rust callers represent non-serializable JavaScript values such as
/// functions as `None` when using [`serialize_model_options`].
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedModelOptions {
    /// Provider model identifier.
    pub model_id: String,

    /// JSON-serializable provider configuration.
    pub config: JsonObject,
}

impl SerializedModelOptions {
    /// Creates serialized model options.
    pub fn new(model_id: impl Into<String>, config: JsonObject) -> Self {
        Self {
            model_id: model_id.into(),
            config,
        }
    }
}

/// Runtime value used to check whether data can cross a JSON serialization boundary.
///
/// Rust's [`JsonValue`] cannot contain JavaScript functions, symbols, bigints,
/// dates, regular expressions, class instances, or null-prototype objects. This
/// enum models those upstream runtime values explicitly so
/// [`is_json_serializable`] can mirror provider-utils `isJSONSerializable`.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonSerializableValue {
    /// JavaScript `undefined`, accepted by the upstream helper.
    Undefined,

    /// A value already represented by JSON.
    Json(JsonValue),

    /// JavaScript array-like values, including entries that may be unsupported.
    Array(Vec<JsonSerializableValue>),

    /// Plain object values with recursively checked properties.
    PlainObject(BTreeMap<String, JsonSerializableValue>),

    /// JavaScript function value.
    Function,

    /// JavaScript symbol value.
    Symbol,

    /// JavaScript bigint value.
    BigInt,

    /// Non-plain JavaScript object, such as Date, RegExp, class instance, or
    /// null-prototype object.
    NonPlainObject(String),
}

impl JsonSerializableValue {
    /// Creates a JSON-backed serializable value.
    pub fn json(value: impl Into<JsonValue>) -> Self {
        Self::Json(value.into())
    }

    /// Creates a JavaScript array-like value.
    pub fn array(values: impl IntoIterator<Item = JsonSerializableValue>) -> Self {
        Self::Array(values.into_iter().collect())
    }

    /// Creates a JavaScript plain object-like value.
    pub fn plain_object<K, I>(entries: I) -> Self
    where
        I: IntoIterator<Item = (K, JsonSerializableValue)>,
        K: Into<String>,
    {
        Self::PlainObject(
            entries
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    }

    /// Creates a named non-plain JavaScript object value.
    pub fn non_plain_object(kind: impl Into<String>) -> Self {
        Self::NonPlainObject(kind.into())
    }
}

impl From<JsonValue> for JsonSerializableValue {
    fn from(value: JsonValue) -> Self {
        Self::Json(value)
    }
}

impl From<Option<JsonValue>> for JsonSerializableValue {
    fn from(value: Option<JsonValue>) -> Self {
        value.map_or(Self::Undefined, Self::Json)
    }
}

/// Checks whether a value can cross a JSON serialization boundary.
pub fn is_json_serializable(value: &JsonSerializableValue) -> bool {
    match value {
        JsonSerializableValue::Undefined | JsonSerializableValue::Json(_) => true,
        JsonSerializableValue::Array(values) => values.iter().all(is_json_serializable),
        JsonSerializableValue::PlainObject(entries) => entries.values().all(is_json_serializable),
        JsonSerializableValue::Function
        | JsonSerializableValue::Symbol
        | JsonSerializableValue::BigInt
        | JsonSerializableValue::NonPlainObject(_) => false,
    }
}

/// Result returned by a schema validator.
///
/// This mirrors upstream provider-utils `ValidationResult` while retaining an
/// error message instead of a JavaScript `Error` object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidationResult<T = JsonValue> {
    /// Validation succeeded and produced a typed value.
    Success { value: T },

    /// Validation failed with a human-readable message.
    Failure { error: String },
}

impl<T> ValidationResult<T> {
    /// Creates a successful validation result.
    pub fn success(value: T) -> Self {
        Self::Success { value }
    }

    /// Creates a failed validation result.
    pub fn failure(error: impl Into<String>) -> Self {
        Self::Failure {
            error: error.into(),
        }
    }

    /// Returns whether validation succeeded.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Returns whether validation failed.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failure { .. })
    }

    /// Returns the validated value on success.
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Success { value } => Some(value),
            Self::Failure { .. } => None,
        }
    }

    /// Returns the validation error message on failure.
    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Success { .. } => None,
            Self::Failure { error } => Some(error),
        }
    }

    /// Converts this validation result into a Rust `Result`.
    pub fn into_result(self) -> Result<T, String> {
        match self {
            Self::Success { value } => Ok(value),
            Self::Failure { error } => Err(error),
        }
    }
}

impl<T> Serialize for ValidationResult<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("ValidationResult", 2)?;

        match self {
            Self::Success { value } => {
                state.serialize_field("success", &true)?;
                state.serialize_field("value", value)?;
            }
            Self::Failure { error } => {
                state.serialize_field("success", &false)?;
                state.serialize_field("error", error)?;
            }
        }

        state.end()
    }
}

impl<'de, T> Deserialize<'de> for ValidationResult<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct ValidationResultFields<T> {
            success: bool,
            value: Option<T>,
            error: Option<String>,
        }

        let fields = ValidationResultFields::deserialize(deserializer)?;

        if fields.success {
            Ok(Self::success(
                fields
                    .value
                    .ok_or_else(|| de::Error::missing_field("value"))?,
            ))
        } else {
            Ok(Self::failure(
                fields
                    .error
                    .ok_or_else(|| de::Error::missing_field("error"))?,
            ))
        }
    }
}

type SchemaValidator<T> = dyn Fn(&JsonValue) -> ValidationResult<T> + Send + Sync + 'static;
type CreateJsonSchema = dyn Fn() -> JsonSchema + Send + Sync + 'static;
type CreateSchema<T> = dyn Fn() -> Schema<T> + Send + Sync + 'static;

struct JsonSchemaStore {
    json_schema: OnceLock<JsonSchema>,
    create_json_schema: Option<Arc<CreateJsonSchema>>,
}

impl JsonSchemaStore {
    fn eager(json_schema: JsonSchema) -> Self {
        let store = Self {
            json_schema: OnceLock::new(),
            create_json_schema: None,
        };
        let _ = store.json_schema.set(json_schema);
        store
    }

    fn lazy(create_json_schema: Arc<CreateJsonSchema>) -> Self {
        Self {
            json_schema: OnceLock::new(),
            create_json_schema: Some(create_json_schema),
        }
    }

    fn json_schema(&self) -> &JsonSchema {
        self.json_schema.get_or_init(|| {
            self.create_json_schema
                .as_ref()
                .expect("lazy JSON schema store must have a factory")()
        })
    }
}

/// JSON-schema-backed provider-utils schema.
///
/// This is the Rust-native subset of upstream `Schema`: it stores the provider
/// JSON Schema plus an optional synchronous validator. JavaScript-only schema
/// adapters such as Zod and Standard Schema conversion are intentionally left
/// out of this boundary.
pub struct Schema<T = JsonValue> {
    json_schema: Arc<JsonSchemaStore>,
    validate: Option<Arc<SchemaValidator<T>>>,
}

impl<T> Clone for Schema<T> {
    fn clone(&self) -> Self {
        Self {
            json_schema: Arc::clone(&self.json_schema),
            validate: self.validate.clone(),
        }
    }
}

impl<T> Schema<T> {
    /// Creates a schema from an already-built JSON Schema 7 object.
    pub fn new(json_schema: JsonSchema) -> Self {
        Self {
            json_schema: Arc::new(JsonSchemaStore::eager(json_schema)),
            validate: None,
        }
    }

    /// Creates a schema whose JSON Schema object is initialized on first access.
    ///
    /// This mirrors the lazy function branch of upstream provider-utils
    /// `jsonSchema`, including caching the produced JSON Schema for subsequent
    /// accesses and clones.
    pub fn lazy_json_schema<F>(create_json_schema: F) -> Self
    where
        F: Fn() -> JsonSchema + Send + Sync + 'static,
    {
        Self {
            json_schema: Arc::new(JsonSchemaStore::lazy(Arc::new(create_json_schema))),
            validate: None,
        }
    }

    /// Adds a synchronous validator for the schema.
    pub fn with_validator<F>(mut self, validate: F) -> Self
    where
        F: Fn(&JsonValue) -> ValidationResult<T> + Send + Sync + 'static,
    {
        self.validate = Some(Arc::new(validate));
        self
    }

    /// Returns the JSON Schema object passed to providers.
    pub fn json_schema(&self) -> &JsonSchema {
        self.json_schema.json_schema()
    }

    /// Returns whether this schema has a Rust-side validator.
    pub fn has_validator(&self) -> bool {
        self.validate.is_some()
    }

    /// Runs the schema validator when one is present.
    pub fn validate(&self, value: &JsonValue) -> Option<ValidationResult<T>> {
        self.validate.as_ref().map(|validate| validate(value))
    }
}

impl<T> fmt::Debug for Schema<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Schema")
            .field("json_schema", &self.json_schema())
            .field("has_validator", &self.has_validator())
            .finish()
    }
}

/// Creates a provider-utils schema from a JSON Schema 7 object.
pub fn json_schema(json_schema: JsonSchema) -> Schema {
    Schema::new(json_schema)
}

/// Creates a provider-utils schema whose JSON Schema is initialized lazily.
pub fn lazy_json_schema<F>(create_json_schema: F) -> Schema
where
    F: Fn() -> JsonSchema + Send + Sync + 'static,
{
    Schema::lazy_json_schema(create_json_schema)
}

/// Lazily creates and caches a provider-utils schema.
///
/// This mirrors upstream provider-utils `lazySchema`: the schema factory is not
/// called until the schema is requested, and the resulting schema is reused for
/// all later accesses and clones.
pub struct LazySchema<T = JsonValue> {
    inner: Arc<LazySchemaInner<T>>,
}

struct LazySchemaInner<T> {
    schema: OnceLock<Schema<T>>,
    create_schema: Arc<CreateSchema<T>>,
}

impl<T> Clone for LazySchema<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> LazySchema<T> {
    /// Creates a lazy schema from a schema factory.
    pub fn new<F>(create_schema: F) -> Self
    where
        F: Fn() -> Schema<T> + Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(LazySchemaInner {
                schema: OnceLock::new(),
                create_schema: Arc::new(create_schema),
            }),
        }
    }

    /// Returns the cached schema, creating it on first access.
    pub fn schema(&self) -> &Schema<T> {
        self.inner
            .schema
            .get_or_init(|| (self.inner.create_schema)())
    }

    /// Returns whether the schema factory has already been evaluated.
    pub fn is_initialized(&self) -> bool {
        self.inner.schema.get().is_some()
    }
}

impl<T> fmt::Debug for LazySchema<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LazySchema")
            .field("is_initialized", &self.is_initialized())
            .finish()
    }
}

/// Creates a lazily initialized provider-utils schema.
pub fn lazy_schema<T, F>(create_schema: F) -> LazySchema<T>
where
    F: Fn() -> Schema<T> + Send + Sync + 'static,
{
    LazySchema::new(create_schema)
}

type StandardSchemaCreateJsonSchema =
    dyn Fn(StandardSchemaJsonSchemaOptions) -> JsonSchema + Send + Sync + 'static;

/// Options passed when a Standard Schema produces provider-facing JSON Schema.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StandardSchemaJsonSchemaOptions {
    /// JSON Schema target draft requested by provider-utils.
    pub target: Option<String>,
}

impl StandardSchemaJsonSchemaOptions {
    /// Creates Standard Schema JSON Schema options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the requested JSON Schema target draft.
    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }
}

/// Rust-native representation of upstream Standard Schema v1.
#[derive(Clone)]
pub struct StandardSchema<T = JsonValue> {
    vendor: String,
    create_json_schema: Arc<StandardSchemaCreateJsonSchema>,
    validate: Arc<SchemaValidator<T>>,
}

impl<T> StandardSchema<T> {
    /// Creates a Standard Schema from a static JSON Schema and validator.
    pub fn new<F>(vendor: impl Into<String>, json_schema: JsonSchema, validate: F) -> Self
    where
        F: Fn(&JsonValue) -> ValidationResult<T> + Send + Sync + 'static,
    {
        Self::with_json_schema(vendor, move |_options| json_schema.clone(), validate)
    }

    /// Creates a Standard Schema from a JSON Schema callback and validator.
    pub fn with_json_schema<F, V>(
        vendor: impl Into<String>,
        create_json_schema: F,
        validate: V,
    ) -> Self
    where
        F: Fn(StandardSchemaJsonSchemaOptions) -> JsonSchema + Send + Sync + 'static,
        V: Fn(&JsonValue) -> ValidationResult<T> + Send + Sync + 'static,
    {
        Self {
            vendor: vendor.into(),
            create_json_schema: Arc::new(create_json_schema),
            validate: Arc::new(validate),
        }
    }

    /// Returns the Standard Schema vendor identifier.
    pub fn vendor(&self) -> &str {
        &self.vendor
    }

    fn into_schema(self) -> Schema<T>
    where
        T: 'static,
    {
        let create_json_schema = Arc::clone(&self.create_json_schema);
        let validate = Arc::clone(&self.validate);

        Schema::lazy_json_schema(move || {
            add_additional_properties_to_json_schema(create_json_schema(
                StandardSchemaJsonSchemaOptions::new().with_target("draft-07"),
            ))
        })
        .with_validator(move |value| validate(value))
    }
}

impl<T> fmt::Debug for StandardSchema<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StandardSchema")
            .field("vendor", &self.vendor)
            .finish_non_exhaustive()
    }
}

/// Rust-native subset of upstream provider-utils `FlexibleSchema`.
///
/// JavaScript schema adapters such as Zod are intentionally left to future
/// slices, but concrete, lazy, and Standard Schema provider-utils schemas can
/// already share normalization behavior.
#[derive(Clone)]
pub enum FlexibleSchema<T = JsonValue> {
    /// Already constructed provider-utils schema.
    Schema(Schema<T>),

    /// Lazily created provider-utils schema.
    Lazy(LazySchema<T>),

    /// Standard Schema v1 wrapper converted through its JSON Schema input.
    Standard(StandardSchema<T>),
}

impl<T> FlexibleSchema<T> {
    /// Returns the concrete schema, evaluating lazy schemas on first access.
    pub fn as_schema(&self) -> &Schema<T> {
        match self {
            Self::Schema(schema) => schema,
            Self::Lazy(schema) => schema.schema(),
            Self::Standard(_) => panic!("standard schemas must be converted with into_schema"),
        }
    }

    /// Converts this flexible schema into a concrete schema.
    pub fn into_schema(self) -> Schema<T>
    where
        T: 'static,
    {
        match self {
            Self::Schema(schema) => schema,
            Self::Lazy(schema) => schema.schema().clone(),
            Self::Standard(schema) => schema.into_schema(),
        }
    }
}

impl<T> fmt::Debug for FlexibleSchema<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Schema(schema) => formatter.debug_tuple("Schema").field(schema).finish(),
            Self::Lazy(schema) => formatter.debug_tuple("Lazy").field(schema).finish(),
            Self::Standard(schema) => formatter.debug_tuple("Standard").field(schema).finish(),
        }
    }
}

impl<T> From<Schema<T>> for FlexibleSchema<T> {
    fn from(schema: Schema<T>) -> Self {
        Self::Schema(schema)
    }
}

impl<T> From<LazySchema<T>> for FlexibleSchema<T> {
    fn from(schema: LazySchema<T>) -> Self {
        Self::Lazy(schema)
    }
}

impl<T> From<StandardSchema<T>> for FlexibleSchema<T> {
    fn from(schema: StandardSchema<T>) -> Self {
        Self::Standard(schema)
    }
}

/// Creates a provider-utils Standard Schema wrapper.
pub fn standard_schema<T, F>(
    vendor: impl Into<String>,
    json_schema: JsonSchema,
    validate: F,
) -> StandardSchema<T>
where
    F: Fn(&JsonValue) -> ValidationResult<T> + Send + Sync + 'static,
{
    StandardSchema::new(vendor, json_schema, validate)
}

/// Normalizes an optional schema, defaulting to an empty closed object schema.
///
/// This mirrors the `undefined` branch of upstream `asSchema`.
pub fn as_schema(schema: Option<Schema>) -> Schema {
    schema.unwrap_or_else(default_schema)
}

/// Normalizes an optional concrete or lazy schema.
pub fn as_flexible_schema<T: 'static>(schema: Option<FlexibleSchema<T>>) -> Schema<T> {
    schema.map_or_else(default_schema, FlexibleSchema::into_schema)
}

fn default_schema<T>() -> Schema<T> {
    Schema::new(JsonObject::from_iter([
        ("type".to_string(), JsonValue::String("object".to_string())),
        (
            "properties".to_string(),
            JsonValue::Object(JsonObject::new()),
        ),
        ("additionalProperties".to_string(), JsonValue::Bool(false)),
    ]))
}

/// Scalar value stored in dependency-free multipart form data.
///
/// Upstream `convertToFormData` appends JavaScript strings and `Blob`s to a
/// browser `FormData` object. Rust keeps the same logical boundary as ordered
/// text or byte entries so HTTP adapters can choose their multipart encoder.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum FormDataValue {
    /// Text form value.
    Text {
        /// Text content for this form field.
        value: String,
    },

    /// Binary form value.
    Bytes {
        /// Raw bytes for this form field.
        value: Vec<u8>,
    },
}

impl FormDataValue {
    /// Creates a text form value.
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text {
            value: value.into(),
        }
    }

    /// Creates a binary form value.
    pub fn bytes(value: impl Into<Vec<u8>>) -> Self {
        Self::Bytes {
            value: value.into(),
        }
    }
}

impl From<String> for FormDataValue {
    fn from(value: String) -> Self {
        Self::text(value)
    }
}

impl From<&str> for FormDataValue {
    fn from(value: &str) -> Self {
        Self::text(value)
    }
}

impl From<Vec<u8>> for FormDataValue {
    fn from(value: Vec<u8>) -> Self {
        Self::bytes(value)
    }
}

/// Input value accepted by [`convert_to_form_data`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum FormDataInputValue {
    /// Single text value.
    Text {
        /// Text content for this form field.
        value: String,
    },

    /// Single binary value.
    Bytes {
        /// Raw bytes for this form field.
        value: Vec<u8>,
    },

    /// Repeated values for this form field.
    Array {
        /// Ordered values for this form field.
        values: Vec<FormDataValue>,
    },
}

impl FormDataInputValue {
    /// Creates a single text input value.
    pub fn text(value: impl Into<String>) -> Self {
        Self::Text {
            value: value.into(),
        }
    }

    /// Creates a single binary input value.
    pub fn bytes(value: impl Into<Vec<u8>>) -> Self {
        Self::Bytes {
            value: value.into(),
        }
    }

    /// Creates repeated input values.
    pub fn array(values: Vec<FormDataValue>) -> Self {
        Self::Array { values }
    }

    fn into_values(self) -> Vec<FormDataValue> {
        match self {
            Self::Text { value } => vec![FormDataValue::text(value)],
            Self::Bytes { value } => vec![FormDataValue::bytes(value)],
            Self::Array { values } => values,
        }
    }
}

impl From<FormDataValue> for FormDataInputValue {
    fn from(value: FormDataValue) -> Self {
        match value {
            FormDataValue::Text { value } => Self::text(value),
            FormDataValue::Bytes { value } => Self::bytes(value),
        }
    }
}

impl From<String> for FormDataInputValue {
    fn from(value: String) -> Self {
        Self::text(value)
    }
}

impl From<&str> for FormDataInputValue {
    fn from(value: &str) -> Self {
        Self::text(value)
    }
}

impl From<Vec<u8>> for FormDataInputValue {
    fn from(value: Vec<u8>) -> Self {
        Self::bytes(value)
    }
}

/// One ordered multipart form data entry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormDataEntry {
    /// Field name for this entry.
    pub name: String,

    /// Field value.
    pub value: FormDataValue,
}

impl FormDataEntry {
    /// Creates a form data entry.
    pub fn new(name: impl Into<String>, value: FormDataValue) -> Self {
        Self {
            name: name.into(),
            value,
        }
    }
}

/// Dependency-free representation of ordered multipart form data entries.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormData {
    /// Ordered form entries.
    pub entries: Vec<FormDataEntry>,
}

impl FormData {
    /// Creates empty form data.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a form entry.
    pub fn append(&mut self, name: impl Into<String>, value: FormDataValue) {
        self.entries.push(FormDataEntry::new(name, value));
    }

    /// Returns true when at least one entry exists for the field name.
    pub fn has(&self, name: &str) -> bool {
        self.entries.iter().any(|entry| entry.name == name)
    }

    /// Returns the first value for the field name.
    pub fn get(&self, name: &str) -> Option<&FormDataValue> {
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| &entry.value)
    }

    /// Returns all values for the field name in append order.
    pub fn get_all(&self, name: &str) -> Vec<&FormDataValue> {
        self.entries
            .iter()
            .filter(|entry| entry.name == name)
            .map(|entry| &entry.value)
            .collect()
    }
}

/// Options for [`convert_to_form_data`].
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertToFormDataOptions {
    /// Whether multi-value array fields use the upstream `[]` suffix.
    #[serde(default = "default_true")]
    pub use_array_brackets: bool,
}

impl Default for ConvertToFormDataOptions {
    fn default() -> Self {
        Self {
            use_array_brackets: true,
        }
    }
}

impl ConvertToFormDataOptions {
    /// Creates options with upstream defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets whether multi-value array fields use the upstream `[]` suffix.
    pub fn with_use_array_brackets(mut self, use_array_brackets: bool) -> Self {
        self.use_array_brackets = use_array_brackets;
        self
    }
}

/// Converts an input map to ordered multipart form data entries.
///
/// This mirrors upstream `convertToFormData`: missing values are skipped,
/// empty arrays add no entries, one-element arrays use the original key, and
/// multi-element arrays use a `[]` suffix unless disabled by options.
pub fn convert_to_form_data(
    input: impl IntoIterator<Item = (String, Option<FormDataInputValue>)>,
    options: ConvertToFormDataOptions,
) -> FormData {
    let mut form_data = FormData::new();

    for (key, value) in input {
        let Some(value) = value else {
            continue;
        };

        let values = value.into_values();
        let form_key = if values.len() > 1 && options.use_array_brackets {
            format!("{key}[]")
        } else {
            key
        };

        for value in values {
            form_data.append(form_key.clone(), value);
        }
    }

    form_data
}

/// Options for downloading a URL into a dependency-free blob value.
///
/// Upstream `downloadBlob` accepts a URL and an optional `maxBytes` value.
/// Rust omits JavaScript-only `AbortSignal` and lets callers inject the HTTP
/// transport in [`download_blob`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadBlobOptions {
    /// URL to download.
    pub url: String,

    /// Maximum accepted response body size in bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<usize>,
}

impl DownloadBlobOptions {
    /// Creates download options for a URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            max_bytes: None,
        }
    }

    /// Sets the maximum accepted response body size in bytes.
    pub fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = Some(max_bytes);
        self
    }
}

/// HTTP response data supplied to [`download_blob`] by an adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadBlobResponse {
    /// HTTP response status code.
    pub status_code: u16,

    /// HTTP response status text.
    pub status_text: String,

    /// Headers extracted from the HTTP response.
    #[serde(default)]
    pub headers: Headers,

    /// Downloaded response body bytes. Missing bodies are treated as empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Vec<u8>>,

    /// Final URL after an HTTP redirect, when the adapter followed one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_url: Option<String>,
}

impl DownloadBlobResponse {
    /// Creates a response without a body.
    pub fn new(status_code: u16, status_text: impl Into<String>) -> Self {
        Self {
            status_code,
            status_text: status_text.into(),
            headers: Headers::new(),
            body: None,
            final_url: None,
        }
    }

    /// Creates a response with byte body content.
    pub fn bytes(
        status_code: u16,
        status_text: impl Into<String>,
        body: impl Into<Vec<u8>>,
    ) -> Self {
        Self::new(status_code, status_text).with_body(body)
    }

    /// Adds response headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = headers;
        self
    }

    /// Adds byte body content.
    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Marks the response as redirected to a final URL.
    pub fn with_final_url(mut self, final_url: impl Into<String>) -> Self {
        self.final_url = Some(final_url.into());
        self
    }

    fn is_success_status(&self) -> bool {
        (200..=299).contains(&self.status_code)
    }
}

/// Dependency-free blob returned by [`download_blob`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadedBlob {
    /// Downloaded bytes.
    pub data: Vec<u8>,

    /// Response media type from the `content-type` header, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

impl DownloadedBlob {
    /// Creates a downloaded blob from bytes.
    pub fn new(data: impl Into<Vec<u8>>) -> Self {
        Self {
            data: data.into(),
            media_type: None,
        }
    }

    /// Sets the response media type.
    pub fn with_media_type(mut self, media_type: impl Into<String>) -> Self {
        self.media_type = Some(media_type.into());
        self
    }
}

/// Validation mode for OpenAI-compatible streaming tool-call deltas.
///
/// Upstream `StreamingToolCallTracker` accepts `none`, `if-present`, or
/// `required` to control whether the delta `type` field must be `function`.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StreamingToolCallTypeValidation {
    /// Do not validate the delta `type` field.
    #[default]
    None,

    /// Validate the delta `type` field only when it is present.
    IfPresent,

    /// Require the delta `type` field to be exactly `function`.
    Required,
}

/// Options for a [`StreamingToolCallTracker`].
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingToolCallTrackerOptions {
    /// How to validate the `type` field on newly observed tool-call deltas.
    #[serde(
        default,
        skip_serializing_if = "is_streaming_tool_call_type_validation_none"
    )]
    pub type_validation: StreamingToolCallTypeValidation,
}

impl StreamingToolCallTrackerOptions {
    /// Creates streaming tool-call tracker options with upstream defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the delta `type` validation mode.
    pub fn with_type_validation(
        mut self,
        type_validation: StreamingToolCallTypeValidation,
    ) -> Self {
        self.type_validation = type_validation;
        self
    }
}

/// Function payload carried by an OpenAI-compatible streaming tool-call delta.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingToolCallDeltaFunction {
    /// Name of the function-style tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Incremental JSON input text for the tool call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

impl StreamingToolCallDeltaFunction {
    /// Creates an empty streaming tool-call function payload.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the tool function name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the incremental JSON arguments text.
    pub fn with_arguments(mut self, arguments: impl Into<String>) -> Self {
        self.arguments = Some(arguments.into());
        self
    }
}

/// Minimal OpenAI-compatible streaming tool-call delta.
///
/// The upstream tracker accepts provider-specific delta extensions. Rust keeps
/// those extensions in [`StreamingToolCallDelta::extra`] so provider adapters can
/// preserve metadata without forcing a provider-specific type into this crate.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamingToolCallDelta {
    /// Tool-call index in the provider stream.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,

    /// Provider-supplied tool-call identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Provider-supplied delta type, expected to be `function` depending on validation mode.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,

    /// Function tool-call details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<StreamingToolCallDeltaFunction>,

    /// Provider-specific extension fields preserved from the source delta.
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

impl StreamingToolCallDelta {
    /// Creates an empty streaming tool-call delta.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the provider stream index for the delta.
    pub fn with_index(mut self, index: usize) -> Self {
        self.index = Some(index);
        self
    }

    /// Sets the provider tool-call identifier.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the provider delta type.
    pub fn with_type(mut self, call_type: impl Into<String>) -> Self {
        self.call_type = Some(call_type.into());
        self
    }

    /// Sets the function payload for the delta.
    pub fn with_function(mut self, function: StreamingToolCallDeltaFunction) -> Self {
        self.function = Some(function);
        self
    }

    /// Adds a provider-specific extension value.
    pub fn with_extra_value(mut self, key: impl Into<String>, value: JsonValue) -> Self {
        self.extra.insert(key.into(), value);
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TrackedStreamingToolCall {
    id: Option<String>,
    function_name: String,
    arguments: String,
    has_finished: bool,
    metadata: Option<ProviderMetadata>,
}

type StreamingToolCallGenerateId = dyn Fn() -> String + Send + Sync + 'static;
type StreamingToolCallExtractMetadata =
    dyn Fn(&StreamingToolCallDelta) -> Option<ProviderMetadata> + Send + Sync + 'static;
type StreamingToolCallBuildProviderMetadata =
    dyn Fn(Option<&ProviderMetadata>) -> Option<ProviderMetadata> + Send + Sync + 'static;

/// Tracks streaming tool-call state across multiple OpenAI-compatible deltas.
///
/// Upstream uses a stream controller and enqueues language model stream parts.
/// This Rust boundary returns the parts produced by each processed delta, which
/// keeps the helper dependency-free and easy to compose with any async stream.
#[derive(Clone)]
pub struct StreamingToolCallTracker {
    tool_calls: Vec<Option<TrackedStreamingToolCall>>,
    generate_id: Arc<StreamingToolCallGenerateId>,
    type_validation: StreamingToolCallTypeValidation,
    extract_metadata: Option<Arc<StreamingToolCallExtractMetadata>>,
    build_tool_call_provider_metadata: Option<Arc<StreamingToolCallBuildProviderMetadata>>,
}

impl Default for StreamingToolCallTracker {
    fn default() -> Self {
        Self::from_options(StreamingToolCallTrackerOptions::default())
    }
}

impl StreamingToolCallTracker {
    /// Creates a streaming tool-call tracker with upstream defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a streaming tool-call tracker from explicit options.
    pub fn from_options(options: StreamingToolCallTrackerOptions) -> Self {
        Self {
            tool_calls: Vec::new(),
            generate_id: Arc::new(generate_id),
            type_validation: options.type_validation,
            extract_metadata: None,
            build_tool_call_provider_metadata: None,
        }
    }

    /// Sets the ID generator used by the upstream fallback path.
    pub fn with_generate_id<F>(mut self, generate_id: F) -> Self
    where
        F: Fn() -> String + Send + Sync + 'static,
    {
        self.generate_id = Arc::new(generate_id);
        self
    }

    /// Extracts provider metadata from a newly observed tool-call delta.
    pub fn with_extract_metadata<F>(mut self, extract_metadata: F) -> Self
    where
        F: Fn(&StreamingToolCallDelta) -> Option<ProviderMetadata> + Send + Sync + 'static,
    {
        self.extract_metadata = Some(Arc::new(extract_metadata));
        self
    }

    /// Builds provider metadata for the final `tool-call` stream part.
    pub fn with_tool_call_provider_metadata<F>(mut self, build_provider_metadata: F) -> Self
    where
        F: Fn(Option<&ProviderMetadata>) -> Option<ProviderMetadata> + Send + Sync + 'static,
    {
        self.build_tool_call_provider_metadata = Some(Arc::new(build_provider_metadata));
        self
    }

    /// Processes one provider tool-call delta and returns emitted stream parts.
    pub fn process_delta(
        &mut self,
        delta: StreamingToolCallDelta,
    ) -> Result<Vec<LanguageModelStreamPart>, InvalidResponseDataError> {
        let index = delta.index.unwrap_or(self.tool_calls.len());

        if self
            .tool_calls
            .get(index)
            .and_then(Option::as_ref)
            .is_some()
        {
            self.process_existing_tool_call(index, &delta)
        } else {
            self.process_new_tool_call(index, &delta)
        }
    }

    /// Finalizes unfinished tool calls and returns the emitted stream parts.
    pub fn flush(&mut self) -> Vec<LanguageModelStreamPart> {
        let generate_id = Arc::clone(&self.generate_id);
        let build_provider_metadata = self.build_tool_call_provider_metadata.clone();
        let mut parts = Vec::new();

        for tool_call in self.tool_calls.iter_mut().flatten() {
            if !tool_call.has_finished {
                finish_streaming_tool_call(
                    tool_call,
                    &generate_id,
                    build_provider_metadata.as_deref(),
                    &mut parts,
                );
            }
        }

        parts
    }

    fn process_new_tool_call(
        &mut self,
        index: usize,
        delta: &StreamingToolCallDelta,
    ) -> Result<Vec<LanguageModelStreamPart>, InvalidResponseDataError> {
        self.validate_delta_type(delta)?;

        let id = delta.id.clone().ok_or_else(|| {
            invalid_streaming_tool_call_delta_error(delta, "Expected 'id' to be a string.")
        })?;
        let function = delta.function.as_ref();
        let function_name = function
            .and_then(|function| function.name.clone())
            .ok_or_else(|| {
                invalid_streaming_tool_call_delta_error(
                    delta,
                    "Expected 'function.name' to be a string.",
                )
            })?;
        let arguments = function
            .and_then(|function| function.arguments.clone())
            .unwrap_or_default();

        let mut parts = vec![LanguageModelStreamPart::ToolInputStart(
            LanguageModelToolInputStart::new(id.clone(), function_name.clone()),
        )];
        let metadata = self
            .extract_metadata
            .as_ref()
            .and_then(|extract_metadata| extract_metadata(delta));

        if self.tool_calls.len() <= index {
            self.tool_calls.resize_with(index + 1, || None);
        }

        self.tool_calls[index] = Some(TrackedStreamingToolCall {
            id: Some(id),
            function_name,
            arguments: arguments.clone(),
            has_finished: false,
            metadata,
        });

        if !arguments.is_empty() {
            let tool_call = self.tool_calls[index]
                .as_ref()
                .expect("new tool call was inserted");
            parts.push(LanguageModelStreamPart::ToolInputDelta(
                LanguageModelToolInputDelta::new(
                    tool_call.id.as_deref().unwrap_or_default(),
                    arguments.clone(),
                ),
            ));
        }

        if is_parsable_json(&arguments) {
            let generate_id = Arc::clone(&self.generate_id);
            let build_provider_metadata = self.build_tool_call_provider_metadata.clone();
            let tool_call = self.tool_calls[index]
                .as_mut()
                .expect("new tool call was inserted");
            finish_streaming_tool_call(
                tool_call,
                &generate_id,
                build_provider_metadata.as_deref(),
                &mut parts,
            );
        }

        Ok(parts)
    }

    fn process_existing_tool_call(
        &mut self,
        index: usize,
        delta: &StreamingToolCallDelta,
    ) -> Result<Vec<LanguageModelStreamPart>, InvalidResponseDataError> {
        let Some(tool_call) = self.tool_calls.get_mut(index).and_then(Option::as_mut) else {
            return Ok(Vec::new());
        };

        if tool_call.has_finished {
            return Ok(Vec::new());
        }

        let mut parts = Vec::new();

        if let Some(arguments) = delta
            .function
            .as_ref()
            .and_then(|function| function.arguments.as_ref())
        {
            tool_call.arguments.push_str(arguments);
            parts.push(LanguageModelStreamPart::ToolInputDelta(
                LanguageModelToolInputDelta::new(
                    tool_call.id.as_deref().unwrap_or_default(),
                    arguments.clone(),
                ),
            ));
        }

        if is_parsable_json(&tool_call.arguments) {
            let generate_id = Arc::clone(&self.generate_id);
            let build_provider_metadata = self.build_tool_call_provider_metadata.clone();
            finish_streaming_tool_call(
                tool_call,
                &generate_id,
                build_provider_metadata.as_deref(),
                &mut parts,
            );
        }

        Ok(parts)
    }

    fn validate_delta_type(
        &self,
        delta: &StreamingToolCallDelta,
    ) -> Result<(), InvalidResponseDataError> {
        match self.type_validation {
            StreamingToolCallTypeValidation::None => Ok(()),
            StreamingToolCallTypeValidation::IfPresent => {
                if delta
                    .call_type
                    .as_deref()
                    .is_some_and(|call_type| call_type != "function")
                {
                    Err(invalid_streaming_tool_call_delta_error(
                        delta,
                        "Expected 'function' type.",
                    ))
                } else {
                    Ok(())
                }
            }
            StreamingToolCallTypeValidation::Required => {
                if delta.call_type.as_deref() == Some("function") {
                    Ok(())
                } else {
                    Err(invalid_streaming_tool_call_delta_error(
                        delta,
                        "Expected 'function' type.",
                    ))
                }
            }
        }
    }
}

fn finish_streaming_tool_call(
    tool_call: &mut TrackedStreamingToolCall,
    generate_id: &Arc<StreamingToolCallGenerateId>,
    build_provider_metadata: Option<&StreamingToolCallBuildProviderMetadata>,
    parts: &mut Vec<LanguageModelStreamPart>,
) {
    let id = tool_call
        .id
        .clone()
        .unwrap_or_else(|| (generate_id.as_ref())());

    parts.push(LanguageModelStreamPart::ToolInputEnd(
        LanguageModelToolInputEnd::new(id.clone()),
    ));

    let provider_metadata =
        build_provider_metadata.and_then(|build| build(tool_call.metadata.as_ref()));
    let mut tool_call_part = LanguageModelToolCall::new(
        id,
        tool_call.function_name.clone(),
        tool_call.arguments.clone(),
    );

    if let Some(provider_metadata) = provider_metadata {
        tool_call_part = tool_call_part.with_provider_metadata(provider_metadata);
    }

    parts.push(LanguageModelStreamPart::ToolCall(tool_call_part));
    tool_call.has_finished = true;
}

fn invalid_streaming_tool_call_delta_error(
    delta: &StreamingToolCallDelta,
    message: &'static str,
) -> InvalidResponseDataError {
    InvalidResponseDataError::with_message(
        serde_json::to_value(delta).expect("streaming tool-call deltas serialize"),
        message,
    )
}

fn default_id_alphabet() -> String {
    DEFAULT_ID_ALPHABET.to_string()
}

fn default_id_separator() -> String {
    DEFAULT_ID_SEPARATOR.to_string()
}

const fn default_id_size() -> usize {
    DEFAULT_ID_SIZE
}

/// Runtime indicators used to build the provider-utils user-agent suffix.
///
/// Upstream `getRuntimeEnvironmentUserAgent` probes JavaScript globals in a
/// fixed order. Rust callers can supply equivalent indicators explicitly while
/// the default unknown environment maps to the upstream fallback.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeEnvironment {
    /// Whether a browser-like `window` global is present.
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_window: bool,

    /// Browser, worker, Deno, Bun, or Node >= 21.1 navigator user agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub navigator_user_agent: Option<String>,

    /// Node.js `process.version` value for Node runtimes without navigator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_version: Option<String>,

    /// Whether Vercel Edge Runtime is present.
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_edge_runtime: bool,
}

impl RuntimeEnvironment {
    /// Creates an unknown runtime environment.
    pub const fn unknown() -> Self {
        Self {
            has_window: false,
            navigator_user_agent: None,
            node_version: None,
            has_edge_runtime: false,
        }
    }

    /// Returns whether this environment maps to the upstream unknown runtime.
    pub fn is_unknown(&self) -> bool {
        !self.has_window
            && self.navigator_user_agent.is_none()
            && self.node_version.is_none()
            && !self.has_edge_runtime
    }

    /// Creates a browser runtime environment.
    pub const fn browser() -> Self {
        Self {
            has_window: true,
            navigator_user_agent: None,
            node_version: None,
            has_edge_runtime: false,
        }
    }

    /// Creates a navigator-backed runtime environment.
    pub fn navigator_user_agent(user_agent: impl Into<String>) -> Self {
        Self {
            has_window: false,
            navigator_user_agent: Some(user_agent.into()),
            node_version: None,
            has_edge_runtime: false,
        }
    }

    /// Creates a Node.js runtime environment.
    pub fn node_js(version: impl Into<String>) -> Self {
        Self {
            has_window: false,
            navigator_user_agent: None,
            node_version: Some(version.into()),
            has_edge_runtime: false,
        }
    }

    /// Creates a Vercel Edge runtime environment.
    pub const fn vercel_edge() -> Self {
        Self {
            has_window: false,
            navigator_user_agent: None,
            node_version: None,
            has_edge_runtime: true,
        }
    }
}

/// Runtime-independent fetch error information for request error normalization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchErrorInfo {
    /// JavaScript-style error name, when the HTTP layer exposes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,

    /// Human-readable error message.
    message: String,

    /// Runtime-specific network error code, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    code: Option<String>,

    /// Message from the wrapped error cause, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cause_message: Option<String>,
}

impl FetchErrorInfo {
    /// Creates fetch error information with a message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            name: None,
            message: message.into(),
            code: None,
            cause_message: None,
        }
    }

    /// Sets the JavaScript-style error name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the runtime-specific network error code.
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    /// Sets the wrapped cause message.
    pub fn with_cause_message(mut self, cause_message: impl Into<String>) -> Self {
        self.cause_message = Some(cause_message.into());
        self
    }

    /// Returns the JavaScript-style error name, when available.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns the runtime-specific network error code, when available.
    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    /// Returns the wrapped cause message, when available.
    pub fn cause_message(&self) -> Option<&str> {
        self.cause_message.as_deref()
    }
}

/// Result of normalizing a lower-level fetch error.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum HandledFetchError {
    /// The original error should be propagated unchanged.
    Original {
        /// Original fetch error information.
        error: FetchErrorInfo,
    },

    /// The fetch error should be surfaced as an API-call error.
    ApiCall {
        /// Normalized API-call error.
        error: Box<ApiCallError>,
    },
}

impl HandledFetchError {
    /// Returns the normalized API-call error when one was created.
    pub fn api_call_error(&self) -> Option<&ApiCallError> {
        match self {
            Self::Original { .. } => None,
            Self::ApiCall { error } => Some(error),
        }
    }

    /// Returns the original fetch error when it should be propagated unchanged.
    pub fn original_error(&self) -> Option<&FetchErrorInfo> {
        match self {
            Self::Original { error } => Some(error),
            Self::ApiCall { .. } => None,
        }
    }
}

/// Options for a dependency-free upstream-style `getFromApi` request.
///
/// Rust callers provide an injected transport to [`get_from_api`], so this
/// struct only carries the request metadata that upstream prepares before
/// calling `fetch`: URL, optional headers, the runtime used for the
/// provider-utils user-agent suffix, and the optional abort signal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetFromApiOptions {
    /// Provider API URL.
    pub url: String,

    /// Optional request headers. `None` values are removed during header
    /// normalization, matching upstream undefined header entries.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, Option<String>>,

    /// Runtime indicators used to append the provider-utils user-agent suffix.
    #[serde(default, skip_serializing_if = "RuntimeEnvironment::is_unknown")]
    pub environment: RuntimeEnvironment,

    /// Caller-controlled abort signal for the HTTP transport request.
    #[serde(default, skip)]
    pub abort_signal: Option<LanguageModelAbortSignal>,
}

impl GetFromApiOptions {
    /// Creates GET API request options for the given URL.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: BTreeMap::new(),
            environment: RuntimeEnvironment::unknown(),
            abort_signal: None,
        }
    }

    /// Adds or replaces request headers.
    pub fn with_headers<K, V, I>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (key, value) in headers {
            self.headers.insert(key.into(), value.map(Into::into));
        }

        self
    }

    /// Sets a request header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), Some(value.into()));
        self
    }

    /// Sets the runtime indicators used for request header preparation.
    pub fn with_environment(mut self, environment: RuntimeEnvironment) -> Self {
        self.environment = environment;
        self
    }

    /// Sets a caller-controlled abort signal for the HTTP transport request.
    pub fn with_abort_signal(mut self, abort_signal: LanguageModelAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }

    /// Sets an optional caller-controlled abort signal for the HTTP transport request.
    pub fn with_optional_abort_signal(
        mut self,
        abort_signal: Option<LanguageModelAbortSignal>,
    ) -> Self {
        self.abort_signal = abort_signal;
        self
    }

    /// Converts these options into the prepared provider API request.
    pub fn into_request(self) -> ProviderApiRequest {
        let Self {
            url,
            headers,
            environment,
            abort_signal,
        } = self;

        let request = prepare_get_from_api_request(url, Some(headers), &environment);

        match abort_signal {
            Some(abort_signal) => request.with_abort_signal(abort_signal),
            None => request,
        }
    }
}

/// Options for a dependency-free upstream-style `postJsonToApi` request.
///
/// Rust callers provide an injected transport to [`post_json_to_api`], so this
/// struct carries the request metadata that upstream prepares before calling
/// `fetch`: URL, optional headers, a JSON body, and the runtime used for the
/// provider-utils user-agent suffix.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostJsonToApiOptions {
    /// Provider API URL.
    pub url: String,

    /// Optional request headers. `None` values are removed during header
    /// normalization, matching upstream undefined header entries.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, Option<String>>,

    /// JSON request body. Upstream stringifies this value for the sent body and
    /// preserves the original value as `requestBodyValues`.
    pub body: JsonValue,

    /// Runtime indicators used to append the provider-utils user-agent suffix.
    #[serde(default, skip_serializing_if = "RuntimeEnvironment::is_unknown")]
    pub environment: RuntimeEnvironment,

    /// Caller-controlled abort signal for this provider API request.
    #[serde(default, skip)]
    pub abort_signal: Option<LanguageModelAbortSignal>,
}

impl PostJsonToApiOptions {
    /// Creates JSON POST API request options for the given URL and body.
    pub fn new(url: impl Into<String>, body: impl Into<JsonValue>) -> Self {
        Self {
            url: url.into(),
            headers: BTreeMap::new(),
            body: body.into(),
            environment: RuntimeEnvironment::unknown(),
            abort_signal: None,
        }
    }

    /// Adds or replaces request headers.
    pub fn with_headers<K, V, I>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (key, value) in headers {
            self.headers.insert(key.into(), value.map(Into::into));
        }

        self
    }

    /// Sets a request header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), Some(value.into()));
        self
    }

    /// Sets the JSON request body.
    pub fn with_body(mut self, body: impl Into<JsonValue>) -> Self {
        self.body = body.into();
        self
    }

    /// Sets the runtime indicators used for request header preparation.
    pub fn with_environment(mut self, environment: RuntimeEnvironment) -> Self {
        self.environment = environment;
        self
    }

    /// Sets a caller-controlled abort signal for the HTTP transport request.
    pub fn with_abort_signal(mut self, abort_signal: LanguageModelAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }

    /// Sets an optional caller-controlled abort signal for the HTTP transport request.
    pub fn with_optional_abort_signal(
        mut self,
        abort_signal: Option<LanguageModelAbortSignal>,
    ) -> Self {
        self.abort_signal = abort_signal;
        self
    }

    /// Converts these options into the prepared provider API request.
    pub fn into_request(self) -> ProviderApiRequest {
        let Self {
            url,
            headers,
            body,
            environment,
            abort_signal,
        } = self;

        let mut request = prepare_post_json_to_api_request(url, Some(headers), body, &environment);
        request.abort_signal = abort_signal;
        request
    }
}

/// Options for a dependency-free upstream-style `postFormDataToApi` request.
///
/// Rust callers provide an injected transport to [`post_form_data_to_api`], so
/// this struct carries the request metadata that upstream prepares before
/// calling `fetch`: URL, optional headers, form data, and the runtime used for
/// the provider-utils user-agent suffix.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostFormDataToApiOptions {
    /// Provider API URL.
    pub url: String,

    /// Optional request headers. `None` values are removed during header
    /// normalization, matching upstream undefined header entries.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, Option<String>>,

    /// Multipart form data request body.
    pub form_data: FormData,

    /// Runtime indicators used to append the provider-utils user-agent suffix.
    #[serde(default, skip_serializing_if = "RuntimeEnvironment::is_unknown")]
    pub environment: RuntimeEnvironment,

    /// Caller-controlled abort signal for this provider API request.
    #[serde(default, skip)]
    pub abort_signal: Option<LanguageModelAbortSignal>,
}

impl PostFormDataToApiOptions {
    /// Creates form-data POST API request options for the given URL and form data.
    pub fn new(url: impl Into<String>, form_data: FormData) -> Self {
        Self {
            url: url.into(),
            headers: BTreeMap::new(),
            form_data,
            environment: RuntimeEnvironment::unknown(),
            abort_signal: None,
        }
    }

    /// Adds or replaces request headers.
    pub fn with_headers<K, V, I>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (key, value) in headers {
            self.headers.insert(key.into(), value.map(Into::into));
        }

        self
    }

    /// Sets a request header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), Some(value.into()));
        self
    }

    /// Sets the form-data request body.
    pub fn with_form_data(mut self, form_data: FormData) -> Self {
        self.form_data = form_data;
        self
    }

    /// Sets the runtime indicators used for request header preparation.
    pub fn with_environment(mut self, environment: RuntimeEnvironment) -> Self {
        self.environment = environment;
        self
    }

    /// Sets a caller-controlled abort signal for the HTTP transport request.
    pub fn with_abort_signal(mut self, abort_signal: LanguageModelAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }

    /// Sets an optional caller-controlled abort signal for the HTTP transport request.
    pub fn with_optional_abort_signal(
        mut self,
        abort_signal: Option<LanguageModelAbortSignal>,
    ) -> Self {
        self.abort_signal = abort_signal;
        self
    }

    /// Converts these options into the prepared provider API request.
    pub fn into_request(self) -> ProviderApiRequest {
        let Self {
            url,
            headers,
            form_data,
            environment,
            abort_signal,
        } = self;

        let mut request =
            prepare_post_form_data_to_api_request(url, Some(headers), form_data, &environment);
        request.abort_signal = abort_signal;
        request
    }
}

/// Options for a dependency-free upstream-style `postToApi` request.
///
/// Rust callers provide an injected transport to [`post_to_api`], so this
/// struct carries the request metadata that upstream prepares before calling
/// `fetch`: URL, optional headers, text or binary body content, body values for
/// response handlers, and the runtime used for the provider-utils user-agent
/// suffix.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostToApiOptions {
    /// Provider API URL.
    pub url: String,

    /// Optional request headers. `None` values are removed during header
    /// normalization, matching upstream undefined header entries.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, Option<String>>,

    /// Request body content sent by the HTTP adapter.
    pub body: ProviderApiRequestBody,

    /// Values supplied to upstream response handlers as `requestBodyValues`.
    pub request_body_values: JsonValue,

    /// Runtime indicators used to append the provider-utils user-agent suffix.
    #[serde(default, skip_serializing_if = "RuntimeEnvironment::is_unknown")]
    pub environment: RuntimeEnvironment,

    /// Caller-controlled abort signal for this provider API request.
    #[serde(default, skip)]
    pub abort_signal: Option<LanguageModelAbortSignal>,
}

impl PostToApiOptions {
    /// Creates generic POST API request options for the given URL, body, and
    /// response-handler body values.
    pub fn new(
        url: impl Into<String>,
        body: ProviderApiRequestBody,
        request_body_values: impl Into<JsonValue>,
    ) -> Self {
        Self {
            url: url.into(),
            headers: BTreeMap::new(),
            body,
            request_body_values: request_body_values.into(),
            environment: RuntimeEnvironment::unknown(),
            abort_signal: None,
        }
    }

    /// Adds or replaces request headers.
    pub fn with_headers<K, V, I>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = (K, Option<V>)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (key, value) in headers {
            self.headers.insert(key.into(), value.map(Into::into));
        }

        self
    }

    /// Sets a request header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), Some(value.into()));
        self
    }

    /// Sets the request body content.
    pub fn with_body(mut self, body: ProviderApiRequestBody) -> Self {
        self.body = body;
        self
    }

    /// Sets the response-handler request body values.
    pub fn with_request_body_values(mut self, request_body_values: impl Into<JsonValue>) -> Self {
        self.request_body_values = request_body_values.into();
        self
    }

    /// Sets the runtime indicators used for request header preparation.
    pub fn with_environment(mut self, environment: RuntimeEnvironment) -> Self {
        self.environment = environment;
        self
    }

    /// Sets a caller-controlled abort signal for the HTTP transport request.
    pub fn with_abort_signal(mut self, abort_signal: LanguageModelAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }

    /// Sets an optional caller-controlled abort signal for the HTTP transport request.
    pub fn with_optional_abort_signal(
        mut self,
        abort_signal: Option<LanguageModelAbortSignal>,
    ) -> Self {
        self.abort_signal = abort_signal;
        self
    }

    /// Converts these options into the prepared provider API request.
    pub fn into_request(self) -> ProviderApiRequest {
        let Self {
            url,
            headers,
            body,
            request_body_values,
            environment,
            abort_signal,
        } = self;

        let mut request = prepare_post_to_api_request(
            url,
            Some(headers),
            body,
            request_body_values,
            &environment,
        );
        request.abort_signal = abort_signal;
        request
    }
}

/// HTTP method for provider API adapter requests.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ProviderApiRequestMethod {
    /// Upstream `getFromApi` request method.
    Get,

    /// Upstream `postToApi` request method.
    Post,
}

/// Body content sent by provider API adapter requests.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ProviderApiRequestBody {
    /// Text request body content.
    #[serde(rename = "text")]
    Text {
        /// Text body content.
        content: String,
    },

    /// Binary request body content.
    #[serde(rename = "bytes")]
    Bytes {
        /// Binary body content.
        content: Vec<u8>,
    },

    /// Multipart form data request body content.
    #[serde(rename = "form-data")]
    FormData {
        /// Ordered multipart form-data entries.
        content: FormData,
    },
}

impl ProviderApiRequestBody {
    /// Creates text request body content.
    pub fn text(content: impl Into<String>) -> Self {
        Self::Text {
            content: content.into(),
        }
    }

    /// Creates binary request body content.
    pub fn bytes(content: impl Into<Vec<u8>>) -> Self {
        Self::Bytes {
            content: content.into(),
        }
    }

    /// Creates form-data request body content.
    pub fn form_data(content: FormData) -> Self {
        Self::FormData { content }
    }

    /// Returns text request body content when this body is text.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { content } => Some(content),
            Self::Bytes { .. } | Self::FormData { .. } => None,
        }
    }

    /// Returns binary request body content when this body is bytes.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Text { .. } | Self::FormData { .. } => None,
            Self::Bytes { content } => Some(content),
        }
    }

    /// Returns form-data request body content when this body is form data.
    pub fn as_form_data(&self) -> Option<&FormData> {
        match self {
            Self::FormData { content } => Some(content),
            Self::Text { .. } | Self::Bytes { .. } => None,
        }
    }
}

/// Runtime-independent provider API request prepared for an HTTP adapter.
///
/// This is the Rust-native request boundary shared by upstream `getFromApi`
/// and `postToApi`: it preserves the fetch method, normalized headers,
/// optional request body content, and the values passed to response handlers as
/// `requestBodyValues`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderApiRequest {
    /// HTTP method used by the provider API request.
    pub method: ProviderApiRequestMethod,

    /// Provider API URL.
    pub url: String,

    /// Normalized request headers.
    #[serde(default)]
    pub headers: Headers,

    /// Request body content, when the request sends a body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<ProviderApiRequestBody>,

    /// Values supplied to upstream response handlers as `requestBodyValues`.
    pub request_body_values: JsonValue,

    /// Caller-controlled abort signal for transports that can cancel requests.
    #[serde(default, skip)]
    pub abort_signal: Option<LanguageModelAbortSignal>,
}

impl ProviderApiRequest {
    /// Creates a prepared provider API request.
    pub fn new(
        method: ProviderApiRequestMethod,
        url: impl Into<String>,
        headers: Headers,
        body: Option<ProviderApiRequestBody>,
        request_body_values: impl Into<JsonValue>,
    ) -> Self {
        Self {
            method,
            url: url.into(),
            headers,
            body,
            request_body_values: request_body_values.into(),
            abort_signal: None,
        }
    }

    /// Creates a prepared GET provider API request with empty request body values.
    pub fn get(url: impl Into<String>, headers: Headers) -> Self {
        Self::new(
            ProviderApiRequestMethod::Get,
            url,
            headers,
            None,
            JsonValue::Object(JsonObject::new()),
        )
    }

    /// Creates a prepared POST provider API request.
    pub fn post(
        url: impl Into<String>,
        headers: Headers,
        body: ProviderApiRequestBody,
        request_body_values: impl Into<JsonValue>,
    ) -> Self {
        Self::new(
            ProviderApiRequestMethod::Post,
            url,
            headers,
            Some(body),
            request_body_values,
        )
    }

    /// Adds an abort signal to the provider API request.
    pub fn with_abort_signal(mut self, abort_signal: LanguageModelAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }
}

/// Body content returned by provider API adapter responses.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ProviderApiResponseBody {
    /// Text response body content.
    #[serde(rename = "text")]
    Text {
        /// Text body content.
        content: String,
    },

    /// Binary response body content.
    #[serde(rename = "bytes")]
    Bytes {
        /// Binary body content.
        content: Vec<u8>,
    },
}

impl ProviderApiResponseBody {
    /// Creates text response body content.
    pub fn text(content: impl Into<String>) -> Self {
        Self::Text {
            content: content.into(),
        }
    }

    /// Creates binary response body content.
    pub fn bytes(content: impl Into<Vec<u8>>) -> Self {
        Self::Bytes {
            content: content.into(),
        }
    }

    /// Returns text response body content when this body is text.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { content } => Some(content),
            Self::Bytes { .. } => None,
        }
    }

    /// Returns binary response body content when this body is bytes.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Text { .. } => None,
            Self::Bytes { content } => Some(content),
        }
    }

    fn to_text(&self) -> String {
        match self {
            Self::Text { content } => content.clone(),
            Self::Bytes { content } => String::from_utf8_lossy(content).into_owned(),
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::Text { content } => content.as_bytes().to_vec(),
            Self::Bytes { content } => content.clone(),
        }
    }
}

/// Runtime-independent provider API response returned by an HTTP adapter.
///
/// This pairs with [`ProviderApiRequest`] as the dependency-free boundary for
/// upstream `getFromApi` and `postToApi`: HTTP adapters can supply status,
/// status text, extracted headers, and an already-read body without committing
/// this crate to a concrete HTTP client.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderApiResponse {
    /// HTTP response status code.
    pub status_code: u16,

    /// HTTP response status text.
    pub status_text: String,

    /// Headers extracted from the HTTP response.
    #[serde(default)]
    pub headers: Headers,

    /// Response body content, when one was available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<ProviderApiResponseBody>,
}

impl ProviderApiResponse {
    /// Creates a provider API response without a body.
    pub fn new(status_code: u16, status_text: impl Into<String>) -> Self {
        Self {
            status_code,
            status_text: status_text.into(),
            headers: Headers::new(),
            body: None,
        }
    }

    /// Creates a provider API response with a text body.
    pub fn text(status_code: u16, status_text: impl Into<String>, body: impl Into<String>) -> Self {
        Self::new(status_code, status_text).with_text_body(body)
    }

    /// Creates a provider API response with a binary body.
    pub fn bytes(
        status_code: u16,
        status_text: impl Into<String>,
        body: impl Into<Vec<u8>>,
    ) -> Self {
        Self::new(status_code, status_text).with_bytes_body(body)
    }

    /// Adds response headers extracted from the response.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = headers;
        self
    }

    /// Adds text response body content.
    pub fn with_text_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(ProviderApiResponseBody::text(body));
        self
    }

    /// Adds binary response body content.
    pub fn with_bytes_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(ProviderApiResponseBody::bytes(body));
        self
    }

    /// Returns whether the status maps to upstream `Response.ok`.
    pub fn is_success_status(&self) -> bool {
        (200..=299).contains(&self.status_code)
    }

    /// Returns text response body content when this response has text content.
    pub fn text_body(&self) -> Option<&str> {
        self.body
            .as_ref()
            .and_then(ProviderApiResponseBody::as_text)
    }

    /// Returns binary response body content when this response has binary content.
    pub fn bytes_body(&self) -> Option<&[u8]> {
        self.body
            .as_ref()
            .and_then(ProviderApiResponseBody::as_bytes)
    }

    /// Builds inputs for [`create_status_code_error_response_handler`].
    pub fn status_code_error_response_handler_options(
        &self,
        request: &ProviderApiRequest,
    ) -> StatusCodeErrorResponseHandlerOptions {
        StatusCodeErrorResponseHandlerOptions::new(
            request.url.clone(),
            request.request_body_values.clone(),
            self.status_code,
            self.status_text.clone(),
            self.body_as_text(),
        )
        .with_response_headers(self.headers.clone())
    }

    /// Builds inputs for [`create_json_error_response_handler`].
    pub fn json_error_response_handler_options(
        &self,
        request: &ProviderApiRequest,
    ) -> JsonErrorResponseHandlerOptions {
        JsonErrorResponseHandlerOptions::new(
            request.url.clone(),
            request.request_body_values.clone(),
            self.status_code,
            self.status_text.clone(),
            self.body_as_text(),
        )
        .with_response_headers(self.headers.clone())
    }

    /// Builds inputs for [`create_json_response_handler`].
    pub fn json_response_handler_options(
        &self,
        request: &ProviderApiRequest,
    ) -> JsonResponseHandlerOptions {
        JsonResponseHandlerOptions::new(
            request.url.clone(),
            request.request_body_values.clone(),
            self.status_code,
            self.body_as_text(),
        )
        .with_response_headers(self.headers.clone())
    }

    /// Builds inputs for [`create_binary_response_handler`].
    pub fn binary_response_handler_options(
        &self,
        request: &ProviderApiRequest,
    ) -> BinaryResponseHandlerOptions {
        let options = BinaryResponseHandlerOptions::empty(
            request.url.clone(),
            request.request_body_values.clone(),
            self.status_code,
        )
        .with_response_headers(self.headers.clone());

        if let Some(body) = self.body.as_ref().map(ProviderApiResponseBody::to_bytes) {
            BinaryResponseHandlerOptions {
                response_body: Some(body),
                ..options
            }
        } else {
            options
        }
    }

    /// Builds inputs for [`create_event_source_response_handler`].
    pub fn event_source_response_handler_options(&self) -> EventSourceResponseHandlerOptions {
        let options = if let Some(body) = self.body.as_ref().map(ProviderApiResponseBody::to_bytes)
        {
            EventSourceResponseHandlerOptions::new(body)
        } else {
            EventSourceResponseHandlerOptions::empty()
        };

        options.with_response_headers(self.headers.clone())
    }

    fn body_as_text(&self) -> String {
        self.body
            .as_ref()
            .map_or_else(String::new, ProviderApiResponseBody::to_text)
    }
}

/// Error returned by provider API response handlers.
///
/// Upstream `getFromApi` and `postToApi` pass through API-call errors from
/// response handlers but wrap other response-handler failures in a new
/// [`ApiCallError`]. This Rust boundary makes that distinction explicit without
/// depending on JavaScript `Error`/`AbortSignal` runtime mechanics.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum ProviderApiResponseHandlerError {
    /// The handler already produced an API-call error that should be propagated.
    ApiCall {
        /// API-call error returned by the response handler.
        error: Box<ApiCallError>,
    },

    /// The handler failed for another reason and should be wrapped.
    Other {
        /// Human-readable handler failure message.
        message: String,
    },
}

impl ProviderApiResponseHandlerError {
    /// Creates an API-call handler error.
    pub fn api_call(error: ApiCallError) -> Self {
        Self::ApiCall {
            error: Box::new(error),
        }
    }

    /// Creates an API-call handler error from a boxed API-call error.
    pub fn boxed_api_call(error: Box<ApiCallError>) -> Self {
        Self::ApiCall { error }
    }

    /// Creates a non-API-call handler error.
    pub fn other(message: impl Into<String>) -> Self {
        Self::Other {
            message: message.into(),
        }
    }

    /// Returns the API-call error when this failure should be propagated.
    pub fn api_call_error(&self) -> Option<&ApiCallError> {
        match self {
            Self::ApiCall { error } => Some(error),
            Self::Other { .. } => None,
        }
    }

    /// Returns the non-API-call handler failure message.
    pub fn other_message(&self) -> Option<&str> {
        match self {
            Self::ApiCall { .. } => None,
            Self::Other { message } => Some(message),
        }
    }
}

impl From<ApiCallError> for ProviderApiResponseHandlerError {
    fn from(error: ApiCallError) -> Self {
        Self::api_call(error)
    }
}

impl From<Box<ApiCallError>> for ProviderApiResponseHandlerError {
    fn from(error: Box<ApiCallError>) -> Self {
        Self::boxed_api_call(error)
    }
}

impl fmt::Display for ProviderApiResponseHandlerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiCall { error } => error.fmt(formatter),
            Self::Other { message } => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ProviderApiResponseHandlerError {}

/// Result returned by safe type validation.
#[derive(Clone, Debug, PartialEq)]
pub enum ValidateTypesResult<T = JsonValue> {
    /// Type validation succeeded.
    Success {
        /// Validated or transformed value.
        value: T,

        /// Raw JSON value before validation.
        raw_value: JsonValue,
    },

    /// Type validation failed without panicking.
    Failure {
        /// Wrapped type-validation error.
        error: TypeValidationError,

        /// Raw JSON value that failed validation.
        raw_value: JsonValue,
    },
}

impl<T> ValidateTypesResult<T> {
    /// Creates a successful type-validation result.
    pub fn success(value: T, raw_value: JsonValue) -> Self {
        Self::Success { value, raw_value }
    }

    /// Creates a failed type-validation result.
    pub fn failure(error: TypeValidationError, raw_value: JsonValue) -> Self {
        Self::Failure { error, raw_value }
    }

    /// Returns whether type validation succeeded.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Returns whether type validation failed.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failure { .. })
    }

    /// Returns the validated or transformed value on success.
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Success { value, .. } => Some(value),
            Self::Failure { .. } => None,
        }
    }

    /// Returns the raw JSON value before validation.
    pub fn raw_value(&self) -> &JsonValue {
        match self {
            Self::Success { raw_value, .. } | Self::Failure { raw_value, .. } => raw_value,
        }
    }

    /// Returns the type-validation error on failure.
    pub fn error(&self) -> Option<&TypeValidationError> {
        match self {
            Self::Success { .. } => None,
            Self::Failure { error, .. } => Some(error),
        }
    }
}

/// Error returned by safe JSON parsing.
#[derive(Clone, Debug, PartialEq)]
pub enum ParseJsonError {
    /// JSON text could not be parsed or failed secure JSON parsing.
    JsonParse(JsonParseError),

    /// Parsed JSON failed schema/type validation.
    TypeValidation(TypeValidationError),
}

impl ParseJsonError {
    /// Returns the JSON parse error when this is a parse failure.
    pub fn as_json_parse_error(&self) -> Option<&JsonParseError> {
        match self {
            Self::JsonParse(error) => Some(error),
            Self::TypeValidation(_) => None,
        }
    }

    /// Returns the type validation error when this is a validation failure.
    pub fn as_type_validation_error(&self) -> Option<&TypeValidationError> {
        match self {
            Self::JsonParse(_) => None,
            Self::TypeValidation(error) => Some(error),
        }
    }
}

impl From<JsonParseError> for ParseJsonError {
    fn from(error: JsonParseError) -> Self {
        Self::JsonParse(error)
    }
}

impl From<TypeValidationError> for ParseJsonError {
    fn from(error: TypeValidationError) -> Self {
        Self::TypeValidation(error)
    }
}

impl fmt::Display for ParseJsonError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::JsonParse(error) => error.fmt(formatter),
            Self::TypeValidation(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ParseJsonError {}

/// Result returned by safe JSON parsing.
#[derive(Clone, Debug, PartialEq)]
pub enum ParseJsonResult<T = JsonValue> {
    /// Parsing and optional validation succeeded.
    Success {
        /// Parsed or validated value.
        value: T,

        /// Raw JSON value before optional schema/type validation.
        raw_value: JsonValue,
    },

    /// Parsing or optional validation failed without panicking.
    Failure {
        /// Parse or validation error.
        error: ParseJsonError,

        /// Raw JSON value before validation, when parsing succeeded.
        raw_value: Option<JsonValue>,
    },
}

impl<T> ParseJsonResult<T> {
    /// Creates a successful parse result.
    pub fn success(value: T, raw_value: JsonValue) -> Self {
        Self::Success { value, raw_value }
    }

    /// Creates a failed parse result.
    pub fn failure(error: impl Into<ParseJsonError>, raw_value: Option<JsonValue>) -> Self {
        Self::Failure {
            error: error.into(),
            raw_value,
        }
    }

    /// Returns whether parsing and optional validation succeeded.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Returns whether parsing or optional validation failed.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failure { .. })
    }

    /// Returns the parsed or validated value on success.
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Success { value, .. } => Some(value),
            Self::Failure { .. } => None,
        }
    }

    /// Returns the raw parsed JSON value when one is available.
    pub fn raw_value(&self) -> Option<&JsonValue> {
        match self {
            Self::Success { raw_value, .. } => Some(raw_value),
            Self::Failure { raw_value, .. } => raw_value.as_ref(),
        }
    }

    /// Returns the parse or validation error on failure.
    pub fn error(&self) -> Option<&ParseJsonError> {
        match self {
            Self::Success { .. } => None,
            Self::Failure { error, .. } => Some(error),
        }
    }
}

/// Result returned by provider response handlers.
///
/// This mirrors upstream `@ai-sdk/provider-utils` response handlers: every
/// handler returns a parsed value and may include raw JSON data plus extracted
/// response headers.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseHandlerResult<T = JsonValue> {
    /// Parsed or constructed response value.
    pub value: T,

    /// Raw JSON value before optional validation, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_value: Option<JsonValue>,

    /// Headers extracted from the HTTP response, when available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_headers: Option<Headers>,
}

impl<T> ResponseHandlerResult<T> {
    /// Creates a response-handler result with a parsed value.
    pub fn new(value: T) -> Self {
        Self {
            value,
            raw_value: None,
            response_headers: None,
        }
    }

    /// Adds the raw JSON value before validation.
    pub fn with_raw_value(mut self, raw_value: impl Into<JsonValue>) -> Self {
        self.raw_value = Some(raw_value.into());
        self
    }

    /// Adds headers extracted from the response.
    pub fn with_response_headers(mut self, response_headers: Headers) -> Self {
        self.response_headers = Some(response_headers);
        self
    }

    /// Returns the parsed or constructed response value.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Returns the raw JSON value before validation, when available.
    pub fn raw_value(&self) -> Option<&JsonValue> {
        self.raw_value.as_ref()
    }

    /// Returns the extracted response headers, when available.
    pub fn response_headers(&self) -> Option<&Headers> {
        self.response_headers.as_ref()
    }

    /// Converts this result into the parsed or constructed response value.
    pub fn into_value(self) -> T {
        self.value
    }
}

/// Inputs for the status-code error response handler.
///
/// This is the Rust-native data boundary for upstream
/// `createStatusCodeErrorResponseHandler`, avoiding a concrete HTTP client
/// dependency while preserving the API-call error shape.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusCodeErrorResponseHandlerOptions {
    /// URL that produced the status-code error response.
    pub url: String,

    /// Request body values associated with the failed provider call.
    pub request_body_values: JsonValue,

    /// HTTP status code from the response.
    pub status_code: u16,

    /// HTTP status text from the response.
    pub status_text: String,

    /// Headers extracted from the response.
    #[serde(default)]
    pub response_headers: Headers,

    /// Raw response body text.
    pub response_body: String,
}

impl StatusCodeErrorResponseHandlerOptions {
    /// Creates status-code error response handler options.
    pub fn new(
        url: impl Into<String>,
        request_body_values: impl Into<JsonValue>,
        status_code: u16,
        status_text: impl Into<String>,
        response_body: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            request_body_values: request_body_values.into(),
            status_code,
            status_text: status_text.into(),
            response_headers: Headers::new(),
            response_body: response_body.into(),
        }
    }

    /// Adds response headers extracted from the response.
    pub fn with_response_headers(mut self, response_headers: Headers) -> Self {
        self.response_headers = response_headers;
        self
    }
}

/// Inputs for the JSON response handler.
///
/// This is the Rust-native data boundary for upstream
/// `createJsonResponseHandler`, keeping response parsing independent from any
/// concrete HTTP client while preserving API-call error context.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonResponseHandlerOptions {
    /// URL that produced the response.
    pub url: String,

    /// Request body values associated with the provider call.
    pub request_body_values: JsonValue,

    /// HTTP status code from the response.
    pub status_code: u16,

    /// Headers extracted from the HTTP response.
    #[serde(default)]
    pub response_headers: Headers,

    /// Raw response body text.
    pub response_body: String,
}

impl JsonResponseHandlerOptions {
    /// Creates JSON response handler options.
    pub fn new(
        url: impl Into<String>,
        request_body_values: impl Into<JsonValue>,
        status_code: u16,
        response_body: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            request_body_values: request_body_values.into(),
            status_code,
            response_headers: Headers::new(),
            response_body: response_body.into(),
        }
    }

    /// Adds response headers extracted from the response.
    pub fn with_response_headers(mut self, response_headers: Headers) -> Self {
        self.response_headers = response_headers;
        self
    }
}

/// Inputs for the JSON error response handler.
///
/// This is the Rust-native data boundary for upstream
/// `createJsonErrorResponseHandler`, preserving resilient JSON error parsing
/// without introducing a concrete HTTP client dependency.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonErrorResponseHandlerOptions {
    /// URL that produced the error response.
    pub url: String,

    /// Request body values associated with the failed provider call.
    pub request_body_values: JsonValue,

    /// HTTP status code from the response.
    pub status_code: u16,

    /// HTTP status text from the response.
    pub status_text: String,

    /// Headers extracted from the HTTP response.
    #[serde(default)]
    pub response_headers: Headers,

    /// Raw response body text.
    pub response_body: String,
}

impl JsonErrorResponseHandlerOptions {
    /// Creates JSON error response handler options.
    pub fn new(
        url: impl Into<String>,
        request_body_values: impl Into<JsonValue>,
        status_code: u16,
        status_text: impl Into<String>,
        response_body: impl Into<String>,
    ) -> Self {
        Self {
            url: url.into(),
            request_body_values: request_body_values.into(),
            status_code,
            status_text: status_text.into(),
            response_headers: Headers::new(),
            response_body: response_body.into(),
        }
    }

    /// Adds response headers extracted from the response.
    pub fn with_response_headers(mut self, response_headers: Headers) -> Self {
        self.response_headers = response_headers;
        self
    }
}

/// Inputs for the event-source response handler.
///
/// This is the Rust-native data boundary for upstream
/// `createEventSourceResponseHandler`, preserving response headers and a
/// byte-backed event stream without introducing a concrete HTTP client or async
/// stream dependency.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventSourceResponseHandlerOptions {
    /// Headers extracted from the HTTP response.
    #[serde(default)]
    pub response_headers: Headers,

    /// Raw event-source response body bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_body: Option<Vec<u8>>,
}

impl EventSourceResponseHandlerOptions {
    /// Creates event-source response handler options with a readable body.
    pub fn new(response_body: impl Into<Vec<u8>>) -> Self {
        Self {
            response_headers: Headers::new(),
            response_body: Some(response_body.into()),
        }
    }

    /// Creates event-source response handler options without a response body.
    pub fn empty() -> Self {
        Self {
            response_headers: Headers::new(),
            response_body: None,
        }
    }

    /// Adds response headers extracted from the response.
    pub fn with_response_headers(mut self, response_headers: Headers) -> Self {
        self.response_headers = response_headers;
        self
    }
}

/// Inputs for the binary response handler.
///
/// This is the Rust-native data boundary for upstream
/// `createBinaryResponseHandler`, keeping response body reading independent
/// from any concrete HTTP client while preserving API-call error context.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryResponseHandlerOptions {
    /// URL that produced the response.
    pub url: String,

    /// Request body values associated with the provider call.
    pub request_body_values: JsonValue,

    /// HTTP status code from the response.
    pub status_code: u16,

    /// Headers extracted from the HTTP response.
    #[serde(default)]
    pub response_headers: Headers,

    /// Raw binary response body bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_body: Option<Vec<u8>>,
}

impl BinaryResponseHandlerOptions {
    /// Creates binary response handler options with a readable response body.
    pub fn new(
        url: impl Into<String>,
        request_body_values: impl Into<JsonValue>,
        status_code: u16,
        response_body: impl Into<Vec<u8>>,
    ) -> Self {
        Self {
            url: url.into(),
            request_body_values: request_body_values.into(),
            status_code,
            response_headers: Headers::new(),
            response_body: Some(response_body.into()),
        }
    }

    /// Creates binary response handler options without a response body.
    pub fn empty(
        url: impl Into<String>,
        request_body_values: impl Into<JsonValue>,
        status_code: u16,
    ) -> Self {
        Self {
            url: url.into(),
            request_body_values: request_body_values.into(),
            status_code,
            response_headers: Headers::new(),
            response_body: None,
        }
    }

    /// Adds response headers extracted from the response.
    pub fn with_response_headers(mut self, response_headers: Headers) -> Self {
        self.response_headers = response_headers;
        self
    }
}

struct MediaTypeSignature {
    media_type: &'static str,
    bytes_prefix: &'static [Option<u8>],
}

const IMAGE_MEDIA_TYPE_SIGNATURES: &[MediaTypeSignature] = &[
    MediaTypeSignature {
        media_type: "image/gif",
        bytes_prefix: &[Some(0x47), Some(0x49), Some(0x46)],
    },
    MediaTypeSignature {
        media_type: "image/png",
        bytes_prefix: &[Some(0x89), Some(0x50), Some(0x4e), Some(0x47)],
    },
    MediaTypeSignature {
        media_type: "image/jpeg",
        bytes_prefix: &[Some(0xff), Some(0xd8)],
    },
    MediaTypeSignature {
        media_type: "image/webp",
        bytes_prefix: &[
            Some(0x52),
            Some(0x49),
            Some(0x46),
            Some(0x46),
            None,
            None,
            None,
            None,
            Some(0x57),
            Some(0x45),
            Some(0x42),
            Some(0x50),
        ],
    },
    MediaTypeSignature {
        media_type: "image/bmp",
        bytes_prefix: &[Some(0x42), Some(0x4d)],
    },
    MediaTypeSignature {
        media_type: "image/tiff",
        bytes_prefix: &[Some(0x49), Some(0x49), Some(0x2a), Some(0x00)],
    },
    MediaTypeSignature {
        media_type: "image/tiff",
        bytes_prefix: &[Some(0x4d), Some(0x4d), Some(0x00), Some(0x2a)],
    },
    MediaTypeSignature {
        media_type: "image/avif",
        bytes_prefix: &[
            Some(0x00),
            Some(0x00),
            Some(0x00),
            Some(0x20),
            Some(0x66),
            Some(0x74),
            Some(0x79),
            Some(0x70),
            Some(0x61),
            Some(0x76),
            Some(0x69),
            Some(0x66),
        ],
    },
    MediaTypeSignature {
        media_type: "image/heic",
        bytes_prefix: &[
            Some(0x00),
            Some(0x00),
            Some(0x00),
            Some(0x20),
            Some(0x66),
            Some(0x74),
            Some(0x79),
            Some(0x70),
            Some(0x68),
            Some(0x65),
            Some(0x69),
            Some(0x63),
        ],
    },
];

const DOCUMENT_MEDIA_TYPE_SIGNATURES: &[MediaTypeSignature] = &[MediaTypeSignature {
    media_type: "application/pdf",
    bytes_prefix: &[Some(0x25), Some(0x50), Some(0x44), Some(0x46)],
}];

const AUDIO_MEDIA_TYPE_SIGNATURES: &[MediaTypeSignature] = &[
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xfb)],
    },
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xfa)],
    },
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xf3)],
    },
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xf2)],
    },
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xe3)],
    },
    MediaTypeSignature {
        media_type: "audio/mpeg",
        bytes_prefix: &[Some(0xff), Some(0xe2)],
    },
    MediaTypeSignature {
        media_type: "audio/wav",
        bytes_prefix: &[
            Some(0x52),
            Some(0x49),
            Some(0x46),
            Some(0x46),
            None,
            None,
            None,
            None,
            Some(0x57),
            Some(0x41),
            Some(0x56),
            Some(0x45),
        ],
    },
    MediaTypeSignature {
        media_type: "audio/ogg",
        bytes_prefix: &[Some(0x4f), Some(0x67), Some(0x67), Some(0x53)],
    },
    MediaTypeSignature {
        media_type: "audio/flac",
        bytes_prefix: &[Some(0x66), Some(0x4c), Some(0x61), Some(0x43)],
    },
    MediaTypeSignature {
        media_type: "audio/aac",
        bytes_prefix: &[Some(0x40), Some(0x15), Some(0x00), Some(0x00)],
    },
    MediaTypeSignature {
        media_type: "audio/mp4",
        bytes_prefix: &[Some(0x66), Some(0x74), Some(0x79), Some(0x70)],
    },
    MediaTypeSignature {
        media_type: "audio/webm",
        bytes_prefix: &[Some(0x1a), Some(0x45), Some(0xdf), Some(0xa3)],
    },
];

const VIDEO_MEDIA_TYPE_SIGNATURES: &[MediaTypeSignature] = &[
    MediaTypeSignature {
        media_type: "video/mp4",
        bytes_prefix: &[
            Some(0x00),
            Some(0x00),
            Some(0x00),
            None,
            Some(0x66),
            Some(0x74),
            Some(0x79),
            Some(0x70),
        ],
    },
    MediaTypeSignature {
        media_type: "video/webm",
        bytes_prefix: &[Some(0x1a), Some(0x45), Some(0xdf), Some(0xa3)],
    },
    MediaTypeSignature {
        media_type: "video/quicktime",
        bytes_prefix: &[
            Some(0x00),
            Some(0x00),
            Some(0x00),
            Some(0x14),
            Some(0x66),
            Some(0x74),
            Some(0x79),
            Some(0x70),
            Some(0x71),
            Some(0x74),
        ],
    },
    MediaTypeSignature {
        media_type: "video/x-msvideo",
        bytes_prefix: &[Some(0x52), Some(0x49), Some(0x46), Some(0x46)],
    },
];

/// Future returned by a Rust tool execution function.
pub type ToolExecuteFuture =
    Pin<Box<dyn Future<Output = Result<JsonValue, ToolExecutionError>> + Send>>;

/// Function used to execute a Rust tool call.
pub type ToolExecuteFunction =
    dyn Fn(JsonValue, ToolExecutionOptions) -> ToolExecuteFuture + Send + Sync + 'static;

/// Future returned by a Rust tool execution function that emits stream records.
pub type ToolExecuteOutputsFuture =
    Pin<Box<dyn Future<Output = Result<Vec<ExecuteToolOutput>, ToolExecutionError>> + Send>>;

/// Function used to execute a Rust tool call and emit upstream-shaped records.
pub type ToolExecuteOutputsFunction =
    dyn Fn(JsonValue, ToolExecutionOptions) -> ToolExecuteOutputsFuture + Send + Sync + 'static;

/// Future returned by a tool input lifecycle callback.
pub type ToolInputCallbackFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Callback invoked when streaming tool input starts.
pub type ToolInputStartFunction =
    dyn Fn(ToolExecutionOptions) -> ToolInputCallbackFuture + Send + Sync + 'static;

/// Callback invoked when a streamed tool input delta arrives.
pub type ToolInputDeltaFunction =
    dyn Fn(ToolInputDeltaOptions) -> ToolInputCallbackFuture + Send + Sync + 'static;

/// Callback invoked when a complete tool input is available.
pub type ToolInputAvailableFunction =
    dyn Fn(ToolInputAvailableOptions) -> ToolInputCallbackFuture + Send + Sync + 'static;

/// Future returned by a sandbox command runner.
pub type SandboxRunCommandFuture = Pin<Box<dyn Future<Output = SandboxCommandResult> + Send>>;

/// Options passed to an experimental sandbox command runner.
///
/// This mirrors upstream `Experimental_Sandbox.runCommand` options, mapping the
/// JavaScript `AbortSignal` boundary to Rust's cloneable provider abort signal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxCommandOptions {
    /// Command to execute in the sandbox.
    pub command: String,

    /// Working directory used for the command, when supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,

    /// Caller-controlled abort signal for cancelling the sandbox command.
    #[serde(skip)]
    pub abort_signal: Option<LanguageModelAbortSignal>,
}

impl SandboxCommandOptions {
    /// Creates sandbox command options with the required command.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            working_directory: None,
            abort_signal: None,
        }
    }

    /// Sets the sandbox working directory for the command.
    pub fn with_working_directory(mut self, working_directory: impl Into<String>) -> Self {
        self.working_directory = Some(working_directory.into());
        self
    }

    /// Sets the abort signal used to cancel the sandbox command.
    pub fn with_abort_signal(mut self, abort_signal: LanguageModelAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }
}

/// Result returned by an experimental sandbox command runner.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxCommandResult {
    /// Exit code returned by the command.
    pub exit_code: i32,

    /// Standard output produced by the command.
    pub stdout: String,

    /// Standard error produced by the command.
    pub stderr: String,
}

impl SandboxCommandResult {
    /// Creates an empty sandbox command result for an exit code.
    pub fn new(exit_code: i32) -> Self {
        Self {
            exit_code,
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    /// Sets standard output for the command result.
    pub fn with_stdout(mut self, stdout: impl Into<String>) -> Self {
        self.stdout = stdout.into();
        self
    }

    /// Sets standard error for the command result.
    pub fn with_stderr(mut self, stderr: impl Into<String>) -> Self {
        self.stderr = stderr.into();
        self
    }
}

/// Experimental sandbox environment available to Rust tool executors.
///
/// Upstream exposes a description plus a `runCommand` callback. Rust keeps the
/// same runtime boundary through an object-safe trait so callers can provide
/// their own sandbox implementation without selecting a process runtime here.
pub trait ExperimentalSandbox: fmt::Debug + Send + Sync {
    /// Returns a human-readable sandbox description for model/tool instructions.
    fn description(&self) -> &str;

    /// Runs a command in the sandbox.
    fn run_command(&self, options: SandboxCommandOptions) -> SandboxRunCommandFuture;
}

/// Options passed to a tool execution function.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionOptions {
    /// Identifier of the model tool call being executed.
    pub tool_call_id: String,

    /// Prompt messages sent to the model for the step that produced the tool call.
    pub messages: LanguageModelPrompt,

    /// Tool-specific context configured for the executed tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<JsonValue>,

    /// Caller-controlled abort signal for cancelling the tool execution.
    #[serde(skip)]
    pub abort_signal: Option<LanguageModelAbortSignal>,

    /// Experimental sandbox environment available to the tool executor.
    #[serde(skip)]
    pub experimental_sandbox: Option<Arc<dyn ExperimentalSandbox>>,
}

impl ToolExecutionOptions {
    /// Creates tool execution options.
    pub fn new(tool_call_id: impl Into<String>, messages: LanguageModelPrompt) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            messages,
            context: None,
            abort_signal: None,
            experimental_sandbox: None,
        }
    }

    /// Sets the context for the executed tool.
    pub fn with_context(mut self, context: impl Into<JsonValue>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Sets the abort signal used to cancel this tool execution.
    pub fn with_abort_signal(mut self, abort_signal: LanguageModelAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }

    /// Sets the experimental sandbox available to this tool execution.
    pub fn with_experimental_sandbox(mut self, sandbox: Arc<dyn ExperimentalSandbox>) -> Self {
        self.experimental_sandbox = Some(sandbox);
        self
    }
}

impl PartialEq for ToolExecutionOptions {
    fn eq(&self, other: &Self) -> bool {
        self.tool_call_id == other.tool_call_id
            && self.messages == other.messages
            && self.context == other.context
            && self.abort_signal == other.abort_signal
            && match (&self.experimental_sandbox, &other.experimental_sandbox) {
                (None, None) => true,
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                _ => false,
            }
    }
}

/// Options passed to a streamed tool input delta callback.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInputDeltaOptions {
    /// Identifier of the model tool call whose input is streaming.
    pub tool_call_id: String,

    /// Prompt messages sent to the model for the step that produced the tool call.
    pub messages: LanguageModelPrompt,

    /// Streamed input text delta for this tool call.
    pub input_text_delta: String,

    /// Runtime context passed to the generation call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<JsonValue>,

    /// Caller-controlled abort signal for cancelling the overall operation.
    #[serde(skip)]
    pub abort_signal: Option<LanguageModelAbortSignal>,

    /// Experimental sandbox environment available to the tool callback.
    #[serde(skip)]
    pub experimental_sandbox: Option<Arc<dyn ExperimentalSandbox>>,
}

impl ToolInputDeltaOptions {
    /// Creates streamed tool input delta options.
    pub fn new(
        tool_call_id: impl Into<String>,
        messages: LanguageModelPrompt,
        input_text_delta: impl Into<String>,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            messages,
            input_text_delta: input_text_delta.into(),
            context: None,
            abort_signal: None,
            experimental_sandbox: None,
        }
    }

    /// Sets the runtime context for this callback.
    pub fn with_context(mut self, context: impl Into<JsonValue>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Sets the abort signal used to cancel the overall operation.
    pub fn with_abort_signal(mut self, abort_signal: LanguageModelAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }

    /// Sets the experimental sandbox available to this callback.
    pub fn with_experimental_sandbox(mut self, sandbox: Arc<dyn ExperimentalSandbox>) -> Self {
        self.experimental_sandbox = Some(sandbox);
        self
    }
}

/// Options passed when a complete streamed tool input is available.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInputAvailableOptions {
    /// Identifier of the model tool call whose input is available.
    pub tool_call_id: String,

    /// Prompt messages sent to the model for the step that produced the tool call.
    pub messages: LanguageModelPrompt,

    /// Parsed tool input.
    pub input: JsonValue,

    /// Runtime context passed to the generation call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<JsonValue>,

    /// Caller-controlled abort signal for cancelling the overall operation.
    #[serde(skip)]
    pub abort_signal: Option<LanguageModelAbortSignal>,

    /// Experimental sandbox environment available to the tool callback.
    #[serde(skip)]
    pub experimental_sandbox: Option<Arc<dyn ExperimentalSandbox>>,
}

impl ToolInputAvailableOptions {
    /// Creates complete tool input options.
    pub fn new(
        tool_call_id: impl Into<String>,
        messages: LanguageModelPrompt,
        input: JsonValue,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            messages,
            input,
            context: None,
            abort_signal: None,
            experimental_sandbox: None,
        }
    }

    /// Sets the runtime context for this callback.
    pub fn with_context(mut self, context: impl Into<JsonValue>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// Sets the abort signal used to cancel the overall operation.
    pub fn with_abort_signal(mut self, abort_signal: LanguageModelAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }

    /// Sets the experimental sandbox available to this callback.
    pub fn with_experimental_sandbox(mut self, sandbox: Arc<dyn ExperimentalSandbox>) -> Self {
        self.experimental_sandbox = Some(sandbox);
        self
    }
}

/// Error returned by a Rust tool execution function.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionError {
    /// Human-readable execution failure message.
    pub message: String,
}

impl ToolExecutionError {
    /// Creates a tool execution error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the execution failure message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its message.
    pub fn into_message(self) -> String {
        self.message
    }
}

impl fmt::Display for ToolExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ToolExecutionError {}

impl From<String> for ToolExecutionError {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl From<&str> for ToolExecutionError {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

/// Output yielded by [`execute_tool`].
///
/// Upstream provider-utils `executeTool` is an async generator that emits
/// preliminary outputs for streaming executors and a final output when
/// execution completes. Rust preserves that tagged contract directly for
/// streaming output executors and wraps single-value executors in one final
/// record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum ExecuteToolOutput {
    /// Preliminary output from a streaming tool executor.
    Preliminary {
        /// JSON-serializable preliminary tool output.
        output: JsonValue,
    },

    /// Final output from a tool executor.
    Final {
        /// JSON-serializable final tool output.
        output: JsonValue,
    },
}

impl ExecuteToolOutput {
    /// Creates a preliminary tool output.
    pub fn preliminary(output: JsonValue) -> Self {
        Self::Preliminary { output }
    }

    /// Creates a final tool output.
    pub fn final_output(output: JsonValue) -> Self {
        Self::Final { output }
    }

    /// Returns the JSON output payload.
    pub fn output(&self) -> &JsonValue {
        match self {
            Self::Preliminary { output } | Self::Final { output } => output,
        }
    }
}

/// Typed tool call returned by high-level text generation APIs.
///
/// This mirrors upstream provider-utils `ToolCall` while using [`JsonValue`] for
/// the generic input payload at the Rust JSON boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
    /// ID of the tool call used to match it with the tool result.
    pub tool_call_id: String,

    /// Name of the tool that is being called.
    pub tool_name: String,

    /// JSON-serializable input arguments for the tool.
    pub input: JsonValue,

    /// Whether the tool call will be executed by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,

    /// Whether the tool is dynamic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,
}

impl ToolCall {
    /// Creates a typed high-level tool call.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: JsonValue,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
            provider_executed: None,
            dynamic: None,
        }
    }

    /// Sets whether the provider will execute this tool call.
    pub fn with_provider_executed(mut self, provider_executed: bool) -> Self {
        self.provider_executed = Some(provider_executed);
        self
    }

    /// Sets whether this tool call is dynamic.
    pub fn with_dynamic(mut self, dynamic: bool) -> Self {
        self.dynamic = Some(dynamic);
        self
    }
}

/// Typed tool result returned by high-level text generation APIs.
///
/// This mirrors upstream provider-utils `ToolResult` while using [`JsonValue`]
/// for the generic input and output payloads at the Rust JSON boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    /// ID of the tool call used to match it with the tool result.
    pub tool_call_id: String,

    /// Name of the tool that was called.
    pub tool_name: String,

    /// JSON-serializable input arguments for the tool.
    pub input: JsonValue,

    /// JSON-serializable output returned by the tool.
    pub output: JsonValue,

    /// Whether the tool result was executed by the provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,

    /// Whether the tool is dynamic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<bool>,
}

impl ToolResult {
    /// Creates a typed high-level tool result.
    pub fn new(
        tool_call_id: impl Into<String>,
        tool_name: impl Into<String>,
        input: JsonValue,
        output: JsonValue,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            tool_name: tool_name.into(),
            input,
            output,
            provider_executed: None,
            dynamic: None,
        }
    }

    /// Sets whether the provider executed this tool result.
    pub fn with_provider_executed(mut self, provider_executed: bool) -> Self {
        self.provider_executed = Some(provider_executed);
        self
    }

    /// Sets whether this tool result is for a dynamic tool.
    pub fn with_dynamic(mut self, dynamic: bool) -> Self {
        self.dynamic = Some(dynamic);
        self
    }
}

/// File data accepted by provider-utils `FilePart`.
///
/// Upstream accepts either a tagged file-data union or bare `DataContent`,
/// `URL`, and provider-reference shorthand values.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum FilePartData {
    /// Tagged file data.
    Tagged(FileData),

    /// Bare raw-byte/base64 shorthand.
    Data(FileDataContent),

    /// Bare URL shorthand.
    Url(Url),

    /// Bare provider-reference shorthand.
    Reference(ProviderReference),
}

impl From<FileData> for FilePartData {
    fn from(data: FileData) -> Self {
        Self::Tagged(data)
    }
}

impl From<FileDataContent> for FilePartData {
    fn from(data: FileDataContent) -> Self {
        Self::Data(data)
    }
}

impl From<Vec<u8>> for FilePartData {
    fn from(data: Vec<u8>) -> Self {
        Self::Data(FileDataContent::Bytes(data))
    }
}

impl From<&[u8]> for FilePartData {
    fn from(data: &[u8]) -> Self {
        Self::Data(FileDataContent::Bytes(data.to_vec()))
    }
}

impl From<String> for FilePartData {
    fn from(data: String) -> Self {
        Self::Data(FileDataContent::Base64(data))
    }
}

impl From<&str> for FilePartData {
    fn from(data: &str) -> Self {
        Self::Data(FileDataContent::Base64(data.to_string()))
    }
}

impl From<Url> for FilePartData {
    fn from(url: Url) -> Self {
        Self::Url(url)
    }
}

impl From<ProviderReference> for FilePartData {
    fn from(reference: ProviderReference) -> Self {
        Self::Reference(reference)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum FilePartKind {
    #[serde(rename = "file")]
    File,
}

/// File content part owned by provider-utils model-message types.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePart {
    #[serde(rename = "type")]
    kind: FilePartKind,

    /// File data as a tagged union or accepted shorthand.
    pub data: FilePartData,

    /// Optional filename of the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,

    /// Full IANA media type or top-level IANA media segment.
    pub media_type: String,

    /// Provider-specific options for this content part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl FilePart {
    /// Creates a provider-utils file content part.
    pub fn new(data: impl Into<FilePartData>, media_type: impl Into<String>) -> Self {
        Self {
            kind: FilePartKind::File,
            data: data.into(),
            filename: None,
            media_type: media_type.into(),
            provider_options: None,
        }
    }

    /// Sets the file name.
    pub fn with_filename(mut self, filename: impl Into<String>) -> Self {
        self.filename = Some(filename.into());
        self
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

/// Reasoning-file data accepted by provider-utils `ReasoningFilePart`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ReasoningFilePartData {
    /// Tagged reasoning file data.
    Tagged(LanguageModelFileData),

    /// Bare raw-byte/base64 shorthand.
    Data(FileDataContent),

    /// Bare URL shorthand.
    Url(Url),
}

impl From<LanguageModelFileData> for ReasoningFilePartData {
    fn from(data: LanguageModelFileData) -> Self {
        Self::Tagged(data)
    }
}

impl From<FileDataContent> for ReasoningFilePartData {
    fn from(data: FileDataContent) -> Self {
        Self::Data(data)
    }
}

impl From<Vec<u8>> for ReasoningFilePartData {
    fn from(data: Vec<u8>) -> Self {
        Self::Data(FileDataContent::Bytes(data))
    }
}

impl From<&[u8]> for ReasoningFilePartData {
    fn from(data: &[u8]) -> Self {
        Self::Data(FileDataContent::Bytes(data.to_vec()))
    }
}

impl From<String> for ReasoningFilePartData {
    fn from(data: String) -> Self {
        Self::Data(FileDataContent::Base64(data))
    }
}

impl From<&str> for ReasoningFilePartData {
    fn from(data: &str) -> Self {
        Self::Data(FileDataContent::Base64(data.to_string()))
    }
}

impl From<Url> for ReasoningFilePartData {
    fn from(url: Url) -> Self {
        Self::Url(url)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum ReasoningFilePartKind {
    #[serde(rename = "reasoning-file")]
    ReasoningFile,
}

/// Reasoning-file content part owned by provider-utils model-message types.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReasoningFilePart {
    #[serde(rename = "type")]
    kind: ReasoningFilePartKind,

    /// Reasoning file data as a tagged union or accepted shorthand.
    pub data: ReasoningFilePartData,

    /// Full IANA media type of the file.
    pub media_type: String,

    /// Provider-specific options for this content part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_options: Option<ProviderOptions>,
}

impl ReasoningFilePart {
    /// Creates a provider-utils reasoning-file content part.
    pub fn new(data: impl Into<ReasoningFilePartData>, media_type: impl Into<String>) -> Self {
        Self {
            kind: ReasoningFilePartKind::ReasoningFile,
            data: data.into(),
            media_type: media_type.into(),
            provider_options: None,
        }
    }

    /// Adds provider-specific options.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }
}

/// Provider reference or a single provider file id used by legacy content parts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ProviderReferenceOrString {
    /// A single provider file id.
    String(String),

    /// A provider-to-file-id reference map.
    Reference(ProviderReference),
}

impl From<String> for ProviderReferenceOrString {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for ProviderReferenceOrString {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<ProviderReference> for ProviderReferenceOrString {
    fn from(reference: ProviderReference) -> Self {
        Self::Reference(reference)
    }
}

/// Content item inside a provider-utils tool-result output.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ToolResultContentPart {
    /// Text content.
    Text {
        /// Text content.
        text: String,

        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// File content using the current tagged file-data shape.
    File {
        /// Tagged file data. Bare shorthand values are intentionally not
        /// accepted for tool-result file parts.
        data: FileData,

        /// Full IANA media type or top-level IANA media segment.
        media_type: String,

        /// Optional filename of the file.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,

        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// Deprecated base64 file-data content.
    FileData {
        /// Base64-encoded file data.
        data: String,

        /// Full IANA media type.
        media_type: String,

        /// Optional filename of the file.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,

        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// Deprecated file URL content.
    FileUrl {
        /// URL of the file.
        url: String,

        /// Optional full IANA media type.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        media_type: Option<String>,

        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// Deprecated provider file-id content.
    FileId {
        /// Provider file id or provider-reference map.
        file_id: ProviderReferenceOrString,

        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// Deprecated provider file-reference content.
    FileReference {
        /// Provider-reference map.
        provider_reference: ProviderReference,

        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// Deprecated base64 image-data content.
    ImageData {
        /// Base64-encoded image data.
        data: String,

        /// Full IANA media type.
        media_type: String,

        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// Deprecated image URL content.
    ImageUrl {
        /// URL of the image.
        url: String,

        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// Deprecated provider image file-id content.
    ImageFileId {
        /// Provider file id or provider-reference map.
        file_id: ProviderReferenceOrString,

        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// Deprecated provider image file-reference content.
    ImageFileReference {
        /// Provider-reference map.
        provider_reference: ProviderReference,

        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// Provider-specific content part.
    Custom {
        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
}

/// Output returned from provider-utils tool execution conversion hooks.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ToolResultOutput {
    /// Text tool output.
    Text {
        /// Text output value.
        value: String,

        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// JSON tool output.
    Json {
        /// JSON output value.
        value: JsonValue,

        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// Execution denied by the user.
    ExecutionDenied {
        /// Optional denial reason.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,

        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// Text error output.
    ErrorText {
        /// Text error value.
        value: String,

        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// JSON error output.
    ErrorJson {
        /// JSON error value.
        value: JsonValue,

        /// Provider-specific options.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },

    /// Multi-part tool output.
    Content {
        /// Content output parts.
        value: Vec<ToolResultContentPart>,
    },
}

/// Options passed when resolving a runtime-dependent tool description.
#[derive(Clone, Debug)]
pub struct ToolDescriptionOptions {
    /// Tool-specific context for the current generation call, when supplied.
    pub context: Option<JsonValue>,

    /// Experimental sandbox available while preparing tool definitions.
    pub experimental_sandbox: Option<Arc<dyn ExperimentalSandbox>>,
}

impl ToolDescriptionOptions {
    /// Creates description-resolution options.
    pub fn new(context: Option<JsonValue>) -> Self {
        Self {
            context,
            experimental_sandbox: None,
        }
    }

    /// Adds an experimental sandbox to the description-resolution context.
    pub fn with_experimental_sandbox(
        mut self,
        experimental_sandbox: Arc<dyn ExperimentalSandbox>,
    ) -> Self {
        self.experimental_sandbox = Some(experimental_sandbox);
        self
    }
}

/// Runtime-dependent tool description callback.
pub type ToolDescriptionFunction = dyn Fn(ToolDescriptionOptions) -> String + Send + Sync;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum ToolApprovalRequestKind {
    #[serde(rename = "tool-approval-request")]
    ToolApprovalRequest,
}

/// Tool approval request prompt part.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalRequest {
    #[serde(rename = "type")]
    kind: ToolApprovalRequestKind,

    /// ID of the tool approval.
    pub approval_id: String,

    /// ID of the tool call that the approval request is for.
    pub tool_call_id: String,

    /// Whether the tool was automatically approved or denied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_automatic: Option<bool>,
}

impl ToolApprovalRequest {
    /// Creates a tool approval request prompt part.
    pub fn new(approval_id: impl Into<String>, tool_call_id: impl Into<String>) -> Self {
        Self {
            kind: ToolApprovalRequestKind::ToolApprovalRequest,
            approval_id: approval_id.into(),
            tool_call_id: tool_call_id.into(),
            is_automatic: None,
        }
    }

    /// Sets whether this approval request was resolved automatically.
    pub fn with_automatic(mut self, is_automatic: bool) -> Self {
        self.is_automatic = Some(is_automatic);
        self
    }

    /// Converts this high-level prompt part into the provider-v4 prompt shape.
    pub fn to_language_model_part(&self) -> LanguageModelToolApprovalRequestPart {
        let mut part = LanguageModelToolApprovalRequestPart::new(
            self.approval_id.clone(),
            self.tool_call_id.clone(),
        );

        if let Some(is_automatic) = self.is_automatic {
            part = part.with_automatic(is_automatic);
        }

        part
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
enum ToolApprovalResponseKind {
    #[serde(rename = "tool-approval-response")]
    ToolApprovalResponse,
}

/// Tool approval response prompt part.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolApprovalResponse {
    #[serde(rename = "type")]
    kind: ToolApprovalResponseKind,

    /// ID of the tool approval.
    pub approval_id: String,

    /// Whether the approval was granted.
    pub approved: bool,

    /// Optional reason for the approval or denial.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    /// Whether the approved or denied tool call is provider-executed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_executed: Option<bool>,
}

impl ToolApprovalResponse {
    /// Creates a tool approval response prompt part.
    pub fn new(approval_id: impl Into<String>, approved: bool) -> Self {
        Self {
            kind: ToolApprovalResponseKind::ToolApprovalResponse,
            approval_id: approval_id.into(),
            approved,
            reason: None,
            provider_executed: None,
        }
    }

    /// Adds an approval or denial reason.
    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Sets whether the tool call is provider-executed.
    pub fn with_provider_executed(mut self, provider_executed: bool) -> Self {
        self.provider_executed = Some(provider_executed);
        self
    }

    /// Converts this high-level prompt part into the provider-v4 prompt shape.
    ///
    /// The provider-v4 prompt response does not include the high-level
    /// `providerExecuted` flag; callers can inspect it before conversion when
    /// deciding whether to send the response to the model.
    pub fn to_language_model_part(&self) -> LanguageModelToolApprovalResponsePart {
        let mut part =
            LanguageModelToolApprovalResponsePart::new(self.approval_id.clone(), self.approved);

        if let Some(reason) = &self.reason {
            part = part.with_reason(reason.clone());
        }

        part
    }
}

/// Options passed when converting a tool result into model-facing output.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolModelOutputOptions {
    /// Identifier of the model tool call whose result is being converted.
    pub tool_call_id: String,

    /// Tool input that produced the output.
    pub input: JsonValue,

    /// Raw tool output returned by the executor or high-level message.
    pub output: JsonValue,
}

impl ToolModelOutputOptions {
    /// Creates model-output conversion options.
    pub fn new(tool_call_id: impl Into<String>, input: JsonValue, output: JsonValue) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            input,
            output,
        }
    }
}

/// Future returned by a tool model-output conversion callback.
pub type ToolModelOutputFuture =
    Pin<Box<dyn Future<Output = LanguageModelToolResultOutput> + Send>>;

/// Runtime callback that converts raw tool output to model-facing output.
pub type ToolModelOutputFunction =
    dyn Fn(ToolModelOutputOptions) -> ToolModelOutputFuture + Send + Sync;

/// Future returned by a tool-defined approval callback.
pub type ToolNeedsApprovalFuture = Pin<Box<dyn Future<Output = bool> + Send>>;

/// Options passed to a tool-defined approval callback.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolNeedsApprovalOptions {
    /// Identifier of the model tool call whose execution might need approval.
    pub tool_call_id: String,

    /// Prompt messages sent to the model for the step that produced the tool call.
    pub messages: LanguageModelPrompt,

    /// Tool-specific context configured for the called tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<JsonValue>,
}

impl ToolNeedsApprovalOptions {
    /// Creates tool-defined approval callback options.
    pub fn new(tool_call_id: impl Into<String>, messages: LanguageModelPrompt) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            messages,
            context: None,
        }
    }

    /// Sets tool-specific context for the approval callback.
    pub fn with_context(mut self, context: impl Into<JsonValue>) -> Self {
        self.context = Some(context.into());
        self
    }
}

/// Function that determines whether a tool call needs approval.
pub type ToolNeedsApprovalFunction =
    dyn Fn(JsonValue, ToolNeedsApprovalOptions) -> ToolNeedsApprovalFuture + Send + Sync;

#[derive(Clone, Debug, Eq, PartialEq)]
enum ToolKind {
    Function,
    Dynamic,
    Provider {
        id: String,
        args: JsonObject,
        provider_executed: bool,
        output_schema: Option<JsonSchema>,
        supports_deferred_results: Option<bool>,
    },
}

/// Factory for creating provider-defined tools from shared provider metadata.
///
/// This mirrors upstream `createProviderDefinedToolFactory`: the factory owns
/// the provider tool id and schemas, while Rust callers supply the model-call
/// tool name explicitly because there is no JavaScript object key to infer it
/// from.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDefinedToolFactory {
    /// Provider tool identifier, typically `<provider-id>.<unique-tool-name>`.
    pub id: String,

    /// JSON Schema 7 object describing the provider tool input.
    pub input_schema: JsonSchema,

    /// Optional JSON Schema 7 object describing the provider tool output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<JsonSchema>,
}

impl ProviderDefinedToolFactory {
    /// Creates a provider-defined tool factory.
    pub fn new(id: impl Into<String>, input_schema: JsonSchema) -> Self {
        Self {
            id: id.into(),
            input_schema,
            output_schema: None,
        }
    }

    /// Sets the expected output schema shared by tools created from this factory.
    pub fn with_output_schema(mut self, output_schema: JsonSchema) -> Self {
        self.output_schema = Some(output_schema);
        self
    }

    /// Creates a provider-defined tool from this factory.
    pub fn tool(&self, name: impl Into<String>, args: JsonObject) -> Tool {
        let mut tool =
            Tool::provider_defined(name, self.id.clone(), args, self.input_schema.clone());

        if let Some(output_schema) = &self.output_schema {
            tool = tool.with_output_schema(output_schema.clone());
        }

        tool
    }
}

/// Factory for creating provider-executed tools from shared provider metadata.
///
/// This mirrors upstream `createProviderExecutedToolFactory` while keeping the
/// runtime-independent Rust tool boundary free of JavaScript callback-only
/// streaming hooks.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderExecutedToolFactory {
    /// Provider tool identifier, typically `<provider-id>.<unique-tool-name>`.
    pub id: String,

    /// JSON Schema 7 object describing the provider tool input.
    pub input_schema: JsonSchema,

    /// JSON Schema 7 object describing the provider tool output.
    pub output_schema: JsonSchema,

    /// Whether this provider-executed tool supports deferred results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_deferred_results: Option<bool>,
}

impl ProviderExecutedToolFactory {
    /// Creates a provider-executed tool factory.
    pub fn new(id: impl Into<String>, input_schema: JsonSchema, output_schema: JsonSchema) -> Self {
        Self {
            id: id.into(),
            input_schema,
            output_schema,
            supports_deferred_results: None,
        }
    }

    /// Sets whether created provider-executed tools support deferred results.
    pub fn with_supports_deferred_results(mut self, supports_deferred_results: bool) -> Self {
        self.supports_deferred_results = Some(supports_deferred_results);
        self
    }

    /// Creates a provider-executed tool from this factory.
    pub fn tool(&self, name: impl Into<String>, args: JsonObject) -> Tool {
        let mut tool = Tool::provider_executed(
            name,
            self.id.clone(),
            args,
            self.input_schema.clone(),
            self.output_schema.clone(),
        );

        if let Some(supports_deferred_results) = self.supports_deferred_results {
            tool = tool.with_supports_deferred_results(supports_deferred_results);
        }

        tool
    }
}

/// User-defined Rust, dynamic runtime, or provider-defined tool made available to a language model call.
///
/// This mirrors the function and dynamic branches of upstream
/// `@ai-sdk/provider-utils` `Tool`, plus provider tools whose model-facing
/// schema is owned by the provider. Function-style tools carry model-facing
/// schema/description metadata and may include an executor for later
/// client-side tool handling.
#[derive(Clone)]
pub struct Tool {
    kind: ToolKind,

    /// Name of the tool, unique within a model call.
    pub name: String,

    /// Optional display title for the tool.
    ///
    /// This mirrors upstream's deprecated `title` field: it is not sent to the
    /// model, but can be surfaced on high-level tool calls.
    pub title: Option<String>,

    /// Optional description of what the tool does.
    pub description: Option<String>,

    description_resolver: Option<Arc<ToolDescriptionFunction>>,

    /// JSON Schema 7 object describing the tool input.
    pub input_schema: JsonSchema,

    /// Optional JSON Schema 7 object describing the tool output.
    ///
    /// Function and dynamic tools keep this as high-level SDK metadata for
    /// local execution validation. Provider-facing function tool shapes do not
    /// serialize it today.
    pub output_schema: Option<JsonSchema>,

    /// Optional schema describing the tool-specific execution context.
    ///
    /// This context is not sent to the provider. It validates and normalizes
    /// the matching `toolsContext[toolName]` value before Rust tool execution.
    pub context_schema: Option<FlexibleSchema<JsonValue>>,

    /// Optional examples that show the model what inputs should look like.
    pub input_examples: Option<Vec<LanguageModelToolInputExample>>,

    /// Strict mode setting for providers that support it.
    pub strict: Option<bool>,

    /// Provider-specific options sent with the tool definition.
    pub provider_options: Option<ProviderOptions>,

    /// Tool metadata propagated to generated tool calls and results.
    ///
    /// Unlike provider options, this metadata is not sent to the language
    /// model. It is high-level SDK state for consumers that need to identify a
    /// tool source such as an MCP server.
    pub metadata: Option<JsonObject>,

    /// Whether this tool requires approval before execution.
    ///
    /// This mirrors upstream's deprecated tool-defined `needsApproval` boolean.
    /// Generate-text-level approval configuration can still override it.
    pub needs_approval: Option<bool>,

    needs_approval_resolver: Option<Arc<ToolNeedsApprovalFunction>>,

    execute: Option<Arc<ToolExecuteFunction>>,
    execute_outputs: Option<Arc<ToolExecuteOutputsFunction>>,
    on_input_start: Option<Arc<ToolInputStartFunction>>,
    on_input_delta: Option<Arc<ToolInputDeltaFunction>>,
    on_input_available: Option<Arc<ToolInputAvailableFunction>>,
    to_model_output: Option<Arc<ToolModelOutputFunction>>,
}

impl Tool {
    /// Creates a function tool definition.
    pub fn new(name: impl Into<String>, input_schema: JsonSchema) -> Self {
        Self {
            kind: ToolKind::Function,
            name: name.into(),
            title: None,
            description: None,
            description_resolver: None,
            input_schema,
            output_schema: None,
            context_schema: None,
            input_examples: None,
            strict: None,
            provider_options: None,
            metadata: None,
            needs_approval: None,
            needs_approval_resolver: None,
            execute: None,
            execute_outputs: None,
            on_input_start: None,
            on_input_delta: None,
            on_input_available: None,
            to_model_output: None,
        }
    }

    /// Creates a dynamic function tool definition.
    ///
    /// Upstream dynamic tools are defined at runtime, but cross the provider-v4
    /// boundary as ordinary function tools. The dynamic flag remains high-level
    /// metadata used when interpreting tool calls and results.
    pub fn dynamic(name: impl Into<String>, input_schema: JsonSchema) -> Self {
        Self {
            kind: ToolKind::Dynamic,
            name: name.into(),
            title: None,
            description: None,
            description_resolver: None,
            input_schema,
            output_schema: None,
            context_schema: None,
            input_examples: None,
            strict: None,
            provider_options: None,
            metadata: None,
            needs_approval: None,
            needs_approval_resolver: None,
            execute: None,
            execute_outputs: None,
            on_input_start: None,
            on_input_delta: None,
            on_input_available: None,
            to_model_output: None,
        }
    }

    /// Creates a provider-defined tool that is executed by the caller.
    ///
    /// This is the Rust-native equivalent of upstream provider-defined tool
    /// factories: `id` identifies the provider tool, `args` configures it, and
    /// `name` is the caller-facing tool name used in this model call.
    pub fn provider_defined(
        name: impl Into<String>,
        id: impl Into<String>,
        args: JsonObject,
        input_schema: JsonSchema,
    ) -> Self {
        Self {
            kind: ToolKind::Provider {
                id: id.into(),
                args,
                provider_executed: false,
                output_schema: None,
                supports_deferred_results: None,
            },
            name: name.into(),
            title: None,
            description: None,
            description_resolver: None,
            input_schema,
            output_schema: None,
            context_schema: None,
            input_examples: None,
            strict: None,
            provider_options: None,
            metadata: None,
            needs_approval: None,
            needs_approval_resolver: None,
            execute: None,
            execute_outputs: None,
            on_input_start: None,
            on_input_delta: None,
            on_input_available: None,
            to_model_output: None,
        }
    }

    /// Creates a provider-executed tool.
    ///
    /// Provider-executed tools are sent to the model as provider tools and do
    /// not require a Rust executor because the provider returns tool results.
    pub fn provider_executed(
        name: impl Into<String>,
        id: impl Into<String>,
        args: JsonObject,
        input_schema: JsonSchema,
        output_schema: JsonSchema,
    ) -> Self {
        Self {
            kind: ToolKind::Provider {
                id: id.into(),
                args,
                provider_executed: true,
                output_schema: Some(output_schema),
                supports_deferred_results: None,
            },
            name: name.into(),
            title: None,
            description: None,
            description_resolver: None,
            input_schema,
            output_schema: None,
            context_schema: None,
            input_examples: None,
            strict: None,
            provider_options: None,
            metadata: None,
            needs_approval: None,
            needs_approval_resolver: None,
            execute: None,
            execute_outputs: None,
            on_input_start: None,
            on_input_delta: None,
            on_input_available: None,
            to_model_output: None,
        }
    }

    /// Creates a provider tool with explicit provider-execution state.
    ///
    /// This supports workflow step boundaries where upstream serializes only
    /// the provider tool id, arguments, input schema, and `isProviderExecuted`
    /// flag, without an output schema.
    pub fn provider_tool(
        name: impl Into<String>,
        id: impl Into<String>,
        args: JsonObject,
        input_schema: JsonSchema,
        provider_executed: bool,
    ) -> Self {
        Self {
            kind: ToolKind::Provider {
                id: id.into(),
                args,
                provider_executed,
                output_schema: None,
                supports_deferred_results: None,
            },
            name: name.into(),
            title: None,
            description: None,
            description_resolver: None,
            input_schema,
            output_schema: None,
            context_schema: None,
            input_examples: None,
            strict: None,
            provider_options: None,
            metadata: None,
            needs_approval: None,
            needs_approval_resolver: None,
            execute: None,
            execute_outputs: None,
            on_input_start: None,
            on_input_delta: None,
            on_input_available: None,
            to_model_output: None,
        }
    }

    /// Sets the tool description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        if !self.is_provider_tool() {
            self.description = Some(description.into());
            self.description_resolver = None;
        }
        self
    }

    /// Sets a runtime-dependent tool description.
    ///
    /// Upstream function-style tool descriptions can be functions that receive
    /// the current tool context and experimental sandbox. Rust keeps that
    /// runtime-only behavior as a synchronous callback so provider-facing tool
    /// definitions can be prepared without adding an async dependency.
    pub fn with_dynamic_description<F>(mut self, description: F) -> Self
    where
        F: Fn(ToolDescriptionOptions) -> String + Send + Sync + 'static,
    {
        self.description = None;
        self.description_resolver = Some(Arc::new(description));
        self
    }

    /// Sets the optional display title for this tool.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Adds a tool input example.
    pub fn with_input_example(mut self, input: JsonObject) -> Self {
        if !self.is_provider_tool() {
            self.input_examples
                .get_or_insert_with(Vec::new)
                .push(LanguageModelToolInputExample::new(input));
        }
        self
    }

    /// Sets the schema used to validate tool-specific context before execution.
    pub fn with_context_schema(
        mut self,
        context_schema: impl Into<FlexibleSchema<JsonValue>>,
    ) -> Self {
        self.context_schema = Some(context_schema.into());
        self
    }

    /// Sets strict mode for providers that support it.
    pub fn with_strict(mut self, strict: bool) -> Self {
        if !self.is_provider_tool() {
            self.strict = Some(strict);
        }
        self
    }

    /// Sets the expected output schema.
    pub fn with_output_schema(mut self, output_schema: JsonSchema) -> Self {
        match &mut self.kind {
            ToolKind::Provider {
                output_schema: stored_output_schema,
                ..
            } => {
                *stored_output_schema = Some(output_schema);
            }
            ToolKind::Function | ToolKind::Dynamic => {
                self.output_schema = Some(output_schema);
            }
        }

        self
    }

    /// Sets whether a provider-executed tool supports deferred results.
    pub fn with_supports_deferred_results(mut self, supports_deferred_results: bool) -> Self {
        if let ToolKind::Provider {
            provider_executed: true,
            supports_deferred_results: stored_supports_deferred_results,
            ..
        } = &mut self.kind
        {
            *stored_supports_deferred_results = Some(supports_deferred_results);
        }

        self
    }

    /// Adds provider-specific options to this tool.
    pub fn with_provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = Some(provider_options);
        self
    }

    /// Sets high-level tool metadata that is not sent to the provider.
    pub fn with_metadata(mut self, metadata: JsonObject) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Sets whether this tool requires approval before execution.
    ///
    /// This is the Rust equivalent of upstream tool-defined `needsApproval`
    /// when it is configured as a boolean rather than a callback.
    pub fn with_needs_approval(mut self, needs_approval: bool) -> Self {
        self.needs_approval = Some(needs_approval);
        self.needs_approval_resolver = None;
        self
    }

    /// Sets a runtime callback that determines whether this tool requires approval.
    ///
    /// This mirrors upstream's deprecated function-form `needsApproval`
    /// setting while keeping approval resolution dependency-free and async.
    pub fn with_needs_approval_function<F, Fut>(mut self, needs_approval: F) -> Self
    where
        F: Fn(JsonValue, ToolNeedsApprovalOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        self.needs_approval = None;
        self.needs_approval_resolver = Some(Arc::new(move |input, options| {
            Box::pin(needs_approval(input, options))
        }));
        self
    }

    /// Sets the Rust executor for this tool.
    pub fn with_execute<F, Fut>(mut self, execute: F) -> Self
    where
        F: Fn(JsonValue, ToolExecutionOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<JsonValue, ToolExecutionError>> + Send + 'static,
    {
        self.execute = Some(Arc::new(move |input, options| {
            Box::pin(execute(input, options))
        }));
        self.execute_outputs = None;
        self
    }

    /// Sets a Rust executor that emits upstream-shaped preliminary/final records.
    pub fn with_execute_outputs<F, Fut>(mut self, execute_outputs: F) -> Self
    where
        F: Fn(JsonValue, ToolExecutionOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Vec<ExecuteToolOutput>, ToolExecutionError>> + Send + 'static,
    {
        self.execute = None;
        self.execute_outputs = Some(Arc::new(move |input, options| {
            Box::pin(execute_outputs(input, options))
        }));
        self
    }

    /// Sets the callback invoked when streamed tool input starts.
    pub fn with_on_input_start<F, Fut>(mut self, on_input_start: F) -> Self
    where
        F: Fn(ToolExecutionOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.on_input_start = Some(Arc::new(move |options| Box::pin(on_input_start(options))));
        self
    }

    /// Sets the callback invoked for each streamed tool input text delta.
    pub fn with_on_input_delta<F, Fut>(mut self, on_input_delta: F) -> Self
    where
        F: Fn(ToolInputDeltaOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.on_input_delta = Some(Arc::new(move |options| Box::pin(on_input_delta(options))));
        self
    }

    /// Sets the callback invoked when a complete tool input is available.
    pub fn with_on_input_available<F, Fut>(mut self, on_input_available: F) -> Self
    where
        F: Fn(ToolInputAvailableOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.on_input_available = Some(Arc::new(move |options| {
            Box::pin(on_input_available(options))
        }));
        self
    }

    /// Sets the conversion callback used to shape model-facing tool output.
    ///
    /// Upstream `toModelOutput` is invoked after successful local tool
    /// execution, and before a tool result is appended to the next model
    /// prompt. Error outputs bypass this callback.
    pub fn with_to_model_output<F, Fut>(mut self, to_model_output: F) -> Self
    where
        F: Fn(ToolModelOutputOptions) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = LanguageModelToolResultOutput> + Send + 'static,
    {
        self.to_model_output = Some(Arc::new(move |options| Box::pin(to_model_output(options))));
        self
    }

    /// Returns whether this tool has an executor.
    pub fn is_executable(&self) -> bool {
        self.execute.is_some() || self.execute_outputs.is_some()
    }

    /// Returns whether this tool is a provider tool.
    pub fn is_provider_tool(&self) -> bool {
        matches!(self.kind, ToolKind::Provider { .. })
    }

    /// Returns whether this tool is defined dynamically at runtime.
    pub fn is_dynamic(&self) -> bool {
        matches!(self.kind, ToolKind::Dynamic)
    }

    /// Returns whether this tool is executed by the provider.
    pub fn is_provider_executed(&self) -> bool {
        matches!(
            self.kind,
            ToolKind::Provider {
                provider_executed: true,
                ..
            }
        )
    }

    /// Returns the provider tool identifier for provider tools.
    pub fn provider_tool_id(&self) -> Option<&str> {
        match &self.kind {
            ToolKind::Provider { id, .. } => Some(id),
            ToolKind::Function | ToolKind::Dynamic => None,
        }
    }

    /// Returns the provider tool arguments for provider tools.
    pub fn provider_tool_args(&self) -> Option<&JsonObject> {
        match &self.kind {
            ToolKind::Provider { args, .. } => Some(args),
            ToolKind::Function | ToolKind::Dynamic => None,
        }
    }

    /// Returns the expected output schema when one is configured.
    pub fn output_schema(&self) -> Option<&JsonSchema> {
        match &self.kind {
            ToolKind::Provider { output_schema, .. } => output_schema.as_ref(),
            ToolKind::Function | ToolKind::Dynamic => self.output_schema.as_ref(),
        }
    }

    /// Returns the schema used to validate tool-specific context, if configured.
    pub fn context_schema(&self) -> Option<&FlexibleSchema<JsonValue>> {
        self.context_schema.as_ref()
    }

    /// Returns whether this provider-executed tool supports deferred results.
    pub fn supports_deferred_results(&self) -> Option<bool> {
        match &self.kind {
            ToolKind::Provider {
                supports_deferred_results,
                ..
            } => *supports_deferred_results,
            ToolKind::Function | ToolKind::Dynamic => None,
        }
    }

    /// Returns high-level tool metadata when configured.
    pub fn metadata(&self) -> Option<&JsonObject> {
        self.metadata.as_ref()
    }

    /// Returns the optional high-level display title.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Returns whether this tool has a static approval requirement.
    pub fn needs_approval(&self) -> Option<bool> {
        self.needs_approval
    }

    /// Returns whether this tool has a tool-defined approval callback.
    pub fn has_needs_approval_function(&self) -> bool {
        self.needs_approval_resolver.is_some()
    }

    /// Resolves this tool's approval requirement when one is configured.
    pub fn resolve_needs_approval(
        &self,
        input: JsonValue,
        options: ToolNeedsApprovalOptions,
    ) -> Option<ToolNeedsApprovalFuture> {
        if let Some(needs_approval) = self.needs_approval {
            return Some(Box::pin(std::future::ready(needs_approval)));
        }

        self.needs_approval_resolver
            .as_ref()
            .map(|needs_approval| needs_approval(input, options))
    }

    /// Returns whether this tool has a runtime-dependent description callback.
    pub fn has_dynamic_description(&self) -> bool {
        self.description_resolver.is_some()
    }

    /// Returns whether this tool has a model-output conversion callback.
    pub fn has_to_model_output(&self) -> bool {
        self.to_model_output.is_some()
    }

    /// Returns whether this tool has a streamed input-start callback.
    pub fn has_on_input_start(&self) -> bool {
        self.on_input_start.is_some()
    }

    /// Returns whether this tool has a streamed input-delta callback.
    pub fn has_on_input_delta(&self) -> bool {
        self.on_input_delta.is_some()
    }

    /// Returns whether this tool has a complete-input callback.
    pub fn has_on_input_available(&self) -> bool {
        self.on_input_available.is_some()
    }

    /// Executes this tool when an executor is present.
    pub fn execute(
        &self,
        input: JsonValue,
        options: ToolExecutionOptions,
    ) -> Option<ToolExecuteFuture> {
        self.execute.as_ref().map(|execute| execute(input, options))
    }

    /// Executes this tool as an upstream-shaped output stream when configured.
    pub fn execute_outputs(
        &self,
        input: JsonValue,
        options: ToolExecutionOptions,
    ) -> Option<ToolExecuteOutputsFuture> {
        self.execute_outputs
            .as_ref()
            .map(|execute_outputs| execute_outputs(input, options))
    }

    /// Invokes this tool's streamed input-start callback when configured.
    pub fn on_input_start(&self, options: ToolExecutionOptions) -> Option<ToolInputCallbackFuture> {
        self.on_input_start
            .as_ref()
            .map(|on_input_start| on_input_start(options))
    }

    /// Invokes this tool's streamed input-delta callback when configured.
    pub fn on_input_delta(
        &self,
        options: ToolInputDeltaOptions,
    ) -> Option<ToolInputCallbackFuture> {
        self.on_input_delta
            .as_ref()
            .map(|on_input_delta| on_input_delta(options))
    }

    /// Invokes this tool's complete-input callback when configured.
    pub fn on_input_available(
        &self,
        options: ToolInputAvailableOptions,
    ) -> Option<ToolInputCallbackFuture> {
        self.on_input_available
            .as_ref()
            .map(|on_input_available| on_input_available(options))
    }

    /// Converts raw tool output into model-facing output when a callback exists.
    pub fn model_output(&self, options: ToolModelOutputOptions) -> Option<ToolModelOutputFuture> {
        self.to_model_output
            .as_ref()
            .map(|to_model_output| to_model_output(options))
    }

    /// Converts this high-level tool into the provider-facing language-model tool shape.
    pub fn to_language_model_tool(&self) -> LanguageModelTool {
        self.to_language_model_tool_with_context(None, None)
    }

    /// Converts this high-level tool into the provider-facing shape with
    /// runtime context available for dynamic descriptions.
    pub fn to_language_model_tool_with_context(
        &self,
        context: Option<&JsonValue>,
        experimental_sandbox: Option<&Arc<dyn ExperimentalSandbox>>,
    ) -> LanguageModelTool {
        if let ToolKind::Provider { id, args, .. } = &self.kind {
            return LanguageModelTool::Provider(LanguageModelProviderTool::new(
                id.clone(),
                self.name.clone(),
                args.clone(),
            ));
        }

        let mut tool = LanguageModelFunctionTool::new(self.name.clone(), self.input_schema.clone());

        if let Some(description) = self.resolve_description(context, experimental_sandbox) {
            tool = tool.with_description(description);
        }

        if let Some(input_examples) = &self.input_examples {
            for input_example in input_examples {
                tool = tool.with_input_example(input_example.input.clone());
            }
        }

        if let Some(strict) = self.strict {
            tool = tool.with_strict(strict);
        }

        if let Some(provider_options) = &self.provider_options {
            tool = tool.with_provider_options(provider_options.clone());
        }

        LanguageModelTool::Function(tool)
    }

    fn resolve_description(
        &self,
        context: Option<&JsonValue>,
        experimental_sandbox: Option<&Arc<dyn ExperimentalSandbox>>,
    ) -> Option<String> {
        if let Some(description_resolver) = &self.description_resolver {
            return Some(description_resolver(ToolDescriptionOptions {
                context: context.cloned(),
                experimental_sandbox: experimental_sandbox.cloned(),
            }));
        }

        self.description.clone()
    }
}

impl fmt::Debug for Tool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Tool")
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("title", &self.title)
            .field("description", &self.description)
            .field(
                "has_dynamic_description",
                &self.description_resolver.is_some(),
            )
            .field("has_to_model_output", &self.to_model_output.is_some())
            .field("input_schema", &self.input_schema)
            .field("output_schema", &self.output_schema)
            .field("context_schema", &self.context_schema)
            .field("input_examples", &self.input_examples)
            .field("strict", &self.strict)
            .field("provider_options", &self.provider_options)
            .field("metadata", &self.metadata)
            .field("needs_approval", &self.needs_approval)
            .field(
                "has_needs_approval_function",
                &self.needs_approval_resolver.is_some(),
            )
            .field("has_on_input_start", &self.on_input_start.is_some())
            .field("has_on_input_delta", &self.on_input_delta.is_some())
            .field("has_on_input_available", &self.on_input_available.is_some())
            .field("is_executable", &self.is_executable())
            .finish()
    }
}

/// Creates a provider-defined tool factory with an input schema.
pub fn create_provider_defined_tool_factory(
    id: impl Into<String>,
    input_schema: JsonSchema,
) -> ProviderDefinedToolFactory {
    ProviderDefinedToolFactory::new(id, input_schema)
}

/// Creates a provider-defined tool factory with input and output schemas.
pub fn create_provider_defined_tool_factory_with_output_schema(
    id: impl Into<String>,
    input_schema: JsonSchema,
    output_schema: JsonSchema,
) -> ProviderDefinedToolFactory {
    ProviderDefinedToolFactory::new(id, input_schema).with_output_schema(output_schema)
}

/// Creates a provider-executed tool factory with input and output schemas.
pub fn create_provider_executed_tool_factory(
    id: impl Into<String>,
    input_schema: JsonSchema,
    output_schema: JsonSchema,
) -> ProviderExecutedToolFactory {
    ProviderExecutedToolFactory::new(id, input_schema, output_schema)
}

/// Defines a function-style tool.
///
/// This is the Rust-native counterpart to upstream provider-utils `tool`.
/// Upstream infers the tool name from the surrounding tool set; the Rust
/// contract stores it directly so provider-v4 tool preparation can remain
/// dependency-free and explicit.
pub fn tool(name: impl Into<String>, input_schema: JsonSchema) -> Tool {
    Tool::new(name, input_schema)
}

/// Defines a dynamic runtime tool.
///
/// Dynamic tools prepare as provider-v4 function tools, matching upstream
/// `dynamicTool`, while retaining their high-level dynamic identity in Rust.
pub fn dynamic_tool(name: impl Into<String>, input_schema: JsonSchema) -> Tool {
    Tool::dynamic(name, input_schema)
}

/// Returns whether a tool exposes a Rust executor.
///
/// This mirrors upstream provider-utils `isExecutableTool`. Rust callers can
/// also use [`Tool::is_executable`] directly when they already have a tool.
pub fn is_executable_tool(tool: Option<&Tool>) -> bool {
    tool.is_some_and(Tool::is_executable)
}

/// Executes a Rust tool and returns its upstream-shaped output stream records.
///
/// Upstream `executeTool` yields preliminary records for async iterable tool
/// outputs and a final record at completion. This dependency-free Rust helper
/// keeps the public output contract but currently returns one final record
/// because [`ToolExecuteFunction`] produces a single JSON value.
pub async fn execute_tool(
    tool: &Tool,
    input: JsonValue,
    options: ToolExecutionOptions,
) -> Result<Vec<ExecuteToolOutput>, ToolExecutionError> {
    if let Some(execute_outputs) = tool.execute_outputs(input.clone(), options.clone()) {
        return execute_outputs.await.map(normalize_execute_tool_outputs);
    }

    let Some(execute) = tool.execute(input, options) else {
        return Err(ToolExecutionError::new("Tool is not executable."));
    };

    execute
        .await
        .map(|output| vec![ExecuteToolOutput::final_output(output)])
}

fn normalize_execute_tool_outputs(mut outputs: Vec<ExecuteToolOutput>) -> Vec<ExecuteToolOutput> {
    if let Some(ExecuteToolOutput::Preliminary { output }) = outputs.last().cloned() {
        outputs.push(ExecuteToolOutput::final_output(output));
    }

    outputs
}

/// Bidirectional mapping between caller-facing and provider-facing tool names.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToolNameMapping {
    custom_tool_name_to_provider_tool_name: BTreeMap<String, String>,
    provider_tool_name_to_custom_tool_name: BTreeMap<String, String>,
}

impl ToolNameMapping {
    /// Maps a caller-facing tool name to the provider-facing name.
    ///
    /// Names without a mapping are returned unchanged.
    pub fn to_provider_tool_name(&self, custom_tool_name: &str) -> String {
        self.custom_tool_name_to_provider_tool_name
            .get(custom_tool_name)
            .cloned()
            .unwrap_or_else(|| custom_tool_name.to_string())
    }

    /// Maps a provider-facing tool name to the caller-facing name.
    ///
    /// Names without a mapping are returned unchanged.
    pub fn to_custom_tool_name(&self, provider_tool_name: &str) -> String {
        self.provider_tool_name_to_custom_tool_name
            .get(provider_tool_name)
            .cloned()
            .unwrap_or_else(|| provider_tool_name.to_string())
    }
}

/// Creates provider-defined tool name mappings from model tools.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `createToolNameMapping`:
/// only provider-defined tools whose ids are present in `provider_tool_names`
/// produce mappings; function tools and unknown provider tool ids pass through
/// unchanged.
pub fn create_tool_name_mapping<'a>(
    tools: impl IntoIterator<Item = &'a LanguageModelTool>,
    provider_tool_names: &BTreeMap<String, String>,
) -> ToolNameMapping {
    let mut mapping = ToolNameMapping::default();

    for tool in tools {
        let LanguageModelTool::Provider(tool) = tool else {
            continue;
        };

        if let Some(provider_tool_name) = provider_tool_names.get(&tool.id) {
            mapping
                .custom_tool_name_to_provider_tool_name
                .insert(tool.name.clone(), provider_tool_name.clone());
            mapping
                .provider_tool_name_to_custom_tool_name
                .insert(provider_tool_name.clone(), tool.name.clone());
        }
    }

    mapping
}

/// Converts high-level Rust tools into provider-facing language-model tools.
pub fn prepare_tools<'a>(
    tools: impl IntoIterator<Item = &'a Tool>,
) -> Option<Vec<LanguageModelTool>> {
    prepare_tools_with_context(tools, None, None)
}

/// Converts high-level Rust tools into provider-facing language-model tools
/// with runtime context available for dynamic tool descriptions.
pub fn prepare_tools_with_context<'a>(
    tools: impl IntoIterator<Item = &'a Tool>,
    tools_context: Option<&JsonObject>,
    experimental_sandbox: Option<&Arc<dyn ExperimentalSandbox>>,
) -> Option<Vec<LanguageModelTool>> {
    let tools = tools
        .into_iter()
        .map(|tool| {
            tool.to_language_model_tool_with_context(
                tools_context.and_then(|context| context.get(&tool.name)),
                experimental_sandbox,
            )
        })
        .collect::<Vec<_>>();

    if tools.is_empty() { None } else { Some(tools) }
}

/// Options for injecting JSON response instructions into a standardized prompt.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InjectJsonInstructionIntoMessagesOptions {
    /// Standardized prompt messages to update.
    pub messages: LanguageModelPrompt,

    /// JSON schema to include in the system instruction.
    pub schema: Option<JsonSchema>,

    /// Custom prefix to place before the serialized JSON schema.
    pub schema_prefix: Option<String>,

    /// Custom suffix to place after the serialized JSON schema or generic JSON instruction.
    pub schema_suffix: Option<String>,
}

impl InjectJsonInstructionIntoMessagesOptions {
    /// Creates JSON instruction injection options for a standardized prompt.
    pub fn new(messages: LanguageModelPrompt) -> Self {
        Self {
            messages,
            schema: None,
            schema_prefix: None,
            schema_suffix: None,
        }
    }

    /// Sets the JSON schema included in the system instruction.
    pub fn with_schema(mut self, schema: JsonSchema) -> Self {
        self.schema = Some(schema);
        self
    }

    /// Sets the prefix placed before the serialized JSON schema.
    pub fn with_schema_prefix(mut self, schema_prefix: impl Into<String>) -> Self {
        self.schema_prefix = Some(schema_prefix.into());
        self
    }

    /// Sets the suffix placed after the schema or generic JSON instruction.
    pub fn with_schema_suffix(mut self, schema_suffix: impl Into<String>) -> Self {
        self.schema_suffix = Some(schema_suffix.into());
        self
    }
}

/// Injects JSON response instructions into the leading system prompt message.
///
/// This mirrors upstream `@ai-sdk/provider-utils`
/// `injectJsonInstructionIntoMessages`: the first system message is updated
/// when present, otherwise a new system message is inserted before the original
/// prompt, and all non-system messages are preserved in order.
pub fn inject_json_instruction_into_messages(
    options: InjectJsonInstructionIntoMessagesOptions,
) -> LanguageModelPrompt {
    let InjectJsonInstructionIntoMessagesOptions {
        messages,
        schema,
        schema_prefix,
        schema_suffix,
    } = options;

    let mut messages = messages.into_iter();
    let first_message = messages.next();
    let mut remaining_messages = Vec::new();

    let mut system_message = match first_message {
        Some(LanguageModelMessage::System(system_message)) => system_message,
        Some(message) => {
            remaining_messages.push(message);
            LanguageModelSystemMessage::new("")
        }
        None => LanguageModelSystemMessage::new(""),
    };

    remaining_messages.extend(messages);
    system_message.content = inject_json_instruction(
        Some(&system_message.content),
        schema.as_ref(),
        schema_prefix.as_deref(),
        schema_suffix.as_deref(),
    );

    let mut updated_messages = Vec::with_capacity(remaining_messages.len() + 1);
    updated_messages.push(LanguageModelMessage::System(system_message));
    updated_messages.extend(remaining_messages);
    updated_messages
}

fn inject_json_instruction(
    prompt: Option<&str>,
    schema: Option<&JsonSchema>,
    schema_prefix: Option<&str>,
    schema_suffix: Option<&str>,
) -> String {
    let mut lines = Vec::new();

    if let Some(prompt) = prompt.filter(|prompt| !prompt.is_empty()) {
        lines.push(prompt.to_string());
        lines.push(String::new());
    }

    let schema_prefix = schema_prefix.or(schema.map(|_| DEFAULT_JSON_SCHEMA_INSTRUCTION_PREFIX));
    if let Some(schema_prefix) = schema_prefix {
        lines.push(schema_prefix.to_string());
    }

    if let Some(schema) = schema {
        lines.push(serde_json::to_string(schema).expect("JSON schemas serialize"));
    }

    let schema_suffix = schema_suffix.or_else(|| {
        Some(if schema.is_some() {
            DEFAULT_JSON_SCHEMA_INSTRUCTION_SUFFIX
        } else {
            DEFAULT_JSON_INSTRUCTION_SUFFIX
        })
    });
    if let Some(schema_suffix) = schema_suffix {
        lines.push(schema_suffix.to_string());
    }

    lines.join("\n")
}

/// Adds `additionalProperties: false` to object JSON schemas recursively.
///
/// This mirrors upstream `@ai-sdk/provider-utils`
/// `addAdditionalPropertiesToJsonSchema`: object schemas, including union
/// schemas whose `type` includes `"object"`, are made closed recursively across
/// properties, items, composition lists, and definitions.
pub fn add_additional_properties_to_json_schema(mut json_schema: JsonSchema) -> JsonSchema {
    add_additional_properties_to_json_schema_object(&mut json_schema);
    json_schema
}

fn add_additional_properties_to_json_schema_object(json_schema: &mut JsonSchema) {
    if is_object_json_schema(json_schema) {
        json_schema.insert("additionalProperties".to_string(), JsonValue::Bool(false));

        if let Some(JsonValue::Object(properties)) = json_schema.get_mut("properties") {
            for property in properties.values_mut() {
                visit_json_schema_definition(property);
            }
        }
    }

    if let Some(items) = json_schema.get_mut("items") {
        visit_json_schema_definition_or_array(items);
    }

    for key in ["anyOf", "allOf", "oneOf"] {
        if let Some(JsonValue::Array(definitions)) = json_schema.get_mut(key) {
            for definition in definitions {
                visit_json_schema_definition(definition);
            }
        }
    }

    if let Some(JsonValue::Object(definitions)) = json_schema.get_mut("definitions") {
        for definition in definitions.values_mut() {
            visit_json_schema_definition(definition);
        }
    }
}

fn visit_json_schema_definition_or_array(definition: &mut JsonValue) {
    match definition {
        JsonValue::Array(definitions) => {
            for definition in definitions {
                visit_json_schema_definition(definition);
            }
        }
        _ => visit_json_schema_definition(definition),
    }
}

fn visit_json_schema_definition(definition: &mut JsonValue) {
    if let JsonValue::Object(json_schema) = definition {
        add_additional_properties_to_json_schema_object(json_schema);
    }
}

fn is_object_json_schema(json_schema: &JsonSchema) -> bool {
    match json_schema.get("type") {
        Some(JsonValue::String(schema_type)) => schema_type == "object",
        Some(JsonValue::Array(schema_types)) => schema_types
            .iter()
            .any(|schema_type| schema_type.as_str() == Some("object")),
        _ => false,
    }
}

/// Top-level reasoning effort levels that can be mapped to provider-specific settings.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasoningLevel {
    /// Use minimal reasoning effort.
    Minimal,
    /// Use low reasoning effort.
    Low,
    /// Use medium reasoning effort.
    Medium,
    /// Use high reasoning effort.
    High,
    /// Use extra-high reasoning effort.
    Xhigh,
}

impl ReasoningLevel {
    /// Returns the upstream provider-v4 string for this reasoning level.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
        }
    }
}

impl TryFrom<LanguageModelReasoningEffort> for ReasoningLevel {
    type Error = LanguageModelReasoningEffort;

    fn try_from(value: LanguageModelReasoningEffort) -> Result<Self, Self::Error> {
        match value {
            LanguageModelReasoningEffort::Minimal => Ok(Self::Minimal),
            LanguageModelReasoningEffort::Low => Ok(Self::Low),
            LanguageModelReasoningEffort::Medium => Ok(Self::Medium),
            LanguageModelReasoningEffort::High => Ok(Self::High),
            LanguageModelReasoningEffort::Xhigh => Ok(Self::Xhigh),
            value => Err(value),
        }
    }
}

/// Returns whether a reasoning request should override the provider default.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `isCustomReasoning`: missing
/// reasoning and `provider-default` are not custom, while `none` and all effort
/// levels are custom reasoning settings.
pub fn is_custom_reasoning(reasoning: Option<&LanguageModelReasoningEffort>) -> bool {
    !matches!(
        reasoning,
        None | Some(LanguageModelReasoningEffort::ProviderDefault)
    )
}

/// Maps a top-level reasoning effort level to a provider-specific effort value.
///
/// This mirrors upstream `mapReasoningToProviderEffort`: unsupported levels add
/// an unsupported warning, and renamed levels add a compatibility warning.
pub fn map_reasoning_to_provider_effort<T>(
    reasoning: ReasoningLevel,
    effort_map: &BTreeMap<ReasoningLevel, T>,
    warnings: &mut Vec<Warning>,
) -> Option<T>
where
    T: AsRef<str> + Clone,
{
    let Some(mapped) = effort_map.get(&reasoning) else {
        warnings.push(Warning::Unsupported {
            feature: "reasoning".to_string(),
            details: Some(format!(
                "reasoning \"{}\" is not supported by this model.",
                reasoning.as_str()
            )),
        });
        return None;
    };

    if mapped.as_ref() != reasoning.as_str() {
        warnings.push(Warning::Compatibility {
            feature: "reasoning".to_string(),
            details: Some(format!(
                "reasoning \"{}\" is not directly supported by this model. mapped to effort \"{}\".",
                reasoning.as_str(),
                mapped.as_ref()
            )),
        });
    }

    Some(mapped.clone())
}

/// Maps a top-level reasoning effort level to a provider-specific token budget.
///
/// The budget is the rounded product of max output tokens and the configured
/// percentage, clamped between the minimum and maximum reasoning budgets.
pub fn map_reasoning_to_provider_budget(
    reasoning: ReasoningLevel,
    max_output_tokens: u64,
    max_reasoning_budget: u64,
    min_reasoning_budget: Option<u64>,
    budget_percentages: Option<&BTreeMap<ReasoningLevel, f64>>,
    warnings: &mut Vec<Warning>,
) -> Option<u64> {
    let percentage = match budget_percentages {
        Some(percentages) => percentages.get(&reasoning).copied(),
        None => Some(default_reasoning_budget_percentage(reasoning)),
    };

    let Some(percentage) = percentage else {
        warnings.push(Warning::Unsupported {
            feature: "reasoning".to_string(),
            details: Some(format!(
                "reasoning \"{}\" is not supported by this model.",
                reasoning.as_str()
            )),
        });
        return None;
    };

    let requested_budget = ((max_output_tokens as f64) * percentage).round() as u64;

    Some(
        requested_budget
            .max(min_reasoning_budget.unwrap_or(1024))
            .min(max_reasoning_budget),
    )
}

fn default_reasoning_budget_percentage(reasoning: ReasoningLevel) -> f64 {
    match reasoning {
        ReasoningLevel::Minimal => 0.02,
        ReasoningLevel::Low => 0.1,
        ReasoningLevel::Medium => 0.3,
        ReasoningLevel::High => 0.6,
        ReasoningLevel::Xhigh => 0.9,
    }
}

/// A value that can be supplied as either one item or an array of items.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Arrayable<T> {
    /// A single item.
    Single(T),

    /// Multiple items.
    Array(Vec<T>),
}

impl<T> Arrayable<T> {
    /// Creates an arrayable single value.
    pub fn single(value: T) -> Self {
        Self::Single(value)
    }

    /// Creates an arrayable array value.
    pub fn array(values: Vec<T>) -> Self {
        Self::Array(values)
    }

    /// Converts the value into an array.
    pub fn into_vec(self) -> Vec<T> {
        match self {
            Self::Single(value) => vec![value],
            Self::Array(values) => values,
        }
    }
}

/// Normalizes a missing, single, or array value into an array.
pub fn as_array<T>(value: Option<Arrayable<T>>) -> Vec<T> {
    value.map_or_else(Vec::new, Arrayable::into_vec)
}

/// Checks whether an optional value is present.
pub fn is_non_nullable<T>(value: &Option<T>) -> bool {
    value.is_some()
}

/// Filters missing values out of a list of optional values.
pub fn filter_nullable<T>(values: impl IntoIterator<Item = Option<T>>) -> Vec<T> {
    values.into_iter().flatten().collect()
}

/// Removes entries whose values are missing.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `removeUndefinedEntries`:
/// values that are nullish in JavaScript are omitted from the returned record,
/// while present falsy-equivalent values are preserved.
pub fn remove_undefined_entries<K, T, I>(record: I) -> BTreeMap<String, T>
where
    I: IntoIterator<Item = (K, Option<T>)>,
    K: Into<String>,
{
    record
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key.into(), value)))
        .collect()
}

/// Serializes provider model options for workflow step boundaries.
///
/// Upstream `serializeModelOptions` keeps JSON-serializable config values and
/// filters out functions, class instances, and other JavaScript-only
/// non-serializable values. Rust's JSON value type cannot hold those values, so
/// callers can pass `None` for config entries that should be omitted; present
/// JSON values, including `null`, are preserved.
pub fn serialize_model_options<K, V, I>(
    model_id: impl Into<String>,
    config: I,
) -> SerializedModelOptions
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<Option<JsonValue>>,
{
    let config = config
        .into_iter()
        .filter_map(|(key, value)| {
            let value: Option<JsonValue> = value.into();
            value.map(|value| (key.into(), value))
        })
        .collect();

    SerializedModelOptions::new(model_id, config)
}

/// Creates a non-cryptographic ID generator using upstream provider-utils rules.
///
/// The total ID length is the optional prefix length plus separator length plus
/// the configured random part length. When a prefix is present, the separator
/// must not occur in the alphabet so generated IDs can be parsed reliably.
pub fn create_id_generator(
    options: IdGeneratorOptions,
) -> Result<impl Fn() -> String + Send + Sync + 'static, InvalidArgumentError> {
    let IdGeneratorOptions {
        prefix,
        separator,
        size,
        alphabet,
    } = options;

    if prefix.is_some() && alphabet.contains(&separator) {
        return Err(InvalidArgumentError::new(
            "separator",
            format!(
                "The separator \"{separator}\" must not be part of the alphabet \"{alphabet}\"."
            ),
        ));
    }

    let alphabet: Vec<char> = alphabet.chars().collect();

    Ok(move || {
        let random_part = generate_random_id_part(&alphabet, size);

        if let Some(prefix) = &prefix {
            let mut id = String::with_capacity(prefix.len() + separator.len() + random_part.len());
            id.push_str(prefix);
            id.push_str(&separator);
            id.push_str(&random_part);
            id
        } else {
            random_part
        }
    })
}

/// Generates a 16-character non-cryptographic random ID using upstream defaults.
pub fn generate_id() -> String {
    let alphabet: Vec<char> = DEFAULT_ID_ALPHABET.chars().collect();
    generate_random_id_part(&alphabet, DEFAULT_ID_SIZE)
}

fn generate_random_id_part(alphabet: &[char], size: usize) -> String {
    if alphabet.is_empty() || size == 0 {
        return String::new();
    }

    let mut seed = random_id_seed() | 1;
    let mut id = String::with_capacity(size);

    for _ in 0..size {
        let random = next_id_random(&mut seed);
        id.push(alphabet[random as usize % alphabet.len()]);
    }

    id
}

fn random_id_seed() -> u64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let nanos = duration.as_nanos();
    let time_seed = (nanos as u64) ^ ((nanos >> 64) as u64);
    let counter = ID_GENERATOR_COUNTER.fetch_add(0x9e37_79b9_7f4a_7c15, Ordering::Relaxed);

    time_seed ^ counter.rotate_left(17)
}

fn next_id_random(seed: &mut u64) -> u64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    *seed
}

/// Checks whether a JSON value has the provider-reference record shape.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `isProviderReference` at the
/// JSON boundary: plain objects without a `type` discriminator are treated as
/// provider references, while tagged file-data objects and non-objects are not.
pub fn is_provider_reference(data: &JsonValue) -> bool {
    data.as_object()
        .is_some_and(|object| !object.contains_key("type"))
}

/// Validates a JSON value with a schema.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `validateTypes`: validation
/// failures are wrapped in the provider-level [`TypeValidationError`] with the
/// original JSON value and optional validation context.
pub fn validate_types<T>(
    value: JsonValue,
    schema: impl Into<FlexibleSchema<T>>,
    context: Option<TypeValidationContext>,
) -> Result<T, TypeValidationError>
where
    T: DeserializeOwned + 'static,
{
    match safe_validate_types(value, schema, context) {
        ValidateTypesResult::Success { value, .. } => Ok(value),
        ValidateTypesResult::Failure { error, .. } => Err(error),
    }
}

/// Safely validates a JSON value with a schema.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `safeValidateTypes`: success
/// returns both the validated value and the original raw value, while
/// validation failures return a [`TypeValidationError`] and preserve the raw
/// value.
pub fn safe_validate_types<T>(
    value: JsonValue,
    schema: impl Into<FlexibleSchema<T>>,
    context: Option<TypeValidationContext>,
) -> ValidateTypesResult<T>
where
    T: DeserializeOwned + 'static,
{
    let schema = schema.into().into_schema();

    match schema.validate(&value) {
        Some(ValidationResult::Success {
            value: validated_value,
        }) => ValidateTypesResult::success(validated_value, value),
        Some(ValidationResult::Failure { error }) => {
            let validation_error = TypeValidationError::new(value.clone(), error, context);
            ValidateTypesResult::failure(validation_error, value)
        }
        None => match serde_json::from_value::<T>(value.clone()) {
            Ok(validated_value) => ValidateTypesResult::success(validated_value, value),
            Err(error) => {
                let validation_error = TypeValidationError::new(value.clone(), error, context);
                ValidateTypesResult::failure(validation_error, value)
            }
        },
    }
}

fn safe_validate_types_with<T, F, E>(
    value: JsonValue,
    validate: F,
    context: Option<TypeValidationContext>,
) -> ValidateTypesResult<T>
where
    F: FnOnce(&JsonValue) -> Result<T, E>,
    E: fmt::Display,
{
    match validate(&value) {
        Ok(validated_value) => ValidateTypesResult::success(validated_value, value),
        Err(error) => {
            let validation_error = TypeValidationError::new(value.clone(), error, context);
            ValidateTypesResult::failure(validation_error, value)
        }
    }
}

/// Parses and validates options for a single provider.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `parseProviderOptions`:
/// missing provider options are ignored, present provider-specific options are
/// validated, and validation failures become an [`InvalidArgumentError`] for
/// the `providerOptions` argument.
pub fn parse_provider_options<T, F, E>(
    provider: &str,
    provider_options: Option<&ProviderOptions>,
    validate: F,
) -> Result<Option<T>, InvalidArgumentError>
where
    F: FnOnce(&JsonValue) -> Result<T, E>,
    E: fmt::Display,
{
    let Some(provider_options) = provider_options.and_then(|options| options.get(provider)) else {
        return Ok(None);
    };

    match safe_validate_types_with(JsonValue::Object(provider_options.clone()), validate, None) {
        ValidateTypesResult::Success { value, .. } => Ok(Some(value)),
        ValidateTypesResult::Failure { .. } => Err(InvalidArgumentError::new(
            "providerOptions",
            format!("invalid {provider} provider options"),
        )),
    }
}

/// Parses a JSON string into a JSON value.
///
/// This mirrors the no-schema overload of upstream `@ai-sdk/provider-utils`
/// `parseJSON`, using Rust's JSON representation and wrapping parse failures
/// in the provider-level [`JsonParseError`].
pub fn parse_json(text: &str) -> Result<JsonValue, JsonParseError> {
    secure_json_parse(text).map_err(|cause| JsonParseError::new(text, cause))
}

/// Parses a JSON string and validates it with a schema.
///
/// This mirrors the schema overload of upstream `@ai-sdk/provider-utils`
/// `parseJSON`: secure JSON parse failures are returned as
/// [`JsonParseError`], while schema failures are returned as
/// [`TypeValidationError`] through [`ParseJsonError`].
pub fn parse_json_with_schema<T>(
    text: &str,
    schema: impl Into<FlexibleSchema<T>>,
) -> Result<T, ParseJsonError>
where
    T: DeserializeOwned + 'static,
{
    match safe_parse_json_with_schema(text, schema) {
        ParseJsonResult::Success { value, .. } => Ok(value),
        ParseJsonResult::Failure { error, .. } => Err(error),
    }
}

/// Safely parses a JSON string into a JSON value.
///
/// This mirrors the no-schema overload of upstream `@ai-sdk/provider-utils`
/// `safeParseJSON`: successful parses include both `value` and `rawValue`, and
/// parse failures are returned as [`JsonParseError`] values without a raw JSON
/// value.
pub fn safe_parse_json(text: &str) -> ParseJsonResult {
    match parse_json(text) {
        Ok(value) => ParseJsonResult::success(value.clone(), value),
        Err(error) => ParseJsonResult::failure(error, None),
    }
}

/// Safely parses a JSON string and validates it with a schema.
///
/// This mirrors the schema overload of upstream `safeParseJSON`: successful
/// validation returns the typed value plus the original raw JSON value, parse
/// failures have no raw value, and schema failures preserve the parsed raw
/// value alongside the [`TypeValidationError`].
pub fn safe_parse_json_with_schema<T>(
    text: &str,
    schema: impl Into<FlexibleSchema<T>>,
) -> ParseJsonResult<T>
where
    T: DeserializeOwned + 'static,
{
    let raw_value = match parse_json(text) {
        Ok(value) => value,
        Err(error) => return ParseJsonResult::failure(error, None),
    };

    match safe_validate_types(raw_value.clone(), schema, None) {
        ValidateTypesResult::Success { value, raw_value } => {
            ParseJsonResult::success(value, raw_value)
        }
        ValidateTypesResult::Failure { error, raw_value } => {
            ParseJsonResult::failure(error, Some(raw_value))
        }
    }
}

/// Returns whether the input can be parsed as JSON.
pub fn is_parsable_json(input: &str) -> bool {
    secure_json_parse(input).is_ok()
}

fn secure_json_parse(text: &str) -> Result<JsonValue, SecureJsonParseError> {
    let value = serde_json::from_str::<JsonValue>(text).map_err(SecureJsonParseError::Parse)?;
    reject_forbidden_json_keys(&value)?;
    Ok(value)
}

#[derive(Debug)]
enum SecureJsonParseError {
    Parse(serde_json::Error),
    ForbiddenPrototypeProperty,
}

impl fmt::Display for SecureJsonParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => error.fmt(formatter),
            Self::ForbiddenPrototypeProperty => {
                formatter.write_str("Object contains forbidden prototype property")
            }
        }
    }
}

fn reject_forbidden_json_keys(value: &JsonValue) -> Result<(), SecureJsonParseError> {
    match value {
        JsonValue::Array(values) => {
            for value in values {
                reject_forbidden_json_keys(value)?;
            }
        }
        JsonValue::Object(object) => {
            if object.contains_key("__proto__") {
                return Err(SecureJsonParseError::ForbiddenPrototypeProperty);
            }

            if object.get("constructor").is_some_and(JsonValue::is_object) {
                return Err(SecureJsonParseError::ForbiddenPrototypeProperty);
            }

            for value in object.values() {
                reject_forbidden_json_keys(value)?;
            }
        }
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) | JsonValue::String(_) => {}
    }

    Ok(())
}

/// Converts inline file data into raw bytes.
///
/// This mirrors upstream `@ai-sdk/provider-utils`
/// `convertInlineFileDataToUint8Array`: text file data is UTF-8 encoded, raw
/// byte data is returned unchanged, and string data is decoded from base64.
/// URL and provider-reference variants are rejected because the upstream helper
/// only accepts tagged inline data/text file data.
pub fn convert_inline_file_data_to_bytes(
    data: &FileData,
) -> Result<Vec<u8>, InlineFileDataBytesError> {
    match data {
        FileData::Text { text } => Ok(text.as_bytes().to_vec()),
        FileData::Data { data } => match data {
            FileDataContent::Bytes(bytes) => Ok(bytes.clone()),
            FileDataContent::Base64(base64) => convert_base64_to_bytes(base64)
                .map_err(|_| InlineFileDataBytesError::InvalidBase64Data),
        },
        FileData::Url { .. } | FileData::Reference { .. } => {
            Err(InlineFileDataBytesError::NonInlineFileData)
        }
    }
}

/// Converts a base64 or base64url string into raw bytes.
///
/// This mirrors upstream `@ai-sdk/provider-utils`
/// `convertBase64ToUint8Array`: URL-safe `-` and `_` alphabet characters are
/// accepted in addition to ordinary base64 data.
pub fn convert_base64_to_bytes(base64: &str) -> Result<Vec<u8>, Base64DecodeError> {
    decode_base64(base64).ok_or(Base64DecodeError)
}

/// Converts raw bytes into a base64 string.
///
/// This mirrors upstream `@ai-sdk/provider-utils`
/// `convertUint8ArrayToBase64`.
pub fn convert_bytes_to_base64(bytes: &[u8]) -> String {
    encode_base64(bytes)
}

/// Converts file data content into a base64 string.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `convertToBase64`: base64
/// strings pass through unchanged, while raw bytes are encoded.
pub fn convert_to_base64(value: &FileDataContent) -> String {
    match value {
        FileDataContent::Bytes(bytes) => convert_bytes_to_base64(bytes),
        FileDataContent::Base64(base64) => base64.clone(),
    }
}

/// Detects the IANA media type of raw bytes or base64-encoded file content.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `detectMediaType`: when a
/// top-level media type is supplied, only that signature table is checked;
/// otherwise image, application document, audio, and video signatures are
/// considered in upstream order.
pub fn detect_media_type(
    data: &FileDataContent,
    top_level_type: Option<&str>,
) -> Option<&'static str> {
    if let Some(top_level_type) = top_level_type {
        return match top_level_type {
            "image" => detect_media_type_by_signatures(data, IMAGE_MEDIA_TYPE_SIGNATURES),
            "audio" => detect_media_type_by_signatures(data, AUDIO_MEDIA_TYPE_SIGNATURES),
            "video" => detect_media_type_by_signatures(data, VIDEO_MEDIA_TYPE_SIGNATURES),
            "application" => detect_media_type_by_signatures(data, DOCUMENT_MEDIA_TYPE_SIGNATURES),
            _ => None,
        };
    }

    for signatures in [
        IMAGE_MEDIA_TYPE_SIGNATURES,
        DOCUMENT_MEDIA_TYPE_SIGNATURES,
        AUDIO_MEDIA_TYPE_SIGNATURES,
        VIDEO_MEDIA_TYPE_SIGNATURES,
    ] {
        if let Some(media_type) = detect_media_type_by_signatures(data, signatures) {
            return Some(media_type);
        }
    }

    None
}

fn detect_media_type_by_signatures(
    data: &FileDataContent,
    signatures: &[MediaTypeSignature],
) -> Option<&'static str> {
    let bytes = bytes_for_media_type_detection(data)?;

    signatures
        .iter()
        .find(|signature| bytes_match_signature(&bytes, signature.bytes_prefix))
        .map(|signature| signature.media_type)
}

fn bytes_match_signature(bytes: &[u8], bytes_prefix: &[Option<u8>]) -> bool {
    bytes.len() >= bytes_prefix.len()
        && bytes_prefix
            .iter()
            .enumerate()
            .all(|(index, byte)| byte.is_none_or(|byte| bytes[index] == byte))
}

fn bytes_for_media_type_detection(data: &FileDataContent) -> Option<Vec<u8>> {
    match data {
        FileDataContent::Bytes(bytes) => Some(strip_id3_tags_if_present(bytes).to_vec()),
        FileDataContent::Base64(base64) if base64.starts_with("SUQz") => {
            decode_base64(base64).map(|bytes| strip_id3_tags_if_present(&bytes).to_vec())
        }
        FileDataContent::Base64(base64) => {
            let prefix_length = base64
                .char_indices()
                .nth(24)
                .map_or(base64.len(), |(index, _)| index);
            decode_base64(&base64[..prefix_length])
        }
    }
}

fn strip_id3_tags_if_present(bytes: &[u8]) -> &[u8] {
    if bytes.len() <= 10 || !bytes.starts_with(&[0x49, 0x44, 0x33]) {
        return bytes;
    }

    let id3_size = ((usize::from(bytes[6] & 0x7f)) << 21)
        | ((usize::from(bytes[7] & 0x7f)) << 14)
        | ((usize::from(bytes[8] & 0x7f)) << 7)
        | usize::from(bytes[9] & 0x7f);

    bytes.get(id3_size + 10..).unwrap_or_default()
}

fn decode_base64(base64: &str) -> Option<Vec<u8>> {
    let mut sextets = Vec::new();

    for byte in base64.bytes() {
        match byte {
            b'=' => break,
            b'\t' | b'\n' | b'\r' | b' ' => continue,
            _ => sextets.push(base64_value(byte)?),
        }
    }

    if sextets.len() % 4 == 1 {
        return None;
    }

    let mut bytes = Vec::with_capacity((sextets.len() * 3) / 4);
    let mut chunks = sextets.chunks_exact(4);

    for chunk in &mut chunks {
        let buffer = (u32::from(chunk[0]) << 18)
            | (u32::from(chunk[1]) << 12)
            | (u32::from(chunk[2]) << 6)
            | u32::from(chunk[3]);
        bytes.push((buffer >> 16) as u8);
        bytes.push((buffer >> 8) as u8);
        bytes.push(buffer as u8);
    }

    match chunks.remainder() {
        [] => {}
        [first, second] => {
            bytes.push((*first << 2) | (*second >> 4));
        }
        [first, second, third] => {
            bytes.push((*first << 2) | (*second >> 4));
            bytes.push(((*second & 0x0f) << 4) | (*third >> 2));
        }
        _ => return None,
    }

    Some(bytes)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or_default();
        let third = chunk.get(2).copied().unwrap_or_default();
        let bits = (u32::from(first) << 16) | (u32::from(second) << 8) | u32::from(third);

        encoded.push(ALPHABET[((bits >> 18) & 0x3f) as usize] as char);
        encoded.push(ALPHABET[((bits >> 12) & 0x3f) as usize] as char);

        if chunk.len() > 1 {
            encoded.push(ALPHABET[((bits >> 6) & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }

        if chunk.len() > 2 {
            encoded.push(ALPHABET[(bits & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }
    }

    encoded
}

/// Returns the top-level segment of a media type.
pub fn get_top_level_media_type(media_type: &str) -> &str {
    media_type
        .find('/')
        .map_or(media_type, |slash_index| &media_type[..slash_index])
}

/// Returns whether a media type has a non-empty, non-wildcard subtype.
pub fn is_full_media_type(media_type: &str) -> bool {
    media_type
        .split_once('/')
        .is_some_and(|(_, subtype)| !subtype.is_empty() && subtype != "*")
}

/// Resolves a prompt file part media type to a full `type/subtype` value.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `resolveFullMediaType`:
/// full media types are returned unchanged, top-level or wildcard media types
/// are detected from inline byte data when possible, and other unresolved cases
/// report an [`UnsupportedFunctionalityError`].
pub fn resolve_full_media_type(
    part: &LanguageModelFilePart,
) -> Result<String, UnsupportedFunctionalityError> {
    if is_full_media_type(&part.media_type) {
        return Ok(part.media_type.clone());
    }

    let FileData::Data { data } = &part.data else {
        return Err(UnsupportedFunctionalityError::new(format!(
            "file of media type \"{}\" must specify subtype since it is not passed as inline bytes",
            part.media_type
        )));
    };

    detect_media_type(data, Some(get_top_level_media_type(&part.media_type)))
        .map(str::to_string)
        .ok_or_else(|| {
            UnsupportedFunctionalityError::new(format!(
                "file of media type \"{}\" must specify subtype since it could not be auto-detected",
                part.media_type
            ))
        })
}

/// Returns whether a URL is natively supported by a model for a media type.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `isUrlSupported`: media type
/// keys and the checked URL are matched case-insensitively by lowercasing before
/// regex evaluation, `*` and `*/*` match all media types, and top-level-only
/// media types such as `image` only match the corresponding `image/*` key.
pub fn is_url_supported(
    media_type: &str,
    url: &str,
    supported_urls: &LanguageModelSupportedUrls,
) -> bool {
    let media_type = media_type.to_lowercase();
    let url = url.to_lowercase();
    let is_top_level_only = !media_type.contains('/');

    supported_urls
        .iter()
        .flat_map(|(supported_media_type, patterns)| {
            let supported_media_type = supported_media_type.to_lowercase();
            let media_type_prefix = if supported_media_type == "*" || supported_media_type == "*/*"
            {
                String::new()
            } else {
                supported_media_type.replacen('*', "", 1)
            };

            let media_type_matches = if media_type_prefix.is_empty() {
                true
            } else if is_top_level_only {
                format!("{media_type}/") == media_type_prefix
            } else {
                media_type.starts_with(&media_type_prefix)
            };

            media_type_matches.then_some(patterns).into_iter().flatten()
        })
        .any(|pattern| {
            regex::Regex::new(pattern)
                .map(|regex| regex.is_match(&url))
                .unwrap_or(false)
        })
}

/// Reads response body chunks with a maximum size limit.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `readResponseWithSizeLimit`:
/// a parseable `Content-Length` header is checked before reading chunks, streamed
/// bytes are checked as they are accumulated, and limit violations return a
/// [`DownloadError`] with the upstream message shape.
pub fn read_response_with_size_limit<I, C>(
    url: &str,
    chunks: I,
    content_length: Option<&str>,
    max_bytes: Option<usize>,
) -> Result<Vec<u8>, DownloadError>
where
    I: IntoIterator<Item = C>,
    C: AsRef<[u8]>,
{
    let max_bytes = max_bytes.unwrap_or(DEFAULT_MAX_DOWNLOAD_SIZE);

    if let Some(content_length) = content_length.and_then(parse_content_length_header)
        && content_length > max_bytes as u128
    {
        return Err(DownloadError::new(
            url,
            format!(
                "Download of {url} exceeded maximum size of {max_bytes} bytes (Content-Length: {content_length})."
            ),
        ));
    }

    let mut response_body = Vec::new();
    let mut total_bytes = 0usize;

    for chunk in chunks {
        let chunk = chunk.as_ref();
        total_bytes = total_bytes.checked_add(chunk.len()).ok_or_else(|| {
            DownloadError::new(
                url,
                format!("Download of {url} exceeded maximum size of {max_bytes} bytes."),
            )
        })?;

        if total_bytes > max_bytes {
            return Err(DownloadError::new(
                url,
                format!("Download of {url} exceeded maximum size of {max_bytes} bytes."),
            ));
        }

        response_body.extend_from_slice(chunk);
    }

    Ok(response_body)
}

/// Downloads a URL into a dependency-free blob through an injected transport.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `downloadBlob`: the initial
/// URL is SSRF-validated before calling the transport, redirected final URLs are
/// validated when present, non-2xx responses become [`DownloadError`], the
/// response body is read through [`read_response_with_size_limit`], and the
/// `content-type` header becomes the returned blob media type.
pub async fn download_blob<Transport, TransportFuture>(
    options: DownloadBlobOptions,
    transport: Transport,
) -> Result<DownloadedBlob, DownloadError>
where
    Transport: FnOnce(&str) -> TransportFuture,
    TransportFuture: Future<Output = Result<DownloadBlobResponse, DownloadError>>,
{
    let DownloadBlobOptions { url, max_bytes } = options;

    validate_download_url(&url)?;

    let response = transport(&url).await?;

    if let Some(final_url) = response.final_url.as_deref() {
        validate_download_url(final_url)?;
    }

    if !response.is_success_status() {
        return Err(DownloadError::with_status(
            url,
            response.status_code,
            response.status_text,
        ));
    }

    let content_length = header_value(&response.headers, "content-length");
    let response_body =
        read_response_with_size_limit(&url, response.body.as_deref(), content_length, max_bytes)?;

    let mut blob = DownloadedBlob::new(response_body);

    if let Some(media_type) = header_value(&response.headers, "content-type") {
        blob = blob.with_media_type(media_type);
    }

    Ok(blob)
}

fn parse_content_length_header(content_length: &str) -> Option<u128> {
    let content_length = content_length.trim_start();
    let content_length = content_length.strip_prefix('+').unwrap_or(content_length);

    if content_length.starts_with('-') {
        return None;
    }

    let mut digits = content_length.bytes().take_while(u8::is_ascii_digit);
    let first_digit = digits.next()?;
    let mut length = u128::from(first_digit - b'0');

    for digit in digits {
        length = length
            .saturating_mul(10)
            .saturating_add(u128::from(digit - b'0'));
    }

    Some(length)
}

fn header_value<'a>(headers: &'a Headers, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Converts an image model file into a URL or data URI string.
///
/// This mirrors upstream `@ai-sdk/provider-utils`
/// `convertImageModelFileToDataUri`: URL files are returned as-is, base64 file
/// data is embedded directly, and raw bytes are base64-encoded into a data URI.
pub fn convert_image_model_file_to_data_uri(file: &ImageModelFile) -> String {
    match file {
        ImageModelFile::Url { url, .. } => url.as_str().to_string(),
        ImageModelFile::File {
            media_type, data, ..
        } => {
            let base64 = match data {
                FileDataContent::Bytes(bytes) => encode_base64(bytes),
                FileDataContent::Base64(base64) => base64.clone(),
            };

            format!("data:{media_type};base64,{base64}")
        }
    }
}

/// Validates that a URL is safe to download from.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `validateDownloadUrl`:
/// `http`, `https`, and `data` URLs are accepted, while local protocols,
/// localhost-style hostnames, and private IPv4/IPv6 addresses are rejected to
/// avoid accidental internal network access.
pub fn validate_download_url(url: &str) -> Result<(), DownloadError> {
    let parsed =
        Url::parse(url).map_err(|_| DownloadError::new(url, format!("Invalid URL: {url}")))?;

    match parsed.scheme() {
        "data" => return Ok(()),
        "http" | "https" => {}
        scheme => {
            return Err(DownloadError::new(
                url,
                format!("URL scheme must be http, https, or data, got {scheme}:"),
            ));
        }
    }

    let host = parsed
        .host()
        .ok_or_else(|| DownloadError::new(url, "URL must have a hostname"))?;

    match host {
        Host::Domain(hostname) => validate_download_hostname(url, hostname),
        Host::Ipv4(ip) => validate_download_ipv4(url, ip),
        Host::Ipv6(ip) => validate_download_ipv6(url, ip),
    }
}

fn validate_download_hostname(url: &str, hostname: &str) -> Result<(), DownloadError> {
    let hostname = hostname.to_ascii_lowercase();

    if hostname == "localhost" || hostname.ends_with(".local") || hostname.ends_with(".localhost") {
        return Err(DownloadError::new(
            url,
            format!("URL with hostname {hostname} is not allowed"),
        ));
    }

    Ok(())
}

fn validate_download_ipv4(url: &str, ip: Ipv4Addr) -> Result<(), DownloadError> {
    if is_private_download_ipv4(ip) {
        Err(DownloadError::new(
            url,
            format!("URL with IP address {ip} is not allowed"),
        ))
    } else {
        Ok(())
    }
}

fn validate_download_ipv6(url: &str, ip: Ipv6Addr) -> Result<(), DownloadError> {
    if is_private_download_ipv6(ip) {
        Err(DownloadError::new(
            url,
            format!("URL with IPv6 address [{ip}] is not allowed"),
        ))
    } else {
        Ok(())
    }
}

fn is_private_download_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();

    a == 0
        || a == 10
        || a == 127
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
}

fn is_private_download_ipv6(ip: Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() {
        return true;
    }

    if let Some(mapped_ipv4) = ip.to_ipv4_mapped() {
        return is_private_download_ipv4(mapped_ipv4);
    }

    let segments = ip.segments();
    let first_segment = segments[0];

    (first_segment & 0xfe00) == 0xfc00 || (first_segment & 0xffc0) == 0xfe80
}

/// Extracts HTTP response headers into the shared header record shape.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `extractResponseHeaders` by
/// turning iterable response header entries into a plain key-value record. Header
/// names and values are preserved as supplied by the response implementation.
pub fn extract_response_headers<K, V, I>(headers: I) -> Headers
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    headers
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect()
}

/// Creates an API-call error result from a failed HTTP status response.
///
/// This mirrors upstream `createStatusCodeErrorResponseHandler`: it uses the
/// response status text as the error message, preserves request body values,
/// status, headers, and raw response body, and returns the extracted headers
/// beside the constructed [`ApiCallError`].
pub fn create_status_code_error_response_handler(
    options: StatusCodeErrorResponseHandlerOptions,
) -> ResponseHandlerResult<ApiCallError> {
    let StatusCodeErrorResponseHandlerOptions {
        url,
        request_body_values,
        status_code,
        status_text,
        response_headers,
        response_body,
    } = options;

    let error = ApiCallError::new(status_text, url, request_body_values)
        .with_status_code(status_code)
        .with_response_headers(response_headers.clone())
        .with_response_body(response_body);

    ResponseHandlerResult::new(error).with_response_headers(response_headers)
}

/// Parses a failed JSON response body into an API-call error when possible.
///
/// This mirrors upstream `createJsonErrorResponseHandler`: empty bodies and
/// malformed JSON error payloads fall back to the response status text, while a
/// valid parsed error payload drives the error message and is preserved as
/// [`ApiCallError::data`]. The retry override closure returns `Some(bool)` to
/// replace the upstream status-code default or `None` to keep it.
pub fn create_json_error_response_handler<T, F, E, M, S, R>(
    options: JsonErrorResponseHandlerOptions,
    validate: F,
    error_to_message: M,
    is_retryable: R,
) -> ResponseHandlerResult<ApiCallError>
where
    T: Serialize,
    F: FnOnce(&JsonValue) -> Result<T, E>,
    E: fmt::Display,
    M: FnOnce(&T) -> S,
    S: Into<String>,
    R: FnOnce(u16, Option<&T>) -> Option<bool>,
{
    if options.response_body.trim().is_empty() {
        let retry_override = is_retryable(options.status_code, None);
        let message = options.status_text.clone();
        return json_error_response_result(options, message, None, retry_override);
    }

    let raw_value = match safe_parse_json(&options.response_body) {
        ParseJsonResult::Success { raw_value, .. } => raw_value,
        ParseJsonResult::Failure { .. } => {
            let retry_override = is_retryable(options.status_code, None);
            let message = options.status_text.clone();
            return json_error_response_result(options, message, None, retry_override);
        }
    };

    match safe_validate_types_with(raw_value, validate, None) {
        ValidateTypesResult::Success {
            value: parsed_error,
            ..
        } => match serde_json::to_value(&parsed_error) {
            Ok(data) => {
                let retry_override = is_retryable(options.status_code, Some(&parsed_error));
                let message = error_to_message(&parsed_error).into();
                json_error_response_result(options, message, Some(data), retry_override)
            }
            Err(_) => {
                let retry_override = is_retryable(options.status_code, None);
                let message = options.status_text.clone();
                json_error_response_result(options, message, None, retry_override)
            }
        },
        ValidateTypesResult::Failure { .. } => {
            let retry_override = is_retryable(options.status_code, None);
            let message = options.status_text.clone();
            json_error_response_result(options, message, None, retry_override)
        }
    }
}

fn json_error_response_result(
    options: JsonErrorResponseHandlerOptions,
    message: String,
    data: Option<JsonValue>,
    retry_override: Option<bool>,
) -> ResponseHandlerResult<ApiCallError> {
    let JsonErrorResponseHandlerOptions {
        url,
        request_body_values,
        status_code,
        response_headers,
        response_body,
        ..
    } = options;

    let mut error = ApiCallError::new(message, url, request_body_values)
        .with_status_code(status_code)
        .with_response_headers(response_headers.clone())
        .with_response_body(response_body);

    if let Some(data) = data {
        error = error.with_data(data);
    }

    if let Some(is_retryable) = retry_override {
        error = error.with_is_retryable(is_retryable);
    }

    ResponseHandlerResult::new(error).with_response_headers(response_headers)
}

/// Parses a JSON event stream into parsed JSON results.
///
/// This mirrors upstream `parseJsonEventStream`: event-source `data:` payloads
/// are parsed independently, `[DONE]` payloads are ignored, and parse or
/// validation failures are surfaced as safe parse results instead of panicking.
pub fn parse_json_event_stream<T, F, E, B>(
    chunks: impl IntoIterator<Item = B>,
    validate: F,
) -> Vec<ParseJsonResult<T>>
where
    F: Fn(&JsonValue) -> Result<T, E>,
    E: fmt::Display,
    B: AsRef<[u8]>,
{
    let mut bytes = Vec::new();
    for chunk in chunks {
        bytes.extend_from_slice(chunk.as_ref());
    }
    let text = String::from_utf8_lossy(&bytes);

    parse_event_source_data_events(&text)
        .into_iter()
        .filter(|data| data != "[DONE]")
        .map(|data| parse_json_event_data(&data, &validate))
        .collect()
}

fn parse_event_source_data_events(text: &str) -> Vec<String> {
    let mut events = Vec::new();
    let mut data_lines = Vec::new();

    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);

        if line.is_empty() {
            push_event_source_data_event(&mut events, &mut data_lines);
            continue;
        }

        if line.starts_with(':') {
            continue;
        }

        let (field, value) = line.split_once(':').map_or((line, ""), |(field, value)| {
            (field, value.strip_prefix(' ').unwrap_or(value))
        });

        if field == "data" {
            data_lines.push(value.to_string());
        }
    }

    push_event_source_data_event(&mut events, &mut data_lines);
    events
}

fn push_event_source_data_event(events: &mut Vec<String>, data_lines: &mut Vec<String>) {
    if data_lines.is_empty() {
        return;
    }

    events.push(data_lines.join("\n"));
    data_lines.clear();
}

fn parse_json_event_data<T, F, E>(data: &str, validate: &F) -> ParseJsonResult<T>
where
    F: Fn(&JsonValue) -> Result<T, E>,
    E: fmt::Display,
{
    let raw_value = match safe_parse_json(data) {
        ParseJsonResult::Success { raw_value, .. } => raw_value,
        ParseJsonResult::Failure { error, .. } => return ParseJsonResult::failure(error, None),
    };

    match safe_validate_types_with(raw_value.clone(), |value| validate(value), None) {
        ValidateTypesResult::Success { value, raw_value } => {
            ParseJsonResult::success(value, raw_value)
        }
        ValidateTypesResult::Failure { error, raw_value } => {
            ParseJsonResult::failure(error, Some(raw_value))
        }
    }
}

/// Parses a successful event-source response body into JSON parse results.
///
/// This mirrors upstream `createEventSourceResponseHandler`: a missing response
/// body throws [`EmptyResponseBodyError`], while a present body is parsed into
/// safe per-event JSON results and returned with extracted response headers.
pub fn create_event_source_response_handler<T, F, E>(
    options: EventSourceResponseHandlerOptions,
    validate: F,
) -> Result<ResponseHandlerResult<Vec<ParseJsonResult<T>>>, EmptyResponseBodyError>
where
    F: Fn(&JsonValue) -> Result<T, E>,
    E: fmt::Display,
{
    let EventSourceResponseHandlerOptions {
        response_headers,
        response_body,
    } = options;

    let Some(response_body) = response_body else {
        return Err(EmptyResponseBodyError::new());
    };

    Ok(
        ResponseHandlerResult::new(parse_json_event_stream([response_body], validate))
            .with_response_headers(response_headers),
    )
}

/// Parses and validates a successful JSON response body.
///
/// This mirrors upstream `createJsonResponseHandler`: the returned handler
/// result contains the validated value, the raw parsed JSON value, and response
/// headers. JSON parse or validation failures become an [`ApiCallError`] with
/// the upstream `Invalid JSON response` message and the original response
/// context.
pub fn create_json_response_handler<T, F, E>(
    options: JsonResponseHandlerOptions,
    validate: F,
) -> Result<ResponseHandlerResult<T>, Box<ApiCallError>>
where
    F: FnOnce(&JsonValue) -> Result<T, E>,
    E: fmt::Display,
{
    let JsonResponseHandlerOptions {
        url,
        request_body_values,
        status_code,
        response_headers,
        response_body,
    } = options;

    let raw_value = match safe_parse_json(&response_body) {
        ParseJsonResult::Success { raw_value, .. } => raw_value,
        ParseJsonResult::Failure { .. } => {
            return Err(Box::new(invalid_json_response_error(
                url,
                request_body_values,
                status_code,
                response_headers,
                response_body,
            )));
        }
    };

    match safe_validate_types_with(raw_value.clone(), validate, None) {
        ValidateTypesResult::Success { value, raw_value } => Ok(ResponseHandlerResult::new(value)
            .with_raw_value(raw_value)
            .with_response_headers(response_headers)),
        ValidateTypesResult::Failure { .. } => Err(Box::new(invalid_json_response_error(
            url,
            request_body_values,
            status_code,
            response_headers,
            response_body,
        ))),
    }
}

fn invalid_json_response_error(
    url: String,
    request_body_values: JsonValue,
    status_code: u16,
    response_headers: Headers,
    response_body: String,
) -> ApiCallError {
    ApiCallError::new("Invalid JSON response", url, request_body_values)
        .with_status_code(status_code)
        .with_response_headers(response_headers)
        .with_response_body(response_body)
}

/// Returns a successful binary response body.
///
/// This mirrors upstream `createBinaryResponseHandler`: the returned handler
/// result contains the response bytes and headers. A missing response body
/// becomes an [`ApiCallError`] with the upstream `Response body is empty`
/// message and original response context.
pub fn create_binary_response_handler(
    options: BinaryResponseHandlerOptions,
) -> Result<ResponseHandlerResult<Vec<u8>>, Box<ApiCallError>> {
    let BinaryResponseHandlerOptions {
        url,
        request_body_values,
        status_code,
        response_headers,
        response_body,
    } = options;

    match response_body {
        Some(response_body) => {
            Ok(ResponseHandlerResult::new(response_body).with_response_headers(response_headers))
        }
        None => Err(Box::new(
            ApiCallError::new("Response body is empty", url, request_body_values)
                .with_status_code(status_code)
                .with_response_headers(response_headers),
        )),
    }
}

/// Handles a prepared provider API response using success and failure handlers.
///
/// This mirrors the response-processing branch shared by upstream `getFromApi`
/// and `postToApi`: unsuccessful HTTP statuses run the failed-response handler
/// and return its API error, successful statuses run the successful-response
/// handler, and non-API-call handler failures are wrapped in an upstream-shaped
/// [`ApiCallError`] with the response status and headers.
pub fn handle_provider_api_response<T, S, F>(
    request: &ProviderApiRequest,
    response: &ProviderApiResponse,
    successful_response_handler: S,
    failed_response_handler: F,
) -> Result<ResponseHandlerResult<T>, Box<ApiCallError>>
where
    S: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<T>, ProviderApiResponseHandlerError>,
    F: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<ApiCallError>, ProviderApiResponseHandlerError>,
{
    if !response.is_success_status() {
        return match failed_response_handler(request, response) {
            Ok(error_information) => Err(Box::new(error_information.into_value())),
            Err(error) => Err(provider_api_response_handler_error(
                error,
                "Failed to process error response",
                request,
                response,
            )),
        };
    }

    match successful_response_handler(request, response) {
        Ok(result) => Ok(result),
        Err(error) => Err(provider_api_response_handler_error(
            error,
            "Failed to process successful response",
            request,
            response,
        )),
    }
}

fn provider_api_abort_error() -> FetchErrorInfo {
    FetchErrorInfo::new("Aborted").with_name("AbortError")
}

async fn await_provider_api_transport<TransportFuture>(
    future: TransportFuture,
    abort_signal: Option<LanguageModelAbortSignal>,
) -> Result<ProviderApiResponse, FetchErrorInfo>
where
    TransportFuture: Future<Output = Result<ProviderApiResponse, FetchErrorInfo>>,
{
    let Some(abort_signal) = abort_signal else {
        return future.await;
    };

    let mut future = Box::pin(future);

    std::future::poll_fn(move |context| {
        if abort_signal.poll_aborted(context).is_ready() {
            return Poll::Ready(Err(provider_api_abort_error()));
        }

        future.as_mut().poll(context)
    })
    .await
}

/// Executes a prepared provider API request through a caller-supplied transport.
///
/// This is the dependency-free orchestration boundary for upstream
/// `getFromApi` and `postToApi`: HTTP adapters send the prepared request and
/// return a [`ProviderApiResponse`], response statuses are dispatched through
/// [`handle_provider_api_response`], and transport failures are normalized with
/// [`handle_fetch_error`].
pub async fn execute_provider_api_request<T, Transport, TransportFuture, S, F>(
    request: ProviderApiRequest,
    transport: Transport,
    successful_response_handler: S,
    failed_response_handler: F,
) -> Result<ResponseHandlerResult<T>, HandledFetchError>
where
    Transport: FnOnce(ProviderApiRequest) -> TransportFuture,
    TransportFuture: Future<Output = Result<ProviderApiResponse, FetchErrorInfo>>,
    S: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<T>, ProviderApiResponseHandlerError>,
    F: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<ApiCallError>, ProviderApiResponseHandlerError>,
{
    if request
        .abort_signal
        .as_ref()
        .is_some_and(LanguageModelAbortSignal::is_aborted)
    {
        return Err(handle_fetch_error(
            provider_api_abort_error(),
            request.url,
            request.request_body_values,
        ));
    }

    let response = match await_provider_api_transport(
        transport(request.clone()),
        request.abort_signal.clone(),
    )
    .await
    {
        Ok(response) => response,
        Err(error) => {
            return Err(handle_fetch_error(
                error,
                request.url,
                request.request_body_values,
            ));
        }
    };

    handle_provider_api_response(
        &request,
        &response,
        successful_response_handler,
        failed_response_handler,
    )
    .map_err(|error| HandledFetchError::ApiCall { error })
}

/// Runs an upstream-style `getFromApi` request through an injected transport.
///
/// This is the public dependency-free orchestration wrapper for upstream
/// `getFromApi`: request metadata is prepared from [`GetFromApiOptions`], the
/// caller-supplied transport performs the HTTP work, and response handling plus
/// fetch-error normalization are delegated to [`execute_provider_api_request`].
pub async fn get_from_api<T, Transport, TransportFuture, S, F>(
    options: GetFromApiOptions,
    transport: Transport,
    successful_response_handler: S,
    failed_response_handler: F,
) -> Result<ResponseHandlerResult<T>, HandledFetchError>
where
    Transport: FnOnce(ProviderApiRequest) -> TransportFuture,
    TransportFuture: Future<Output = Result<ProviderApiResponse, FetchErrorInfo>>,
    S: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<T>, ProviderApiResponseHandlerError>,
    F: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<ApiCallError>, ProviderApiResponseHandlerError>,
{
    execute_provider_api_request(
        options.into_request(),
        transport,
        successful_response_handler,
        failed_response_handler,
    )
    .await
}

/// Runs an upstream-style `postJsonToApi` request through an injected transport.
///
/// This is the public dependency-free orchestration wrapper for upstream
/// `postJsonToApi`: JSON request metadata is prepared from
/// [`PostJsonToApiOptions`], the caller-supplied transport performs the HTTP
/// work, and response handling plus fetch-error normalization are delegated to
/// [`execute_provider_api_request`].
pub async fn post_json_to_api<T, Transport, TransportFuture, S, F>(
    options: PostJsonToApiOptions,
    transport: Transport,
    successful_response_handler: S,
    failed_response_handler: F,
) -> Result<ResponseHandlerResult<T>, HandledFetchError>
where
    Transport: FnOnce(ProviderApiRequest) -> TransportFuture,
    TransportFuture: Future<Output = Result<ProviderApiResponse, FetchErrorInfo>>,
    S: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<T>, ProviderApiResponseHandlerError>,
    F: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<ApiCallError>, ProviderApiResponseHandlerError>,
{
    execute_provider_api_request(
        options.into_request(),
        transport,
        successful_response_handler,
        failed_response_handler,
    )
    .await
}

/// Runs an upstream-style `postFormDataToApi` request through an injected transport.
///
/// This is the public dependency-free orchestration wrapper for upstream
/// `postFormDataToApi`: form-data request metadata is prepared from
/// [`PostFormDataToApiOptions`], the caller-supplied transport performs the
/// HTTP work, and response handling plus fetch-error normalization are
/// delegated to [`execute_provider_api_request`].
pub async fn post_form_data_to_api<T, Transport, TransportFuture, S, F>(
    options: PostFormDataToApiOptions,
    transport: Transport,
    successful_response_handler: S,
    failed_response_handler: F,
) -> Result<ResponseHandlerResult<T>, HandledFetchError>
where
    Transport: FnOnce(ProviderApiRequest) -> TransportFuture,
    TransportFuture: Future<Output = Result<ProviderApiResponse, FetchErrorInfo>>,
    S: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<T>, ProviderApiResponseHandlerError>,
    F: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<ApiCallError>, ProviderApiResponseHandlerError>,
{
    execute_provider_api_request(
        options.into_request(),
        transport,
        successful_response_handler,
        failed_response_handler,
    )
    .await
}

/// Runs an upstream-style `postToApi` request through an injected transport.
///
/// This is the public dependency-free orchestration wrapper for upstream
/// `postToApi`: POST request metadata is prepared from [`PostToApiOptions`],
/// the caller-supplied transport performs the HTTP work, and response handling
/// plus fetch-error normalization are delegated to
/// [`execute_provider_api_request`].
pub async fn post_to_api<T, Transport, TransportFuture, S, F>(
    options: PostToApiOptions,
    transport: Transport,
    successful_response_handler: S,
    failed_response_handler: F,
) -> Result<ResponseHandlerResult<T>, HandledFetchError>
where
    Transport: FnOnce(ProviderApiRequest) -> TransportFuture,
    TransportFuture: Future<Output = Result<ProviderApiResponse, FetchErrorInfo>>,
    S: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<T>, ProviderApiResponseHandlerError>,
    F: FnOnce(
        &ProviderApiRequest,
        &ProviderApiResponse,
    ) -> Result<ResponseHandlerResult<ApiCallError>, ProviderApiResponseHandlerError>,
{
    execute_provider_api_request(
        options.into_request(),
        transport,
        successful_response_handler,
        failed_response_handler,
    )
    .await
}

fn provider_api_response_handler_error(
    error: ProviderApiResponseHandlerError,
    message: &'static str,
    request: &ProviderApiRequest,
    response: &ProviderApiResponse,
) -> Box<ApiCallError> {
    match error {
        ProviderApiResponseHandlerError::ApiCall { error } => error,
        ProviderApiResponseHandlerError::Other { .. } => Box::new(
            ApiCallError::new(
                message,
                request.url.clone(),
                request.request_body_values.clone(),
            )
            .with_status_code(response.status_code)
            .with_response_headers(response.headers.clone()),
        ),
    }
}

/// Combines optional HTTP header maps, with later maps overriding earlier ones.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `combineHeaders`: missing
/// maps are ignored, header names are preserved as supplied, and missing values
/// are retained so a later `None` can intentionally override an earlier value.
pub fn combine_headers<K, V, I, H>(headers: H) -> BTreeMap<String, Option<String>>
where
    H: IntoIterator<Item = Option<I>>,
    I: IntoIterator<Item = (K, Option<V>)>,
    K: Into<String>,
    V: Into<String>,
{
    let mut combined_headers = BTreeMap::new();

    for current_headers in headers.into_iter().flatten() {
        for (key, value) in current_headers {
            combined_headers.insert(key.into(), value.map(Into::into));
        }
    }

    combined_headers
}

/// Normalizes optional HTTP header entries into a lower-case header map.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `normalizeHeaders`: missing
/// input becomes an empty map, nullish values are removed, and header names are
/// normalized to lower case.
pub fn normalize_headers<K, V, I>(headers: Option<I>) -> Headers
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: AsRef<str>,
    V: Into<String>,
{
    let Some(headers) = headers else {
        return Headers::new();
    };

    headers
        .into_iter()
        .filter_map(|(key, value)| {
            value.map(|value| (key.as_ref().to_ascii_lowercase(), value.into()))
        })
        .collect()
}

/// Appends suffix parts to the normalized `user-agent` header.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `withUserAgentSuffix`: input
/// headers are normalized first, missing header values are removed, and empty
/// user-agent parts are skipped before joining with spaces.
pub fn with_user_agent_suffix<K, V, I, S, P>(
    headers: Option<I>,
    user_agent_suffix_parts: P,
) -> Headers
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: AsRef<str>,
    V: Into<String>,
    P: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut headers = normalize_headers(headers);
    let current_user_agent = headers.get("user-agent").map(String::as_str).unwrap_or("");

    let mut user_agent_parts = Vec::new();

    if !current_user_agent.is_empty() {
        user_agent_parts.push(current_user_agent.to_string());
    }

    for part in user_agent_suffix_parts {
        let part = part.as_ref();
        if !part.is_empty() {
            user_agent_parts.push(part.to_string());
        }
    }

    let user_agent = user_agent_parts.join(" ");

    headers.insert("user-agent".to_string(), user_agent);
    headers
}

/// Appends the provider-utils package and runtime user-agent suffixes to headers.
///
/// This is the Rust-native request-header preparation shared by upstream
/// `getFromApi` and `postToApi`: callers supply their provider headers and an
/// explicit runtime environment, and the result is normalized with
/// `ai-sdk/provider-utils/{VERSION}` plus the upstream runtime suffix.
pub fn with_provider_utils_user_agent<K, V, I>(
    headers: Option<I>,
    environment: &RuntimeEnvironment,
) -> Headers
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: AsRef<str>,
    V: Into<String>,
{
    with_user_agent_suffix(
        headers,
        [
            format!("ai-sdk/provider-utils/{}", crate::VERSION),
            get_runtime_environment_user_agent(environment),
        ],
    )
}

/// Prepares the request metadata used by upstream `getFromApi`.
///
/// The returned request has method `GET`, normalized provider-utils user-agent
/// headers, no body, and empty `requestBodyValues`.
pub fn prepare_get_from_api_request<K, V, I>(
    url: impl Into<String>,
    headers: Option<I>,
    environment: &RuntimeEnvironment,
) -> ProviderApiRequest
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: AsRef<str>,
    V: Into<String>,
{
    ProviderApiRequest::get(url, with_provider_utils_user_agent(headers, environment))
}

/// Prepares the request metadata used by upstream `postToApi`.
///
/// Upstream `postToApi` applies the shared provider-utils user-agent headers,
/// sends the supplied body content, and preserves separate body values for
/// response-handler `requestBodyValues`. The Rust boundary supports text and
/// byte bodies and leaves JavaScript-only `FormData` handling to HTTP adapters.
pub fn prepare_post_to_api_request<K, V, I>(
    url: impl Into<String>,
    headers: Option<I>,
    body: ProviderApiRequestBody,
    request_body_values: impl Into<JsonValue>,
    environment: &RuntimeEnvironment,
) -> ProviderApiRequest
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: AsRef<str>,
    V: Into<String>,
{
    ProviderApiRequest::post(
        url,
        with_provider_utils_user_agent(headers, environment),
        body,
        request_body_values,
    )
}

/// Prepares the JSON request metadata used by upstream `postJsonToApi`.
///
/// Upstream `postJsonToApi` adds `Content-Type: application/json`, allows caller
/// headers to override it, stringifies the JSON body for `body.content`, and
/// preserves the original body value for response-handler `requestBodyValues`.
pub fn prepare_post_json_to_api_request<K, V, I>(
    url: impl Into<String>,
    headers: Option<I>,
    body: impl Into<JsonValue>,
    environment: &RuntimeEnvironment,
) -> ProviderApiRequest
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: Into<String>,
    V: Into<String>,
{
    let body_values = body.into();
    let body_content = body_values.to_string();
    let mut combined_headers = BTreeMap::from([(
        "Content-Type".to_string(),
        Some("application/json".to_string()),
    )]);

    if let Some(headers) = headers {
        for (key, value) in headers {
            combined_headers.insert(key.into(), value.map(Into::into));
        }
    }

    prepare_post_to_api_request(
        url,
        Some(combined_headers),
        ProviderApiRequestBody::text(body_content),
        body_values,
        environment,
    )
}

/// Prepares the form-data request metadata used by upstream `postFormDataToApi`.
///
/// Upstream `postFormDataToApi` sends the form data directly and preserves
/// `Object.fromEntries(formData.entries())` as response-handler
/// `requestBodyValues`. Rust keeps the dependency-free [`FormData`] body and
/// converts text entries to JSON strings plus byte entries to byte arrays for
/// the JSON request-body-values boundary.
pub fn prepare_post_form_data_to_api_request<K, V, I>(
    url: impl Into<String>,
    headers: Option<I>,
    form_data: FormData,
    environment: &RuntimeEnvironment,
) -> ProviderApiRequest
where
    I: IntoIterator<Item = (K, Option<V>)>,
    K: AsRef<str>,
    V: Into<String>,
{
    let request_body_values = form_data_request_body_values(&form_data);

    prepare_post_to_api_request(
        url,
        headers,
        ProviderApiRequestBody::form_data(form_data),
        request_body_values,
        environment,
    )
}

fn form_data_request_body_values(form_data: &FormData) -> JsonValue {
    let mut values = JsonObject::new();

    for entry in &form_data.entries {
        values.insert(
            entry.name.clone(),
            form_data_value_to_request_body_value(&entry.value),
        );
    }

    JsonValue::Object(values)
}

fn form_data_value_to_request_body_value(value: &FormDataValue) -> JsonValue {
    match value {
        FormDataValue::Text { value } => JsonValue::String(value.clone()),
        FormDataValue::Bytes { value } => JsonValue::Array(
            value
                .iter()
                .copied()
                .map(JsonValue::from)
                .collect::<Vec<_>>(),
        ),
    }
}

/// Returns an upstream-style runtime user-agent suffix for provider utilities.
///
/// This mirrors upstream `getRuntimeEnvironmentUserAgent`: browser indicators
/// win first, navigator user agents are lowercased, Node.js versions are
/// included as supplied, Vercel Edge is detected next, and unknown runtimes use
/// the upstream fallback string.
pub fn get_runtime_environment_user_agent(environment: &RuntimeEnvironment) -> String {
    if environment.has_window {
        return "runtime/browser".to_string();
    }

    if let Some(user_agent) = environment
        .navigator_user_agent
        .as_deref()
        .filter(|user_agent| !user_agent.is_empty())
    {
        return format!("runtime/{}", user_agent.to_ascii_lowercase());
    }

    if let Some(version) = environment
        .node_version
        .as_deref()
        .filter(|version| !version.is_empty())
    {
        return format!("runtime/node.js/{version}");
    }

    if environment.has_edge_runtime {
        return "runtime/vercel-edge".to_string();
    }

    "runtime/unknown".to_string()
}

/// Returns whether an error name represents an aborted request.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `isAbortError`: JavaScript
/// checks `Error` and `DOMException` instances by their `name` field, while the
/// Rust boundary accepts that runtime-specific name directly.
pub fn is_abort_error(error_name: &str) -> bool {
    matches!(
        error_name,
        "AbortError" | "ResponseAborted" | "TimeoutError"
    )
}

/// Normalizes lower-level fetch/network errors for provider API helpers.
///
/// This mirrors upstream internal `handleFetchError`: abort-style errors are
/// returned unchanged, recognized fetch connection failures become retryable
/// [`ApiCallError`] values, and unknown errors are propagated unchanged.
pub fn handle_fetch_error(
    error: FetchErrorInfo,
    url: impl Into<String>,
    request_body_values: impl Into<JsonValue>,
) -> HandledFetchError {
    if error.name.as_deref().is_some_and(is_abort_error) {
        return HandledFetchError::Original { error };
    }

    if error.name.as_deref() == Some("TypeError")
        && FETCH_FAILED_ERROR_MESSAGES.contains(&error.message.to_lowercase().as_str())
        && let Some(cause_message) = error.cause_message.as_deref()
    {
        return HandledFetchError::ApiCall {
            error: Box::new(
                ApiCallError::new(
                    format!("Cannot connect to API: {cause_message}"),
                    url,
                    request_body_values,
                )
                .with_is_retryable(true),
            ),
        };
    }

    if error
        .code
        .as_deref()
        .is_some_and(|code| BUN_NETWORK_ERROR_CODES.contains(&code))
    {
        return HandledFetchError::ApiCall {
            error: Box::new(
                ApiCallError::new(
                    format!("Cannot connect to API: {}", error.message),
                    url,
                    request_body_values,
                )
                .with_is_retryable(true),
            ),
        };
    }

    HandledFetchError::Original { error }
}

/// Options for loading a provider API key from an explicit value or environment variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadApiKeyOptions {
    /// Explicit API key value. When present, it is returned without reading the environment.
    pub api_key: Option<String>,

    /// Environment variable to read when `api_key` is not provided.
    pub environment_variable_name: String,

    /// Parameter name used in missing-key error messages.
    pub api_key_parameter_name: String,

    /// Human-readable provider or API description used in error messages.
    pub description: String,
}

impl LoadApiKeyOptions {
    /// Creates API key loading options with the upstream default `apiKey` parameter name.
    pub fn new(
        environment_variable_name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            api_key: None,
            environment_variable_name: environment_variable_name.into(),
            api_key_parameter_name: "apiKey".to_string(),
            description: description.into(),
        }
    }

    /// Sets the explicit API key value.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Sets the parameter name used in missing-key error messages.
    pub fn with_api_key_parameter_name(
        mut self,
        api_key_parameter_name: impl Into<String>,
    ) -> Self {
        self.api_key_parameter_name = api_key_parameter_name.into();
        self
    }
}

/// Loads a provider API key from an explicit value or environment variable.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `loadApiKey` for Rust callers:
/// typed explicit values win, missing values read the named environment variable,
/// and missing or non-Unicode environment values produce `LoadApiKeyError`.
pub fn load_api_key(options: LoadApiKeyOptions) -> Result<String, LoadApiKeyError> {
    load_api_key_with_env(options, |name| env::var(name))
}

fn load_api_key_with_env(
    options: LoadApiKeyOptions,
    load_env: impl FnOnce(&str) -> Result<String, VarError>,
) -> Result<String, LoadApiKeyError> {
    if let Some(api_key) = options.api_key {
        return Ok(api_key);
    }

    match load_env(&options.environment_variable_name) {
        Ok(api_key) => Ok(api_key),
        Err(VarError::NotPresent) => Err(LoadApiKeyError::new(format!(
            "{} API key is missing. Pass it using the '{}' parameter or the {} environment variable.",
            options.description, options.api_key_parameter_name, options.environment_variable_name
        ))),
        Err(VarError::NotUnicode(_)) => Err(LoadApiKeyError::new(format!(
            "{} API key must be a string. The value of the {} environment variable is not a string.",
            options.description, options.environment_variable_name
        ))),
    }
}

/// Options for loading a provider setting from an explicit value or environment variable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadSettingOptions {
    /// Explicit setting value. When present, it is returned without reading the environment.
    pub setting_value: Option<String>,

    /// Environment variable to read when `setting_value` is not provided.
    pub environment_variable_name: String,

    /// Parameter name used in missing-setting error messages.
    pub setting_name: String,

    /// Human-readable setting description used in error messages.
    pub description: String,
}

impl LoadSettingOptions {
    /// Creates setting loading options.
    pub fn new(
        environment_variable_name: impl Into<String>,
        setting_name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            setting_value: None,
            environment_variable_name: environment_variable_name.into(),
            setting_name: setting_name.into(),
            description: description.into(),
        }
    }

    /// Sets the explicit setting value.
    pub fn with_setting_value(mut self, setting_value: impl Into<String>) -> Self {
        self.setting_value = Some(setting_value.into());
        self
    }
}

/// Loads a required string setting from an explicit value or environment variable.
pub fn load_setting(options: LoadSettingOptions) -> Result<String, LoadSettingError> {
    load_setting_with_env(options, |name| env::var(name))
}

fn load_setting_with_env(
    options: LoadSettingOptions,
    load_env: impl FnOnce(&str) -> Result<String, VarError>,
) -> Result<String, LoadSettingError> {
    if let Some(setting_value) = options.setting_value {
        return Ok(setting_value);
    }

    match load_env(&options.environment_variable_name) {
        Ok(setting_value) => Ok(setting_value),
        Err(VarError::NotPresent) => Err(LoadSettingError::new(format!(
            "{} setting is missing. Pass it using the '{}' parameter or the {} environment variable.",
            options.description, options.setting_name, options.environment_variable_name
        ))),
        Err(VarError::NotUnicode(_)) => Err(LoadSettingError::new(format!(
            "{} setting must be a string. The value of the {} environment variable is not a string.",
            options.description, options.environment_variable_name
        ))),
    }
}

/// Options for loading an optional provider setting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadOptionalSettingOptions {
    /// Explicit setting value. When present, it is returned without reading the environment.
    pub setting_value: Option<String>,

    /// Environment variable to read when `setting_value` is not provided.
    pub environment_variable_name: String,
}

impl LoadOptionalSettingOptions {
    /// Creates optional setting loading options.
    pub fn new(environment_variable_name: impl Into<String>) -> Self {
        Self {
            setting_value: None,
            environment_variable_name: environment_variable_name.into(),
        }
    }

    /// Sets the explicit setting value.
    pub fn with_setting_value(mut self, setting_value: impl Into<String>) -> Self {
        self.setting_value = Some(setting_value.into());
        self
    }
}

/// Loads an optional setting from an explicit value or environment variable.
pub fn load_optional_setting(options: LoadOptionalSettingOptions) -> Option<String> {
    load_optional_setting_with_env(options, |name| env::var(name))
}

fn load_optional_setting_with_env(
    options: LoadOptionalSettingOptions,
    load_env: impl FnOnce(&str) -> Result<String, VarError>,
) -> Option<String> {
    if let Some(setting_value) = options.setting_value {
        return Some(setting_value);
    }

    load_env(&options.environment_variable_name).ok()
}

/// Maps a media type to the file extension used by upstream provider uploads.
pub fn media_type_to_extension(media_type: &str) -> String {
    let subtype = media_type
        .split_once('/')
        .map_or("", |(_, subtype)| subtype)
        .to_ascii_lowercase();

    match subtype.as_str() {
        "mpeg" => "mp3".to_string(),
        "x-wav" => "wav".to_string(),
        "opus" => "ogg".to_string(),
        "mp4" | "x-m4a" => "m4a".to_string(),
        _ => subtype,
    }
}

/// Strips all file extension segments from a filename.
pub fn strip_file_extension(filename: &str) -> &str {
    filename
        .find('.')
        .map_or(filename, |first_dot_index| &filename[..first_dot_index])
}

/// Removes exactly one trailing slash from a URL-like string when present.
pub fn without_trailing_slash(url: Option<&str>) -> Option<&str> {
    url.map(|url| url.strip_suffix('/').unwrap_or(url))
}

/// Resolves a provider reference to the provider-specific identifier.
///
/// This mirrors upstream `@ai-sdk/provider-utils` `resolveProviderReference`
/// while reusing the crate's shared provider-reference contract.
pub fn resolve_provider_reference<'a>(
    reference: &'a ProviderReference,
    provider: &str,
) -> Result<&'a str, NoSuchProviderReferenceError> {
    reference.provider_id(provider)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, VecDeque};
    use std::env::VarError;
    use std::ffi::OsString;
    use std::future::{Future, ready};
    use std::pin::Pin;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use std::task::{Context, Poll, Waker};
    use std::thread;
    use std::time::{Duration, Instant};

    use ai_sdk_provider::language_model::{
        LanguageModelAbortController, LanguageModelAssistantContentPart,
        LanguageModelAssistantMessage, LanguageModelFileData, LanguageModelFilePart,
        LanguageModelFunctionTool, LanguageModelMessage, LanguageModelProviderTool,
        LanguageModelReasoningEffort, LanguageModelStreamPart, LanguageModelSystemMessage,
        LanguageModelTextPart, LanguageModelTool, LanguageModelToolApprovalRequestPart,
        LanguageModelToolApprovalResponsePart, LanguageModelToolInputExample,
        LanguageModelToolResultOutput, LanguageModelUserContentPart, LanguageModelUserMessage,
    };
    use ai_sdk_provider::{
        ApiCallError, FileData, FileDataContent, ImageModelFile, JsonObject, JsonSchema, JsonValue,
        ProviderMetadata, ProviderReference, TypeValidationContext, TypeValidationError, Warning,
    };
    use serde_json::json;
    use url::Url;

    use super::{
        Arrayable, AsyncIteratorReadableStreamNextFuture, AsyncIteratorReadableStreamReturnFuture,
        AsyncIteratorReadableStreamSource, Base64DecodeError, BinaryResponseHandlerOptions,
        ConvertToFormDataOptions, DEFAULT_MAX_DOWNLOAD_SIZE, DelayError, DelayOptions,
        DelayedPromise, DownloadBlobOptions, DownloadBlobResponse, DownloadError, DownloadedBlob,
        EventSourceResponseHandlerOptions, ExecuteToolOutput, ExperimentalSandbox, FetchErrorInfo,
        FilePart, FilePartData, FlexibleSchema, FormData, FormDataEntry, FormDataInputValue,
        FormDataValue, GetFromApiOptions, HandledFetchError, IdGeneratorOptions,
        InjectJsonInstructionIntoMessagesOptions, InlineFileDataBytesError,
        JsonErrorResponseHandlerOptions, JsonResponseHandlerOptions, JsonSerializableValue,
        LazySchema, LoadApiKeyOptions, LoadOptionalSettingOptions, LoadSettingOptions,
        ParseJsonError, ParseJsonResult, PostFormDataToApiOptions, PostJsonToApiOptions,
        PostToApiOptions, ProviderApiRequest, ProviderApiRequestBody, ProviderApiRequestMethod,
        ProviderApiResponse, ProviderApiResponseBody, ProviderApiResponseHandlerError,
        ProviderDefinedToolFactory, ProviderExecutedToolFactory, ReasoningFilePart,
        ReasoningFilePartData, ReasoningLevel, Resolvable, ResponseHandlerResult,
        RuntimeEnvironment, SandboxCommandOptions, SandboxCommandResult, SandboxRunCommandFuture,
        Schema, SerializedModelOptions, StandardSchema, StandardSchemaJsonSchemaOptions,
        StatusCodeErrorResponseHandlerOptions, StreamingToolCallDelta,
        StreamingToolCallDeltaFunction, StreamingToolCallTracker, StreamingToolCallTrackerOptions,
        StreamingToolCallTypeValidation, Tool, ToolApprovalRequest, ToolApprovalResponse, ToolCall,
        ToolDescriptionOptions, ToolExecuteFunction, ToolExecutionError, ToolExecutionOptions,
        ToolInputAvailableOptions, ToolInputDeltaOptions, ToolModelOutputOptions,
        ToolNeedsApprovalFunction, ToolNeedsApprovalOptions, ToolResult, ToolResultContentPart,
        ToolResultOutput, ValidateTypesResult, ValidationResult,
        add_additional_properties_to_json_schema, as_array, as_flexible_schema, as_schema,
        combine_headers, convert_async_iterator_to_readable_stream, convert_base64_to_bytes,
        convert_bytes_to_base64, convert_image_model_file_to_data_uri,
        convert_inline_file_data_to_bytes, convert_to_base64, convert_to_form_data,
        create_binary_response_handler, create_event_source_response_handler, create_id_generator,
        create_json_error_response_handler, create_json_response_handler,
        create_provider_defined_tool_factory,
        create_provider_defined_tool_factory_with_output_schema,
        create_provider_executed_tool_factory, create_status_code_error_response_handler,
        create_tool_name_mapping, delay, delay_with_options, detect_media_type, download_blob,
        dynamic_tool, execute_provider_api_request, execute_tool, extract_response_headers,
        filter_nullable, generate_id, get_error_message, get_from_api,
        get_runtime_environment_user_agent, get_top_level_media_type, handle_fetch_error,
        handle_provider_api_response, inject_json_instruction,
        inject_json_instruction_into_messages, is_abort_error, is_custom_reasoning,
        is_executable_tool, is_full_media_type, is_json_serializable, is_non_nullable,
        is_parsable_json, is_provider_reference, is_url_supported, json_schema, lazy_json_schema,
        lazy_schema, load_api_key, load_api_key_with_env, load_optional_setting_with_env,
        load_setting, load_setting_with_env, map_reasoning_to_provider_budget,
        map_reasoning_to_provider_effort, media_type_to_extension, normalize_headers, parse_json,
        parse_json_event_stream, parse_json_with_schema, parse_provider_options,
        post_form_data_to_api, post_json_to_api, post_to_api, prepare_get_from_api_request,
        prepare_post_form_data_to_api_request, prepare_post_json_to_api_request,
        prepare_post_to_api_request, prepare_tools, prepare_tools_with_context,
        read_response_with_size_limit, remove_undefined_entries, resolve, resolve_full_media_type,
        resolve_provider_reference, safe_parse_json, safe_parse_json_with_schema,
        safe_validate_types, secure_json_parse, serialize_model_options, standard_schema,
        strip_file_extension, tool, validate_download_url, validate_types,
        with_provider_utils_user_agent, with_user_agent_suffix, without_trailing_slash,
    };

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Pin::new(&mut future).poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => unreachable!("test futures should be ready"),
        }
    }

    fn test_provider_reference(entries: &[(&str, &str)]) -> ProviderReference {
        ProviderReference::try_from(
            entries
                .iter()
                .map(|(provider, id)| ((*provider).to_string(), (*id).to_string()))
                .collect::<BTreeMap<_, _>>(),
        )
        .expect("provider reference is valid")
    }

    fn tagged_file_data_type(data: &FileData) -> &'static str {
        match data {
            FileData::Data { data } => {
                let _: &FileDataContent = data;
                "data"
            }
            FileData::Url { url } => {
                let _: &Url = url;
                "url"
            }
            FileData::Reference { reference } => {
                let _: &ProviderReference = reference;
                "reference"
            }
            FileData::Text { text } => {
                let _: &String = text;
                "text"
            }
        }
    }

    fn tagged_reasoning_file_data_type(data: &LanguageModelFileData) -> &'static str {
        match data {
            LanguageModelFileData::Data { data } => {
                let _: &FileDataContent = data;
                "data"
            }
            LanguageModelFileData::Url { url } => {
                let _: &Url = url;
                "url"
            }
        }
    }

    #[derive(Debug, Default)]
    struct ScriptedReadableStreamIterator {
        values: VecDeque<i32>,
        next_calls: usize,
        return_calls: usize,
        return_reasons: Vec<Option<String>>,
        finally_called: bool,
        return_error_message: Option<String>,
    }

    impl ScriptedReadableStreamIterator {
        fn with_values(values: impl IntoIterator<Item = i32>) -> Self {
            Self {
                values: values.into_iter().collect(),
                ..Self::default()
            }
        }

        fn with_return_error(mut self, message: impl Into<String>) -> Self {
            self.return_error_message = Some(message.into());
            self
        }
    }

    impl AsyncIteratorReadableStreamSource<i32> for ScriptedReadableStreamIterator {
        type Error = String;

        fn next_item(&mut self) -> AsyncIteratorReadableStreamNextFuture<'_, i32, Self::Error> {
            self.next_calls += 1;
            Box::pin(ready(Ok(self.values.pop_front())))
        }

        fn return_iterator<'a>(
            &'a mut self,
            reason: Option<&'a str>,
        ) -> AsyncIteratorReadableStreamReturnFuture<'a, Self::Error> {
            self.return_calls += 1;
            self.return_reasons.push(reason.map(str::to_string));
            self.finally_called = true;

            Box::pin(ready(self.return_error_message.clone().map_or(Ok(()), Err)))
        }
    }

    #[derive(Debug, Default)]
    struct NextOnlyReadableStreamIterator {
        value: Option<i32>,
        next_calls: usize,
    }

    impl NextOnlyReadableStreamIterator {
        fn new(value: i32) -> Self {
            Self {
                value: Some(value),
                next_calls: 0,
            }
        }
    }

    impl AsyncIteratorReadableStreamSource<i32> for NextOnlyReadableStreamIterator {
        type Error = String;

        fn next_item(&mut self) -> AsyncIteratorReadableStreamNextFuture<'_, i32, Self::Error> {
            self.next_calls += 1;
            Box::pin(ready(Ok(self.value.take())))
        }
    }

    fn header_map<const N: usize>(entries: [(&str, &str); N]) -> BTreeMap<String, String> {
        entries
            .into_iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    fn supported_urls(entries: Vec<(&str, Vec<&str>)>) -> BTreeMap<String, Vec<String>> {
        entries
            .into_iter()
            .map(|(media_type, patterns)| {
                (
                    media_type.to_string(),
                    patterns.into_iter().map(str::to_string).collect(),
                )
            })
            .collect()
    }

    fn poll_until_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        loop {
            match Pin::new(&mut future).poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    fn poll_once<F: Future>(mut future: Pin<&mut F>) -> Poll<F::Output> {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);

        Future::poll(future.as_mut(), &mut context)
    }

    fn poll_pinned_until_ready<F: Future>(mut future: Pin<&mut F>, timeout: Duration) -> F::Output {
        let start = Instant::now();

        loop {
            if let Poll::Ready(value) = poll_once(future.as_mut()) {
                return value;
            }

            assert!(
                start.elapsed() < timeout,
                "future did not become ready within {timeout:?}"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[derive(Debug)]
    struct StaticSandbox {
        description: String,
    }

    impl StaticSandbox {
        fn new(description: impl Into<String>) -> Self {
            Self {
                description: description.into(),
            }
        }
    }

    impl ExperimentalSandbox for StaticSandbox {
        fn description(&self) -> &str {
            &self.description
        }

        fn run_command(&self, options: SandboxCommandOptions) -> SandboxRunCommandFuture {
            Box::pin(ready(
                SandboxCommandResult::new(0).with_stdout(options.command),
            ))
        }
    }

    #[test]
    fn provider_utils_reexports_provider_get_error_message() {
        assert_eq!(get_error_message(None), "unknown error");
        assert_eq!(
            get_error_message(Some(&"provider-utils failure")),
            "provider-utils failure"
        );
        assert_eq!(
            get_error_message(Some(&json!({ "code": "bad_request" }))),
            "{\"code\":\"bad_request\"}"
        );
    }

    #[test]
    fn convert_async_iterator_stream_upstream_calls_return_on_cancel_and_triggers_finally() {
        let iterator = ScriptedReadableStreamIterator::with_values([0, 1]);
        let mut stream = convert_async_iterator_to_readable_stream(iterator);

        assert_eq!(
            poll_ready(stream.read()).expect("first read succeeds"),
            Some(0)
        );

        poll_ready(stream.cancel(Some("stop")));

        assert!(stream.is_cancelled());
        assert!(stream.is_closed());
        assert_eq!(stream.iterator().return_calls, 1);
        assert_eq!(
            stream.iterator().return_reasons,
            vec![Some("stop".to_string())]
        );
        assert!(stream.iterator().finally_called);
    }

    #[test]
    fn convert_async_iterator_stream_upstream_stops_reads_after_cancel() {
        let iterator = ScriptedReadableStreamIterator::with_values([0, 1]);
        let mut stream = convert_async_iterator_to_readable_stream(iterator);

        assert_eq!(
            poll_ready(stream.read()).expect("first read succeeds"),
            Some(0)
        );

        poll_ready(stream.cancel(Some("stop")));

        assert_eq!(
            poll_ready(stream.read()).expect("cancelled stream closes"),
            None
        );
        assert_eq!(stream.iterator().next_calls, 1);
    }

    #[test]
    fn convert_async_iterator_stream_upstream_cancels_without_return_method() {
        let iterator = NextOnlyReadableStreamIterator::new(42);
        let mut stream = convert_async_iterator_to_readable_stream(iterator);

        assert_eq!(
            poll_ready(stream.read()).expect("first read succeeds"),
            Some(42)
        );

        poll_ready(stream.cancel(None));

        assert!(stream.is_cancelled());
        assert!(stream.is_closed());
        assert_eq!(stream.iterator().next_calls, 1);
    }

    #[test]
    fn convert_async_iterator_stream_upstream_ignores_return_errors() {
        let iterator =
            ScriptedReadableStreamIterator::with_values([1]).with_return_error("return() failed");
        let mut stream = convert_async_iterator_to_readable_stream(iterator);

        assert_eq!(
            poll_ready(stream.read()).expect("first read succeeds"),
            Some(1)
        );

        poll_ready(stream.cancel(None));

        assert!(stream.is_cancelled());
        assert!(stream.is_closed());
        assert_eq!(stream.iterator().return_calls, 1);
        assert_eq!(
            stream.iterator().return_error_message.as_deref(),
            Some("return() failed")
        );
    }

    #[test]
    fn tool_call_serializes_upstream_provider_utils_shape() {
        let tool_call = ToolCall::new("call-1", "weather", json!({ "city": "Brisbane" }))
            .with_provider_executed(true)
            .with_dynamic(false);

        assert_eq!(
            serde_json::to_value(&tool_call).expect("tool call serializes"),
            json!({
                "toolCallId": "call-1",
                "toolName": "weather",
                "input": {
                    "city": "Brisbane"
                },
                "providerExecuted": true,
                "dynamic": false
            })
        );
        assert_eq!(
            serde_json::from_value::<ToolCall>(json!({
                "toolCallId": "call-1",
                "toolName": "weather",
                "input": {
                    "city": "Brisbane"
                },
                "providerExecuted": true,
                "dynamic": false
            }))
            .expect("tool call deserializes"),
            tool_call
        );
        assert_eq!(
            serde_json::to_value(ToolCall::new("call-2", "search", json!({ "q": "rust" })))
                .expect("minimal tool call serializes"),
            json!({
                "toolCallId": "call-2",
                "toolName": "search",
                "input": {
                    "q": "rust"
                }
            })
        );
    }

    #[test]
    fn tool_result_serializes_upstream_provider_utils_shape() {
        let tool_result = ToolResult::new(
            "call-1",
            "weather",
            json!({ "city": "Brisbane" }),
            json!({ "temperature": 24 }),
        )
        .with_provider_executed(false)
        .with_dynamic(true);

        assert_eq!(
            serde_json::to_value(&tool_result).expect("tool result serializes"),
            json!({
                "toolCallId": "call-1",
                "toolName": "weather",
                "input": {
                    "city": "Brisbane"
                },
                "output": {
                    "temperature": 24
                },
                "providerExecuted": false,
                "dynamic": true
            })
        );
        assert_eq!(
            serde_json::from_value::<ToolResult>(json!({
                "toolCallId": "call-1",
                "toolName": "weather",
                "input": {
                    "city": "Brisbane"
                },
                "output": {
                    "temperature": 24
                },
                "providerExecuted": false,
                "dynamic": true
            }))
            .expect("tool result deserializes"),
            tool_result
        );
        assert_eq!(
            serde_json::to_value(ToolResult::new(
                "call-2",
                "search",
                json!({ "q": "rust" }),
                json!(["result"])
            ))
            .expect("minimal tool result serializes"),
            json!({
                "toolCallId": "call-2",
                "toolName": "search",
                "input": {
                    "q": "rust"
                },
                "output": ["result"]
            })
        );
    }

    #[test]
    fn content_part_file_part_data_narrows_exhaustively_across_the_4_tagged_arms() {
        let reference = test_provider_reference(&[("openai", "file-abc")]);
        let data = vec![
            FileData::Data {
                data: FileDataContent::Bytes(vec![1, 2, 3]),
            },
            FileData::Url {
                url: Url::parse("https://example.com/file.png").expect("valid URL"),
            },
            FileData::Reference { reference },
            FileData::Text {
                text: "inline text".to_string(),
            },
        ];

        assert_eq!(
            data.iter().map(tagged_file_data_type).collect::<Vec<_>>(),
            vec!["data", "url", "reference", "text"]
        );
    }

    #[test]
    fn content_part_file_part_data_exposes_exactly_4_tagged_type_discriminants() {
        let reference = test_provider_reference(&[("openai", "file-abc")]);
        let data = [
            FileData::Data {
                data: FileDataContent::Base64("aGVsbG8=".to_string()),
            },
            FileData::Url {
                url: Url::parse("https://example.com/file.png").expect("valid URL"),
            },
            FileData::Reference { reference },
            FileData::Text {
                text: "inline text".to_string(),
            },
        ];

        assert_eq!(
            data.iter().map(tagged_file_data_type).collect::<Vec<_>>(),
            vec!["data", "url", "reference", "text"]
        );
    }

    #[test]
    fn content_part_file_part_accepts_the_tagged_data_arm() {
        let part = FilePart::new(
            FileData::Data {
                data: FileDataContent::Bytes(vec![1, 2, 3]),
            },
            "application/octet-stream",
        );

        assert_eq!(
            serde_json::to_value(part).expect("file part serializes"),
            json!({
                "type": "file",
                "data": {
                    "type": "data",
                    "data": [1, 2, 3]
                },
                "mediaType": "application/octet-stream"
            })
        );
    }

    #[test]
    fn content_part_file_part_accepts_the_tagged_url_arm() {
        let part = FilePart::new(
            FileData::Url {
                url: Url::parse("https://example.com/file.png").expect("valid URL"),
            },
            "image/png",
        );

        assert_eq!(
            serde_json::to_value(part).expect("file part serializes"),
            json!({
                "type": "file",
                "data": {
                    "type": "url",
                    "url": "https://example.com/file.png"
                },
                "mediaType": "image/png"
            })
        );
    }

    #[test]
    fn content_part_file_part_accepts_the_tagged_reference_arm() {
        let part = FilePart::new(
            FileData::Reference {
                reference: test_provider_reference(&[("openai", "file-abc")]),
            },
            "application/octet-stream",
        );

        assert_eq!(
            serde_json::to_value(part).expect("file part serializes"),
            json!({
                "type": "file",
                "data": {
                    "type": "reference",
                    "reference": {
                        "openai": "file-abc"
                    }
                },
                "mediaType": "application/octet-stream"
            })
        );
    }

    #[test]
    fn content_part_file_part_accepts_the_tagged_text_arm() {
        let part = FilePart::new(
            FileData::Text {
                text: "inline text".to_string(),
            },
            "text/plain",
        );

        assert_eq!(
            serde_json::to_value(part).expect("file part serializes"),
            json!({
                "type": "file",
                "data": {
                    "type": "text",
                    "text": "inline text"
                },
                "mediaType": "text/plain"
            })
        );
    }

    #[test]
    fn content_part_file_part_also_accepts_bare_data_content_url_and_provider_reference_shorthands()
    {
        let data_part = FilePart::new(FileDataContent::Bytes(vec![1, 2, 3]), "image/png");
        assert!(matches!(
            data_part.data,
            FilePartData::Data(FileDataContent::Bytes(bytes)) if bytes == vec![1, 2, 3]
        ));

        let base64_part = FilePart::new("aGVsbG8=", "image/png");
        assert!(matches!(
            base64_part.data,
            FilePartData::Data(FileDataContent::Base64(base64)) if base64 == "aGVsbG8="
        ));

        let url = Url::parse("https://example.com/file.png").expect("valid URL");
        let url_part = FilePart::new(url.clone(), "image/png");
        assert!(matches!(url_part.data, FilePartData::Url(part_url) if part_url == url));

        let reference = test_provider_reference(&[("openai", "file-abc")]);
        let reference_part = FilePart::new(reference.clone(), "application/octet-stream");
        assert!(matches!(
            reference_part.data,
            FilePartData::Reference(part_reference) if part_reference == reference
        ));
    }

    #[test]
    fn content_part_reasoning_file_part_data_narrows_exhaustively_across_the_2_tagged_arms() {
        let data = [
            LanguageModelFileData::Data {
                data: FileDataContent::Bytes(vec![1, 2, 3]),
            },
            LanguageModelFileData::Url {
                url: Url::parse("https://example.com/reasoning.pdf").expect("valid URL"),
            },
        ];

        assert_eq!(
            data.iter()
                .map(tagged_reasoning_file_data_type)
                .collect::<Vec<_>>(),
            vec!["data", "url"]
        );
    }

    #[test]
    fn content_part_reasoning_file_part_data_exposes_exactly_2_tagged_type_discriminants() {
        let data = [
            LanguageModelFileData::Data {
                data: FileDataContent::Base64("cmVhc29uaW5n".to_string()),
            },
            LanguageModelFileData::Url {
                url: Url::parse("https://example.com/reasoning.pdf").expect("valid URL"),
            },
        ];

        assert_eq!(
            data.iter()
                .map(tagged_reasoning_file_data_type)
                .collect::<Vec<_>>(),
            vec!["data", "url"]
        );
    }

    #[test]
    fn content_part_reasoning_file_part_accepts_the_tagged_data_arm() {
        let part = ReasoningFilePart::new(
            LanguageModelFileData::Data {
                data: FileDataContent::Bytes(vec![4, 5, 6]),
            },
            "application/pdf",
        );

        assert_eq!(
            serde_json::to_value(part).expect("reasoning file part serializes"),
            json!({
                "type": "reasoning-file",
                "data": {
                    "type": "data",
                    "data": [4, 5, 6]
                },
                "mediaType": "application/pdf"
            })
        );
    }

    #[test]
    fn content_part_reasoning_file_part_accepts_the_tagged_url_arm() {
        let part = ReasoningFilePart::new(
            LanguageModelFileData::Url {
                url: Url::parse("https://example.com/reasoning.pdf").expect("valid URL"),
            },
            "application/pdf",
        );

        assert_eq!(
            serde_json::to_value(part).expect("reasoning file part serializes"),
            json!({
                "type": "reasoning-file",
                "data": {
                    "type": "url",
                    "url": "https://example.com/reasoning.pdf"
                },
                "mediaType": "application/pdf"
            })
        );
    }

    #[test]
    fn content_part_reasoning_file_part_also_accepts_bare_data_content_and_url_shorthands() {
        let data_part = ReasoningFilePart::new(FileDataContent::Bytes(vec![1, 2, 3]), "image/png");
        assert!(matches!(
            data_part.data,
            ReasoningFilePartData::Data(FileDataContent::Bytes(bytes)) if bytes == vec![1, 2, 3]
        ));

        let url = Url::parse("https://example.com/reasoning.pdf").expect("valid URL");
        let url_part = ReasoningFilePart::new(url.clone(), "application/pdf");
        assert!(matches!(url_part.data, ReasoningFilePartData::Url(part_url) if part_url == url));
    }

    #[test]
    fn content_part_tool_result_file_variant_accepts_the_tagged_data_arm_with_a_full_media_type() {
        let part = ToolResultContentPart::File {
            data: FileData::Data {
                data: FileDataContent::Bytes(vec![137, 80, 78, 71]),
            },
            media_type: "image/png".to_string(),
            filename: None,
            provider_options: None,
        };

        assert_eq!(
            serde_json::to_value(part).expect("tool result content part serializes"),
            json!({
                "type": "file",
                "data": {
                    "type": "data",
                    "data": [137, 80, 78, 71]
                },
                "mediaType": "image/png"
            })
        );
    }

    #[test]
    fn content_part_tool_result_file_variant_accepts_the_tagged_url_arm_with_top_level_media_type()
    {
        let part = ToolResultContentPart::File {
            data: FileData::Url {
                url: Url::parse("https://example.com/image.png").expect("valid URL"),
            },
            media_type: "image".to_string(),
            filename: None,
            provider_options: None,
        };

        assert_eq!(
            serde_json::to_value(part).expect("tool result content part serializes"),
            json!({
                "type": "file",
                "data": {
                    "type": "url",
                    "url": "https://example.com/image.png"
                },
                "mediaType": "image"
            })
        );
    }

    #[test]
    fn content_part_tool_result_file_variant_accepts_the_tagged_reference_arm() {
        let part = ToolResultContentPart::File {
            data: FileData::Reference {
                reference: test_provider_reference(&[("openai", "file-abc")]),
            },
            media_type: "application/octet-stream".to_string(),
            filename: None,
            provider_options: None,
        };

        assert_eq!(
            serde_json::to_value(part).expect("tool result content part serializes"),
            json!({
                "type": "file",
                "data": {
                    "type": "reference",
                    "reference": {
                        "openai": "file-abc"
                    }
                },
                "mediaType": "application/octet-stream"
            })
        );
    }

    #[test]
    fn content_part_tool_result_file_variant_accepts_the_tagged_text_arm() {
        let part = ToolResultContentPart::File {
            data: FileData::Text {
                text: "inline text".to_string(),
            },
            media_type: "text/plain".to_string(),
            filename: None,
            provider_options: None,
        };

        assert_eq!(
            serde_json::to_value(part).expect("tool result content part serializes"),
            json!({
                "type": "file",
                "data": {
                    "type": "text",
                    "text": "inline text"
                },
                "mediaType": "text/plain"
            })
        );
    }

    #[test]
    fn content_part_tool_result_file_variant_rejects_bare_data_shorthands() {
        serde_json::from_value::<ToolResultContentPart>(json!({
            "type": "file",
            "data": [1, 2, 3],
            "mediaType": "image/png"
        }))
        .expect_err("bare data shorthand is rejected for tool-result file parts");

        serde_json::from_value::<ToolResultContentPart>(json!({
            "type": "file",
            "data": "https://example.com/image.png",
            "mediaType": "image/png"
        }))
        .expect_err("bare URL shorthand is rejected for tool-result file parts");
    }

    #[test]
    fn content_part_tool_result_file_variant_exposes_the_four_tagged_data_type_discriminants() {
        let data = [
            FileData::Data {
                data: FileDataContent::Base64("aW1hZ2U=".to_string()),
            },
            FileData::Url {
                url: Url::parse("https://example.com/image.png").expect("valid URL"),
            },
            FileData::Reference {
                reference: test_provider_reference(&[("openai", "file-abc")]),
            },
            FileData::Text {
                text: "inline text".to_string(),
            },
        ];

        assert_eq!(
            data.iter().map(tagged_file_data_type).collect::<Vec<_>>(),
            vec!["data", "url", "reference", "text"]
        );
    }

    #[test]
    fn content_part_tool_result_legacy_variants_still_type_checks_file_data() {
        let part = ToolResultContentPart::FileData {
            data: "aGVsbG8=".to_string(),
            media_type: "text/plain".to_string(),
            filename: None,
            provider_options: None,
        };

        assert_eq!(
            serde_json::to_value(part).expect("legacy file-data serializes"),
            json!({
                "type": "file-data",
                "data": "aGVsbG8=",
                "mediaType": "text/plain"
            })
        );
    }

    #[test]
    fn content_part_tool_result_legacy_variants_still_type_checks_file_url_with_media_type() {
        let part = ToolResultContentPart::FileUrl {
            url: "https://example.com/file.pdf".to_string(),
            media_type: Some("application/pdf".to_string()),
            provider_options: None,
        };

        assert_eq!(
            serde_json::to_value(part).expect("legacy file-url serializes"),
            json!({
                "type": "file-url",
                "url": "https://example.com/file.pdf",
                "mediaType": "application/pdf"
            })
        );
    }

    #[test]
    fn content_part_tool_result_legacy_variants_still_type_checks_file_url_without_media_type() {
        let part = ToolResultContentPart::FileUrl {
            url: "https://example.com/file.pdf".to_string(),
            media_type: None,
            provider_options: None,
        };

        assert_eq!(
            serde_json::to_value(part).expect("legacy file-url serializes"),
            json!({
                "type": "file-url",
                "url": "https://example.com/file.pdf"
            })
        );
    }

    #[test]
    fn content_part_tool_result_legacy_variants_still_type_checks_file_reference() {
        let part = ToolResultContentPart::FileReference {
            provider_reference: test_provider_reference(&[("openai", "file-abc")]),
            provider_options: None,
        };

        assert_eq!(
            serde_json::to_value(part).expect("legacy file-reference serializes"),
            json!({
                "type": "file-reference",
                "providerReference": {
                    "openai": "file-abc"
                }
            })
        );
    }

    #[test]
    fn content_part_tool_result_legacy_variants_still_type_checks_legacy_image_variants() {
        let image_data = ToolResultContentPart::ImageData {
            data: "iVBORw0KGgo=".to_string(),
            media_type: "image/png".to_string(),
            provider_options: None,
        };
        let image_url = ToolResultContentPart::ImageUrl {
            url: "https://example.com/image.png".to_string(),
            provider_options: None,
        };

        assert_eq!(
            serde_json::to_value(image_data).expect("legacy image-data serializes"),
            json!({
                "type": "image-data",
                "data": "iVBORw0KGgo=",
                "mediaType": "image/png"
            })
        );
        assert_eq!(
            serde_json::to_value(image_url).expect("legacy image-url serializes"),
            json!({
                "type": "image-url",
                "url": "https://example.com/image.png"
            })
        );
    }

    #[test]
    fn content_part_provider_reference_top_level_accepts_plain_provider_id_records() {
        assert_eq!(
            test_provider_reference(&[("openai", "file-abc")])
                .provider_id("openai")
                .expect("openai provider id exists"),
            "file-abc"
        );
        assert_eq!(
            test_provider_reference(&[("fileId", "abc")])
                .provider_id("fileId")
                .expect("fileId provider id exists"),
            "abc"
        );
    }

    #[test]
    fn content_part_provider_reference_top_level_rejects_records_that_carry_a_type_key() {
        let error = ProviderReference::try_from(BTreeMap::from([
            ("type".to_string(), "x".to_string()),
            ("openai".to_string(), "file-abc".to_string()),
        ]))
        .expect_err("provider reference with type key is rejected");

        assert_eq!(
            error.to_string(),
            "provider references cannot contain the reserved `type` key"
        );
    }

    #[test]
    fn content_part_tool_result_output_wraps_content_parts() {
        let output = ToolResultOutput::Content {
            value: vec![ToolResultContentPart::Text {
                text: "done".to_string(),
                provider_options: None,
            }],
        };

        assert_eq!(
            serde_json::to_value(output).expect("tool result output serializes"),
            json!({
                "type": "content",
                "value": [
                    {
                        "type": "text",
                        "text": "done"
                    }
                ]
            })
        );
    }

    #[test]
    fn tool_approval_request_serializes_upstream_prompt_part_shape() {
        let request = ToolApprovalRequest::new("approval-1", "call-1").with_automatic(true);

        assert_eq!(
            serde_json::to_value(&request).expect("tool approval request serializes"),
            json!({
                "type": "tool-approval-request",
                "approvalId": "approval-1",
                "toolCallId": "call-1",
                "isAutomatic": true
            })
        );
        assert_eq!(
            serde_json::from_value::<ToolApprovalRequest>(json!({
                "type": "tool-approval-request",
                "approvalId": "approval-1",
                "toolCallId": "call-1",
                "isAutomatic": true
            }))
            .expect("tool approval request deserializes"),
            request
        );
        assert_eq!(
            request.to_language_model_part(),
            LanguageModelToolApprovalRequestPart::new("approval-1", "call-1").with_automatic(true)
        );
        assert_eq!(
            serde_json::to_value(ToolApprovalRequest::new("approval-2", "call-2"))
                .expect("minimal request serializes"),
            json!({
                "type": "tool-approval-request",
                "approvalId": "approval-2",
                "toolCallId": "call-2"
            })
        );
    }

    #[test]
    fn tool_approval_response_serializes_upstream_prompt_part_shape() {
        let response = ToolApprovalResponse::new("approval-1", false)
            .with_reason("Requires billing access.")
            .with_provider_executed(true);

        assert_eq!(
            serde_json::to_value(&response).expect("tool approval response serializes"),
            json!({
                "type": "tool-approval-response",
                "approvalId": "approval-1",
                "approved": false,
                "reason": "Requires billing access.",
                "providerExecuted": true
            })
        );
        assert_eq!(
            serde_json::from_value::<ToolApprovalResponse>(json!({
                "type": "tool-approval-response",
                "approvalId": "approval-1",
                "approved": false,
                "reason": "Requires billing access.",
                "providerExecuted": true
            }))
            .expect("tool approval response deserializes"),
            response
        );
        assert_eq!(
            response.to_language_model_part(),
            LanguageModelToolApprovalResponsePart::new("approval-1", false)
                .with_reason("Requires billing access.")
        );
        assert_eq!(
            serde_json::to_value(ToolApprovalResponse::new("approval-2", true))
                .expect("minimal response serializes"),
            json!({
                "type": "tool-approval-response",
                "approvalId": "approval-2",
                "approved": true
            })
        );
    }

    #[test]
    fn resolve_returns_raw_values_and_future_values() {
        assert_eq!(poll_ready(resolve(Resolvable::value(42))), 42);
        assert_eq!(
            poll_ready(resolve(Resolvable::future(ready(json!({
                "foo": "bar"
            }))))),
            json!({
                "foo": "bar"
            })
        );
    }

    #[test]
    fn resolve_invokes_lazy_value_and_future_producers_on_demand() {
        let count = std::cell::Cell::new(0);
        let lazy_value = Resolvable::lazy_value(|| {
            count.set(count.get() + 1);
            count.get()
        });

        assert_eq!(count.get(), 0);
        assert_eq!(poll_ready(resolve(lazy_value)), 1);
        assert_eq!(count.get(), 1);

        let lazy_future = Resolvable::function(|| ready("resolved headers"));
        assert_eq!(poll_ready(resolve(lazy_future)), "resolved headers");
    }

    #[test]
    fn resolve_can_carry_result_outputs_for_fallible_values() {
        let success: Resolvable<'_, Result<&str, &str>> = Resolvable::future(ready(Ok("ok")));
        assert_eq!(poll_ready(resolve(success)), Ok("ok"));

        let failure: Resolvable<'_, Result<&str, &str>> =
            Resolvable::function(|| ready(Err("bad")));
        assert_eq!(poll_ready(resolve(failure)), Err("bad"));
    }

    #[test]
    fn resolve_upstream_should_resolve_raw_values() {
        let value: Resolvable<'_, i32> = Resolvable::value(42);

        assert_eq!(poll_ready(resolve(value)), 42);
    }

    #[test]
    fn resolve_upstream_should_resolve_raw_objects() {
        let value: Resolvable<'_, JsonValue> = Resolvable::value(json!({
            "foo": "bar"
        }));

        assert_eq!(
            poll_ready(resolve(value)),
            json!({
                "foo": "bar"
            })
        );
    }

    #[test]
    fn resolve_upstream_should_resolve_promises() {
        let value: Resolvable<'_, &str> = Resolvable::future(ready("hello"));

        assert_eq!(poll_ready(resolve(value)), "hello");
    }

    #[test]
    fn resolve_upstream_should_resolve_rejected_promises() {
        let value: Resolvable<'_, Result<&str, &str>> =
            Resolvable::future(ready(Err("test error")));

        assert_eq!(poll_ready(resolve(value)), Err("test error"));
    }

    #[test]
    fn resolve_upstream_should_resolve_synchronous_functions() {
        let value: Resolvable<'_, i32> = Resolvable::lazy_value(|| 42);

        assert_eq!(poll_ready(resolve(value)), 42);
    }

    #[test]
    fn resolve_upstream_should_resolve_synchronous_functions_returning_objects() {
        let value: Resolvable<'_, JsonValue> = Resolvable::lazy_value(|| {
            json!({
                "foo": "bar"
            })
        });

        assert_eq!(
            poll_ready(resolve(value)),
            json!({
                "foo": "bar"
            })
        );
    }

    #[test]
    fn resolve_upstream_should_resolve_async_functions() {
        let value: Resolvable<'_, &str> = Resolvable::function(|| async { "hello" });

        assert_eq!(poll_ready(resolve(value)), "hello");
    }

    #[test]
    fn resolve_upstream_should_resolve_async_functions_returning_promises() {
        let value: Resolvable<'_, i32> = Resolvable::function(|| ready(42));

        assert_eq!(poll_ready(resolve(value)), 42);
    }

    #[test]
    fn resolve_upstream_should_handle_async_function_rejections() {
        let value: Resolvable<'_, Result<&str, &str>> =
            Resolvable::function(|| ready(Err("async error")));

        assert_eq!(poll_ready(resolve(value)), Err("async error"));
    }

    #[test]
    fn resolve_upstream_should_handle_null() {
        let value: Resolvable<'_, JsonValue> = Resolvable::value(JsonValue::Null);

        assert_eq!(poll_ready(resolve(value)), JsonValue::Null);
    }

    #[test]
    fn resolve_upstream_should_handle_undefined() {
        let value: Resolvable<'_, Option<String>> = Resolvable::value(None);

        assert_eq!(poll_ready(resolve(value)), None);
    }

    #[test]
    fn resolve_upstream_should_resolve_nested_objects() {
        let value: Resolvable<'_, JsonValue> = Resolvable::value(json!({
            "nested": {
                "value": 42
            }
        }));

        assert_eq!(
            poll_ready(resolve(value)),
            json!({
                "nested": {
                    "value": 42
                }
            })
        );
    }

    #[test]
    fn resolve_headers_upstream_should_resolve_header_objects() {
        let headers = header_map([("Content-Type", "application/json")]);

        assert_eq!(
            poll_ready(resolve(Resolvable::value(headers.clone()))),
            headers
        );
    }

    #[test]
    fn resolve_headers_upstream_should_resolve_header_functions() {
        let value = Resolvable::lazy_value(|| header_map([("Authorization", "Bearer token")]));

        assert_eq!(
            poll_ready(resolve(value)),
            header_map([("Authorization", "Bearer token")])
        );
    }

    #[test]
    fn resolve_headers_upstream_should_resolve_async_header_functions() {
        let value = Resolvable::function(|| async { header_map([("X-Custom", "value")]) });

        assert_eq!(
            poll_ready(resolve(value)),
            header_map([("X-Custom", "value")])
        );
    }

    #[test]
    fn resolve_headers_upstream_should_resolve_header_promises() {
        let value = Resolvable::future(ready(header_map([("Accept", "application/json")])));

        assert_eq!(
            poll_ready(resolve(value)),
            header_map([("Accept", "application/json")])
        );
    }

    #[test]
    fn resolve_headers_upstream_reinvokes_async_header_function_each_time() {
        let counter = Arc::new(AtomicUsize::new(0));
        let make_headers = |counter: Arc<AtomicUsize>| {
            Resolvable::function(move || async move {
                let request_number = counter.fetch_add(1, Ordering::SeqCst) + 1;
                header_map([("X-Request-Number", &request_number.to_string())])
            })
        };

        assert_eq!(
            poll_ready(resolve(make_headers(Arc::clone(&counter)))),
            header_map([("X-Request-Number", "1")])
        );
        assert_eq!(
            poll_ready(resolve(make_headers(Arc::clone(&counter)))),
            header_map([("X-Request-Number", "2")])
        );
        assert_eq!(
            poll_ready(resolve(make_headers(counter))),
            header_map([("X-Request-Number", "3")])
        );
    }

    #[test]
    fn resolve_upstream_should_maintain_type_information() {
        struct User {
            id: u64,
            name: String,
        }

        let user_promise: Resolvable<'_, User> = Resolvable::future(ready(User {
            id: 1,
            name: "Test User".to_string(),
        }));

        let result = poll_ready(resolve(user_promise));

        assert_eq!(result.id, 1);
        assert_eq!(result.name, "Test User");
    }

    #[test]
    fn delayed_promise_starts_pending() {
        let delayed = DelayedPromise::<String>::new();

        assert!(delayed.is_pending());
        assert!(!delayed.is_resolved());
        assert!(!delayed.is_rejected());
    }

    #[test]
    fn delayed_promise_resolves_when_accessed_after_resolution() {
        let delayed = DelayedPromise::<String>::new();

        delayed.resolve("success".to_string());

        assert!(delayed.is_resolved());
        assert_eq!(poll_ready(delayed.promise()), Ok("success".to_string()));
    }

    #[test]
    fn delayed_promise_rejects_when_accessed_after_rejection() {
        let delayed = DelayedPromise::<String>::new();

        delayed.reject("failure".to_string());

        assert!(delayed.is_rejected());
        assert_eq!(poll_ready(delayed.promise()), Err("failure".to_string()));
    }

    #[test]
    fn delayed_promise_waits_until_resolved_when_accessed_first() {
        let delayed = DelayedPromise::<String>::new();
        let mut future = Box::pin(delayed.promise());

        assert!(matches!(poll_once(future.as_mut()), Poll::Pending));

        delayed.resolve("delayed-success".to_string());

        assert_eq!(
            poll_once(future.as_mut()),
            Poll::Ready(Ok("delayed-success".to_string()))
        );
    }

    #[test]
    fn delayed_promise_waits_until_rejected_when_accessed_first() {
        let delayed = DelayedPromise::<String>::new();
        let mut future = Box::pin(delayed.promise());

        assert!(matches!(poll_once(future.as_mut()), Poll::Pending));

        delayed.reject("delayed-failure".to_string());

        assert_eq!(
            poll_once(future.as_mut()),
            Poll::Ready(Err("delayed-failure".to_string()))
        );
    }

    #[test]
    fn delayed_promise_resolves_all_accessed_futures() {
        let delayed = DelayedPromise::<String>::new();
        let mut first = Box::pin(delayed.promise());
        let mut second = Box::pin(delayed.promise());

        assert!(matches!(poll_once(first.as_mut()), Poll::Pending));
        assert!(matches!(poll_once(second.as_mut()), Poll::Pending));

        delayed.resolve("success".to_string());

        assert_eq!(
            poll_once(first.as_mut()),
            Poll::Ready(Ok("success".to_string()))
        );
        assert_eq!(
            poll_once(second.as_mut()),
            Poll::Ready(Ok("success".to_string()))
        );
    }

    #[test]
    fn delayed_promise_accessed_future_keeps_first_settlement() {
        let delayed = DelayedPromise::<String>::new();
        let promise = delayed.promise();

        delayed.resolve("first".to_string());
        delayed.reject("second".to_string());

        assert!(delayed.is_rejected());
        assert_eq!(poll_ready(promise), Ok("first".to_string()));
        assert_eq!(poll_ready(delayed.promise()), Ok("first".to_string()));
    }

    #[test]
    fn delayed_promise_uses_latest_status_before_first_access() {
        let delayed = DelayedPromise::<String>::new();

        delayed.resolve("first".to_string());
        delayed.reject("second".to_string());

        assert!(delayed.is_rejected());
        assert_eq!(poll_ready(delayed.promise()), Err("second".to_string()));
    }

    #[test]
    fn delayed_promise_upstream_resolves_when_accessed_after_resolution() {
        let delayed = DelayedPromise::<String>::new();

        delayed.resolve("success".to_string());

        assert_eq!(poll_ready(delayed.promise()), Ok("success".to_string()));
    }

    #[test]
    fn delayed_promise_upstream_rejects_when_accessed_after_rejection() {
        let delayed = DelayedPromise::<String>::new();

        delayed.reject("failure".to_string());

        assert_eq!(poll_ready(delayed.promise()), Err("failure".to_string()));
    }

    #[test]
    fn delayed_promise_upstream_resolves_when_accessed_before_resolution() {
        let delayed = DelayedPromise::<String>::new();
        let promise = delayed.promise();

        delayed.resolve("success".to_string());

        assert_eq!(poll_ready(promise), Ok("success".to_string()));
    }

    #[test]
    fn delayed_promise_upstream_rejects_when_accessed_before_rejection() {
        let delayed = DelayedPromise::<String>::new();
        let promise = delayed.promise();

        delayed.reject("failure".to_string());

        assert_eq!(poll_ready(promise), Err("failure".to_string()));
    }

    #[test]
    fn delayed_promise_upstream_maintains_resolved_state_after_multiple_accesses() {
        let delayed = DelayedPromise::<String>::new();

        delayed.resolve("success".to_string());

        assert_eq!(poll_ready(delayed.promise()), Ok("success".to_string()));
        assert_eq!(poll_ready(delayed.promise()), Ok("success".to_string()));
    }

    #[test]
    fn delayed_promise_upstream_maintains_rejected_state_after_multiple_accesses() {
        let delayed = DelayedPromise::<String>::new();

        delayed.reject("failure".to_string());

        assert_eq!(poll_ready(delayed.promise()), Err("failure".to_string()));
        assert_eq!(poll_ready(delayed.promise()), Err("failure".to_string()));
    }

    #[test]
    fn delayed_promise_upstream_blocks_until_resolved_when_accessed_before_resolution() {
        let delayed = DelayedPromise::<String>::new();
        let mut promise = Box::pin(delayed.promise());

        assert!(matches!(poll_once(promise.as_mut()), Poll::Pending));

        delayed.resolve("delayed-success".to_string());

        assert_eq!(
            poll_once(promise.as_mut()),
            Poll::Ready(Ok("delayed-success".to_string()))
        );
    }

    #[test]
    fn delayed_promise_upstream_blocks_until_rejected_when_accessed_before_rejection() {
        let delayed = DelayedPromise::<String>::new();
        let mut promise = Box::pin(delayed.promise());

        assert!(matches!(poll_once(promise.as_mut()), Poll::Pending));

        delayed.reject("delayed-failure".to_string());

        assert_eq!(
            poll_once(promise.as_mut()),
            Poll::Ready(Err("delayed-failure".to_string()))
        );
    }

    #[test]
    fn delayed_promise_upstream_resolves_all_pending_promises_when_resolved_after_access() {
        let delayed = DelayedPromise::<String>::new();
        let mut first = Box::pin(delayed.promise());
        let mut second = Box::pin(delayed.promise());

        assert!(matches!(poll_once(first.as_mut()), Poll::Pending));
        assert!(matches!(poll_once(second.as_mut()), Poll::Pending));

        delayed.resolve("success".to_string());

        assert_eq!(
            poll_once(first.as_mut()),
            Poll::Ready(Ok("success".to_string()))
        );
        assert_eq!(
            poll_once(second.as_mut()),
            Poll::Ready(Ok("success".to_string()))
        );
    }

    #[test]
    fn delay_without_duration_resolves_immediately() {
        poll_ready(delay(None));
    }

    #[test]
    fn delay_with_duration_resolves_after_timer_completes() {
        let mut future = Box::pin(delay(Some(10)));

        assert!(matches!(poll_once(future.as_mut()), Poll::Pending));

        thread::sleep(Duration::from_millis(30));

        assert!(matches!(poll_once(future.as_mut()), Poll::Ready(())));
    }

    #[test]
    fn delay_zero_and_negative_values_use_timer_like_deferred_completion() {
        for delay_in_ms in [0, -10] {
            let mut future = Box::pin(delay(Some(delay_in_ms)));

            assert!(
                matches!(poll_once(future.as_mut()), Poll::Pending),
                "{delay_in_ms}ms delay should be deferred"
            );

            thread::sleep(Duration::from_millis(5));

            assert!(
                matches!(poll_once(future.as_mut()), Poll::Ready(())),
                "{delay_in_ms}ms delay should complete after the timer runs"
            );
        }
    }

    #[test]
    fn delay_upstream_should_resolve_after_the_specified_delay() {
        let mut future = Box::pin(delay(Some(40)));

        assert!(matches!(poll_once(future.as_mut()), Poll::Pending));

        thread::sleep(Duration::from_millis(10));
        assert!(matches!(poll_once(future.as_mut()), Poll::Pending));

        poll_pinned_until_ready(future.as_mut(), Duration::from_millis(100));
    }

    #[test]
    fn delay_upstream_should_resolve_immediately_when_delay_in_ms_is_null() {
        poll_ready(delay(None));
    }

    #[test]
    fn delay_upstream_should_resolve_immediately_when_delay_in_ms_is_undefined() {
        poll_ready(delay(None));
    }

    #[test]
    fn delay_upstream_should_resolve_immediately_when_delay_in_ms_is_0() {
        let mut future = Box::pin(delay(Some(0)));

        assert!(matches!(poll_once(future.as_mut()), Poll::Pending));

        poll_pinned_until_ready(future.as_mut(), Duration::from_millis(100));
    }

    #[test]
    fn delay_upstream_should_reject_immediately_if_signal_is_already_aborted() {
        let abort_controller = LanguageModelAbortController::new();
        abort_controller.abort();

        let mut future = Box::pin(delay_with_options(
            Some(1000),
            DelayOptions::new().with_abort_signal(abort_controller.signal()),
        ));

        assert_eq!(
            poll_once(future.as_mut()),
            Poll::Ready(Err(DelayError::aborted()))
        );
    }

    #[test]
    fn delay_upstream_should_reject_when_signal_is_aborted_during_delay() {
        let abort_controller = LanguageModelAbortController::new();
        let mut future = Box::pin(delay_with_options(
            Some(1000),
            DelayOptions::new().with_abort_signal(abort_controller.signal()),
        ));

        assert!(matches!(poll_once(future.as_mut()), Poll::Pending));

        abort_controller.abort();

        assert_eq!(
            poll_once(future.as_mut()),
            Poll::Ready(Err(DelayError::aborted()))
        );
    }

    #[test]
    fn delay_upstream_should_clean_up_timeout_when_aborted() {
        let abort_controller = LanguageModelAbortController::new();
        let mut future = Box::pin(delay_with_options(
            Some(5000),
            DelayOptions::new().with_abort_signal(abort_controller.signal()),
        ));

        assert!(matches!(poll_once(future.as_mut()), Poll::Pending));

        abort_controller.abort();

        assert_eq!(
            poll_once(future.as_mut()),
            Poll::Ready(Err(DelayError::aborted()))
        );
    }

    #[test]
    fn delay_upstream_should_clean_up_event_listener_when_delay_completes_normally() {
        let abort_controller = LanguageModelAbortController::new();
        let mut future = Box::pin(delay_with_options(
            Some(10),
            DelayOptions::new().with_abort_signal(abort_controller.signal()),
        ));

        assert!(matches!(poll_once(future.as_mut()), Poll::Pending));
        assert_eq!(
            poll_pinned_until_ready(future.as_mut(), Duration::from_millis(100)),
            Ok(())
        );

        abort_controller.abort();
        assert!(abort_controller.signal().is_aborted());
    }

    #[test]
    fn delay_upstream_should_work_without_signal_option() {
        let mut future = Box::pin(delay_with_options(Some(10), DelayOptions::new()));

        assert!(matches!(poll_once(future.as_mut()), Poll::Pending));
        assert_eq!(
            poll_pinned_until_ready(future.as_mut(), Duration::from_millis(100)),
            Ok(())
        );
    }

    #[test]
    fn delay_upstream_should_create_proper_dom_exception_for_abort() {
        let abort_controller = LanguageModelAbortController::new();
        abort_controller.abort();

        let error = poll_ready(delay_with_options(
            Some(1000),
            DelayOptions::new().with_abort_signal(abort_controller.signal()),
        ))
        .expect_err("aborted delay should fail");

        assert_eq!(error.name(), "AbortError");
        assert_eq!(error.message(), "Delay was aborted");
        assert_eq!(error.to_string(), "Delay was aborted");
    }

    #[test]
    fn delay_upstream_should_handle_very_large_delays() {
        let mut future = Box::pin(delay(Some(i64::MAX)));

        assert!(matches!(poll_once(future.as_mut()), Poll::Pending));

        thread::sleep(Duration::from_millis(2));
        assert!(matches!(poll_once(future.as_mut()), Poll::Pending));
    }

    #[test]
    fn delay_upstream_should_handle_negative_delays_treated_as_0() {
        let mut future = Box::pin(delay(Some(-100)));

        assert!(matches!(poll_once(future.as_mut()), Poll::Pending));

        poll_pinned_until_ready(future.as_mut(), Duration::from_millis(100));
    }

    #[test]
    fn delay_upstream_should_handle_multiple_delays_simultaneously() {
        let mut delay1 = Box::pin(delay(Some(20)));
        let mut delay2 = Box::pin(delay(Some(90)));
        let mut delay3 = Box::pin(delay(Some(170)));

        assert!(matches!(poll_once(delay1.as_mut()), Poll::Pending));
        assert!(matches!(poll_once(delay2.as_mut()), Poll::Pending));
        assert!(matches!(poll_once(delay3.as_mut()), Poll::Pending));

        thread::sleep(Duration::from_millis(45));
        assert!(matches!(poll_once(delay1.as_mut()), Poll::Ready(())));
        assert!(matches!(poll_once(delay2.as_mut()), Poll::Pending));
        assert!(matches!(poll_once(delay3.as_mut()), Poll::Pending));

        thread::sleep(Duration::from_millis(75));
        assert!(matches!(poll_once(delay2.as_mut()), Poll::Ready(())));
        assert!(matches!(poll_once(delay3.as_mut()), Poll::Pending));

        poll_pinned_until_ready(delay3.as_mut(), Duration::from_millis(200));
    }

    fn object_schema() -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        })
        .as_object()
        .expect("schema is an object")
        .clone()
    }

    fn object_schema_json() -> String {
        serde_json::to_string(&object_schema()).expect("schema serializes")
    }

    fn schema_object(value: JsonValue) -> JsonSchema {
        value.as_object().expect("schema is an object").clone()
    }

    fn basic_person_schema() -> JsonSchema {
        schema_object(json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "number" }
            },
            "required": ["name", "age"]
        }))
    }

    fn schema_json(schema: &JsonSchema) -> String {
        serde_json::to_string(schema).expect("schema serializes")
    }

    fn expected_json_instruction(prompt: Option<&str>, schema: &JsonSchema) -> String {
        let schema_instruction = format!(
            "JSON schema:\n{}\nYou MUST answer with a JSON object that matches the JSON schema above.",
            schema_json(schema)
        );

        match prompt.filter(|prompt| !prompt.is_empty()) {
            Some(prompt) => format!("{prompt}\n\n{schema_instruction}"),
            None => schema_instruction,
        }
    }

    #[derive(Debug, Eq, PartialEq, serde::Deserialize)]
    struct Person {
        name: String,
        age: u64,
    }

    fn validate_person(value: &JsonValue) -> Result<Person, &'static str> {
        let object = value.as_object().ok_or("Invalid input")?;
        let name = object
            .get("name")
            .and_then(JsonValue::as_str)
            .ok_or("Invalid input")?;
        let age = object
            .get("age")
            .and_then(JsonValue::as_u64)
            .ok_or("Invalid input")?;

        Ok(Person {
            name: name.to_string(),
            age,
        })
    }

    fn validate_name_value_response(value: &JsonValue) -> Result<JsonValue, &'static str> {
        let object = value.as_object().ok_or("Invalid input")?;
        object
            .get("name")
            .and_then(JsonValue::as_str)
            .ok_or("Invalid input")?;
        object
            .get("value")
            .and_then(JsonValue::as_i64)
            .ok_or("Invalid input")?;

        Ok(value.clone())
    }

    fn person_schema() -> Schema<Person> {
        Schema::new(object_schema()).with_validator(|value| match validate_person(value) {
            Ok(person) => ValidationResult::success(person),
            Err(error) => ValidationResult::failure(error),
        })
    }

    #[test]
    fn validation_result_serializes_upstream_success_and_failure_shapes() {
        let success = ValidationResult::success(json!({ "name": "Ada" }));

        assert_eq!(
            serde_json::to_value(&success).expect("success serializes"),
            json!({
                "success": true,
                "value": {
                    "name": "Ada"
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<ValidationResult<JsonValue>>(json!({
                "success": true,
                "value": {
                    "name": "Ada"
                }
            }))
            .expect("success deserializes"),
            success
        );

        let failure: ValidationResult<JsonValue> =
            ValidationResult::failure("Expected object matching schema");

        assert_eq!(
            serde_json::to_value(&failure).expect("failure serializes"),
            json!({
                "success": false,
                "error": "Expected object matching schema"
            })
        );
        assert_eq!(
            serde_json::from_value::<ValidationResult<JsonValue>>(json!({
                "success": false,
                "error": "Expected object matching schema"
            }))
            .expect("failure deserializes"),
            failure
        );
        assert!(success.is_success());
        assert!(!success.is_failure());
        assert_eq!(success.value(), Some(&json!({ "name": "Ada" })));
        assert_eq!(failure.error(), Some("Expected object matching schema"));
    }

    #[test]
    fn schema_wraps_json_schema_and_default_as_schema_matches_upstream() {
        let schema = json_schema(object_schema());

        assert_eq!(schema.json_schema(), &object_schema());
        assert!(!schema.has_validator());
        assert!(schema.validate(&json!({ "city": "Brisbane" })).is_none());

        let existing = as_schema(Some(schema.clone()));
        assert_eq!(existing.json_schema(), schema.json_schema());

        let default_schema = as_schema(None);
        let expected_default = json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
        .as_object()
        .expect("default schema is an object")
        .clone();

        assert_eq!(default_schema.json_schema(), &expected_default);
        assert!(format!("{default_schema:?}").contains("has_validator: false"));
    }

    #[test]
    fn as_schema_upstream_should_create_an_object_schema_when_no_schema_is_provided() {
        assert_eq!(
            as_schema(None).json_schema(),
            &schema_object(json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }))
        );
    }

    #[test]
    fn standard_schema_upstream_should_return_the_json_schema_from_input() {
        let schema = as_flexible_schema(Some(FlexibleSchema::from(standard_schema(
            "custom",
            schema_object(json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "age": { "type": "number" }
                },
                "required": ["name", "age"]
            })),
            |value| ValidationResult::success(value.clone()),
        ))));

        assert_eq!(
            schema.json_schema(),
            &schema_object(json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "name": { "type": "string" },
                    "age": { "type": "number" }
                },
                "required": ["name", "age"]
            }))
        );
    }

    #[test]
    fn standard_schema_upstream_should_pass_target_draft_07_to_json_schema_input() {
        let captured_target = Arc::new(Mutex::new(None::<String>));
        let schema = StandardSchema::with_json_schema(
            "custom",
            {
                let captured_target = Arc::clone(&captured_target);
                move |options: StandardSchemaJsonSchemaOptions| {
                    *captured_target
                        .lock()
                        .expect("captured target lock is not poisoned") = options.target;
                    schema_object(json!({
                        "type": "object",
                        "properties": {
                            "text": { "type": "string" }
                        }
                    }))
                }
            },
            |value| ValidationResult::success(value.clone()),
        );

        let schema = as_flexible_schema(Some(FlexibleSchema::from(schema)));
        let json_schema = schema.json_schema().clone();

        assert_eq!(
            captured_target
                .lock()
                .expect("captured target lock is not poisoned")
                .as_deref(),
            Some("draft-07")
        );
        assert_eq!(
            &json_schema,
            &schema_object(json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "text": { "type": "string" }
                }
            }))
        );
    }

    #[test]
    fn standard_schema_upstream_should_support_nested_objects() {
        let schema = as_flexible_schema(Some(FlexibleSchema::from(standard_schema(
            "custom",
            schema_object(json!({
                "type": "object",
                "properties": {
                    "user": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "email": { "type": "string" }
                        },
                        "required": ["name", "email"]
                    }
                },
                "required": ["user"]
            })),
            |value| ValidationResult::success(value.clone()),
        ))));

        assert_eq!(
            schema.json_schema(),
            &schema_object(json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "user": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "name": { "type": "string" },
                            "email": { "type": "string" }
                        },
                        "required": ["name", "email"]
                    }
                },
                "required": ["user"]
            }))
        );
    }

    #[test]
    fn standard_schema_upstream_should_support_arrays() {
        let schema = as_flexible_schema(Some(FlexibleSchema::from(standard_schema(
            "custom",
            schema_object(json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["items"]
            })),
            |value| ValidationResult::success(value.clone()),
        ))));

        assert_eq!(
            schema.json_schema(),
            &schema_object(json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "items": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                },
                "required": ["items"]
            }))
        );
    }

    #[test]
    fn standard_schema_upstream_should_validate_and_return_value_for_valid_input() {
        let schema = as_flexible_schema(Some(FlexibleSchema::from(standard_schema(
            "custom",
            schema_object(json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "age": { "type": "number" }
                }
            })),
            |value| {
                if value.get("name").and_then(JsonValue::as_str).is_some()
                    && value.get("age").and_then(JsonValue::as_i64).is_some()
                {
                    ValidationResult::success(value.clone())
                } else {
                    ValidationResult::failure("Type validation failed: Invalid input")
                }
            },
        ))));

        assert_eq!(
            schema
                .validate(&json!({
                    "name": "John",
                    "age": 30
                }))
                .expect("validator exists"),
            ValidationResult::success(json!({
                "name": "John",
                "age": 30
            }))
        );
    }

    #[test]
    fn standard_schema_upstream_should_return_error_for_invalid_input() {
        let schema = as_flexible_schema(Some(FlexibleSchema::from(standard_schema(
            "custom",
            schema_object(json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "age": { "type": "number" }
                }
            })),
            |value| {
                if value.get("name").and_then(JsonValue::as_str).is_some()
                    && value.get("age").and_then(JsonValue::as_i64).is_some()
                {
                    ValidationResult::success(value.clone())
                } else {
                    ValidationResult::failure("Type validation failed: Invalid input")
                }
            },
        ))));

        let result = schema
            .validate(&json!({
                "name": "John",
                "age": "not a number"
            }))
            .expect("validator exists");

        assert!(result.is_failure());
        assert!(
            result
                .error()
                .expect("validation error exists")
                .contains("Type validation failed")
        );
    }

    #[test]
    fn standard_schema_upstream_should_support_transform_in_validation() {
        let schema = as_flexible_schema(Some(FlexibleSchema::from(standard_schema(
            "custom",
            schema_object(json!({
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" }
                }
            })),
            |value| {
                let id = value
                    .get("id")
                    .and_then(JsonValue::as_str)
                    .and_then(|id| id.parse::<i64>().ok())
                    .unwrap_or_default();
                ValidationResult::success(json!({
                    "id": id,
                    "name": value.get("name").cloned().unwrap_or(JsonValue::Null)
                }))
            },
        ))));

        assert_eq!(
            schema
                .validate(&json!({
                    "id": "123",
                    "name": "John"
                }))
                .expect("validator exists"),
            ValidationResult::success(json!({
                "id": 123,
                "name": "John"
            }))
        );
    }

    #[test]
    fn standard_schema_upstream_should_detect_non_zod_standard_schema_by_vendor() {
        let standard = standard_schema(
            "valibot",
            schema_object(json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                }
            })),
            |value| ValidationResult::success(value.clone()),
        );

        assert_eq!(standard.vendor(), "valibot");

        let schema = as_flexible_schema(Some(FlexibleSchema::from(standard)));
        assert_eq!(
            schema.json_schema(),
            &schema_object(json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "text": { "type": "string" }
                }
            }))
        );
    }

    #[test]
    fn infer_schema_type_upstream_should_work_with_standard_schema() {
        fn assert_standard_schema_value(schema: &StandardSchema<i64>, value: i64) -> i64 {
            assert_eq!(schema.vendor(), "custom");
            value
        }

        let schema = standard_schema(
            "custom",
            schema_object(json!({ "type": "number" })),
            |value| {
                value
                    .as_i64()
                    .map(ValidationResult::success)
                    .unwrap_or_else(|| ValidationResult::failure("Invalid input"))
            },
        );

        assert_eq!(assert_standard_schema_value(&schema, 123), 123);
    }

    #[test]
    fn lazy_json_schema_defers_creation_and_caches_across_clones() {
        let calls = Arc::new(AtomicUsize::new(0));
        let schema = lazy_json_schema({
            let calls = Arc::clone(&calls);
            move || {
                calls.fetch_add(1, Ordering::SeqCst);
                object_schema()
            }
        });

        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(schema.json_schema(), &object_schema());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let cloned = schema.clone();
        assert_eq!(cloned.json_schema(), &object_schema());
        assert_eq!(schema.json_schema(), &object_schema());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lazy_schema_defers_whole_schema_creation_and_normalizes_as_flexible_schema() {
        let calls = Arc::new(AtomicUsize::new(0));
        let lazy: LazySchema = lazy_schema({
            let calls = Arc::clone(&calls);
            move || {
                calls.fetch_add(1, Ordering::SeqCst);
                json_schema(object_schema())
            }
        });

        assert!(!lazy.is_initialized());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(format!("{lazy:?}").contains("is_initialized: false"));

        let flexible = FlexibleSchema::from(lazy.clone());
        let schema = as_flexible_schema(Some(flexible));

        assert_eq!(schema.json_schema(), &object_schema());
        assert!(lazy.is_initialized());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let second = as_flexible_schema(Some(FlexibleSchema::from(lazy.clone())));
        assert_eq!(second.json_schema(), &object_schema());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let eager = as_flexible_schema(Some(FlexibleSchema::from(json_schema(object_schema()))));
        assert_eq!(eager.json_schema(), &object_schema());

        let default_schema: Schema = as_flexible_schema(None);
        assert_eq!(default_schema.json_schema(), as_schema(None).json_schema());
    }

    #[test]
    fn schema_runs_optional_rust_validator() {
        let schema =
            Schema::new(object_schema()).with_validator(|value| match validate_person(value) {
                Ok(person) => ValidationResult::success(person),
                Err(error) => ValidationResult::failure(error),
            });

        assert!(schema.has_validator());

        let valid = schema
            .validate(&json!({
                "name": "Ada",
                "age": 36
            }))
            .expect("validator is present");

        assert_eq!(
            valid.into_result().expect("person validates"),
            Person {
                name: "Ada".to_string(),
                age: 36,
            }
        );

        let invalid = schema
            .validate(&json!({
                "name": "Ada",
                "age": "old"
            }))
            .expect("validator is present");

        assert_eq!(invalid.error(), Some("Invalid input"));
        assert_eq!(
            invalid.into_result().expect_err("person validation fails"),
            "Invalid input"
        );
    }

    #[derive(Debug, Eq, PartialEq, serde::Serialize)]
    struct ErrorPayload {
        code: String,
        message: String,
    }

    fn validate_error_payload(value: &JsonValue) -> Result<ErrorPayload, &'static str> {
        let object = value.as_object().ok_or("Invalid error")?;
        let code = object
            .get("code")
            .and_then(JsonValue::as_str)
            .ok_or("Invalid error")?;
        let message = object
            .get("message")
            .and_then(JsonValue::as_str)
            .ok_or("Invalid error")?;

        Ok(ErrorPayload {
            code: code.to_string(),
            message: message.to_string(),
        })
    }

    fn expected_schema_instruction(prompt: &str) -> String {
        format!(
            "{prompt}\n\nJSON schema:\n{}\nYou MUST answer with a JSON object that matches the JSON schema above.",
            object_schema_json()
        )
    }

    #[test]
    fn inject_json_instruction_adds_schema_to_prompt() {
        assert_eq!(
            inject_json_instruction(Some("Generate weather"), Some(&object_schema()), None, None),
            expected_schema_instruction("Generate weather")
        );
    }

    #[test]
    fn inject_json_instruction_uses_generic_json_suffix_without_schema() {
        assert_eq!(
            inject_json_instruction(Some("Generate data"), None, None, None),
            "Generate data\n\nYou MUST answer with JSON."
        );
    }

    #[test]
    fn inject_json_instruction_handles_no_prompt_no_schema() {
        assert_eq!(
            inject_json_instruction(None, None, None, None),
            "You MUST answer with JSON."
        );
    }

    #[test]
    fn inject_json_instruction_omits_empty_prompt() {
        assert_eq!(
            inject_json_instruction(Some(""), Some(&object_schema()), None, None),
            format!(
                "JSON schema:\n{}\nYou MUST answer with a JSON object that matches the JSON schema above.",
                object_schema_json()
            )
        );
    }

    #[test]
    fn inject_json_instruction_uses_custom_schema_lines() {
        assert_eq!(
            inject_json_instruction(
                Some("Generate weather"),
                Some(&object_schema()),
                Some("Custom schema:"),
                Some("Follow this exactly."),
            ),
            format!(
                "Generate weather\n\nCustom schema:\n{}\nFollow this exactly.",
                object_schema_json()
            )
        );
    }

    #[test]
    fn inject_json_instruction_upstream_basic_case_with_prompt_and_schema() {
        let schema = basic_person_schema();

        assert_eq!(
            inject_json_instruction(Some("Generate a person"), Some(&schema), None, None),
            expected_json_instruction(Some("Generate a person"), &schema)
        );
    }

    #[test]
    fn inject_json_instruction_upstream_only_prompt_no_schema() {
        assert_eq!(
            inject_json_instruction(Some("Generate a person"), None, None, None),
            "Generate a person\n\nYou MUST answer with JSON."
        );
    }

    #[test]
    fn inject_json_instruction_upstream_only_schema_no_prompt() {
        let schema = basic_person_schema();

        assert_eq!(
            inject_json_instruction(None, Some(&schema), None, None),
            expected_json_instruction(None, &schema)
        );
    }

    #[test]
    fn inject_json_instruction_upstream_no_prompt_no_schema() {
        assert_eq!(
            inject_json_instruction(None, None, None, None),
            "You MUST answer with JSON."
        );
    }

    #[test]
    fn inject_json_instruction_upstream_custom_schema_prefix_and_suffix() {
        let schema = basic_person_schema();

        assert_eq!(
            inject_json_instruction(
                Some("Generate a person"),
                Some(&schema),
                Some("Custom prefix:"),
                Some("Custom suffix"),
            ),
            format!(
                "Generate a person\n\nCustom prefix:\n{}\nCustom suffix",
                schema_json(&schema)
            )
        );
    }

    #[test]
    fn inject_json_instruction_upstream_empty_string_prompt() {
        let schema = basic_person_schema();

        assert_eq!(
            inject_json_instruction(Some(""), Some(&schema), None, None),
            expected_json_instruction(None, &schema)
        );
    }

    #[test]
    fn inject_json_instruction_upstream_empty_object_schema() {
        let schema = JsonSchema::new();

        assert_eq!(
            inject_json_instruction(Some("Generate something"), Some(&schema), None, None),
            expected_json_instruction(Some("Generate something"), &schema)
        );
    }

    #[test]
    fn inject_json_instruction_upstream_complex_nested_schema() {
        let schema = schema_object(json!({
            "type": "object",
            "properties": {
                "person": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" },
                        "age": { "type": "number" },
                        "address": {
                            "type": "object",
                            "properties": {
                                "street": { "type": "string" },
                                "city": { "type": "string" }
                            }
                        }
                    }
                }
            }
        }));

        assert_eq!(
            inject_json_instruction(Some("Generate a complex person"), Some(&schema), None, None),
            expected_json_instruction(Some("Generate a complex person"), &schema)
        );
    }

    #[test]
    fn inject_json_instruction_upstream_schema_with_special_characters() {
        let schema = schema_object(json!({
            "type": "object",
            "properties": {
                "special@property": { "type": "string" },
                "emoji😊": { "type": "string" }
            }
        }));

        assert_eq!(
            inject_json_instruction(None, Some(&schema), None, None),
            expected_json_instruction(None, &schema)
        );
    }

    #[test]
    fn inject_json_instruction_upstream_very_long_prompt_and_schema() {
        let long_prompt = "A".repeat(1000);
        let mut properties = JsonObject::new();
        for index in 0..100 {
            properties.insert(format!("prop{index}"), json!({ "type": "string" }));
        }
        let schema = schema_object(json!({
            "type": "object",
            "properties": properties
        }));

        assert_eq!(
            inject_json_instruction(Some(&long_prompt), Some(&schema), None, None),
            expected_json_instruction(Some(&long_prompt), &schema)
        );
    }

    #[test]
    fn inject_json_instruction_upstream_undefined_values_for_optional_parameters() {
        assert_eq!(
            inject_json_instruction(None, None, None, None),
            "You MUST answer with JSON."
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_updates_existing_system_message() {
        let messages = vec![
            LanguageModelMessage::System(LanguageModelSystemMessage::new("Generate weather")),
            LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
                LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Use Brisbane")),
            ])),
        ];

        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(messages.clone())
                    .with_schema(object_schema())
            ),
            vec![
                LanguageModelMessage::System(LanguageModelSystemMessage::new(
                    expected_schema_instruction("Generate weather")
                )),
                messages[1].clone(),
            ]
        );
        assert_eq!(
            messages[0],
            LanguageModelMessage::System(LanguageModelSystemMessage::new("Generate weather"))
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_upstream_basic_case_with_prompt_and_schema() {
        let schema = basic_person_schema();

        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(vec![LanguageModelMessage::System(
                    LanguageModelSystemMessage::new("Generate a person")
                ),])
                .with_schema(schema.clone())
            ),
            vec![LanguageModelMessage::System(
                LanguageModelSystemMessage::new(expected_json_instruction(
                    Some("Generate a person"),
                    &schema
                ))
            )]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_upstream_does_not_mutate_input_messages() {
        let schema = basic_person_schema();
        let original_messages = vec![LanguageModelMessage::System(
            LanguageModelSystemMessage::new("Generate a person"),
        )];

        let _ = inject_json_instruction_into_messages(
            InjectJsonInstructionIntoMessagesOptions::new(original_messages.clone())
                .with_schema(schema),
        );

        assert_eq!(
            original_messages,
            vec![LanguageModelMessage::System(
                LanguageModelSystemMessage::new("Generate a person")
            )]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_upstream_empty_messages_array() {
        let schema = basic_person_schema();

        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(Vec::new())
                    .with_schema(schema.clone())
            ),
            vec![LanguageModelMessage::System(
                LanguageModelSystemMessage::new(expected_json_instruction(None, &schema))
            )]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_upstream_messages_without_initial_system_message() {
        let schema = basic_person_schema();
        let user_message = LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
        ]));
        let assistant_message =
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("Hi there")),
            ]));

        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(vec![
                    user_message.clone(),
                    assistant_message.clone(),
                ])
                .with_schema(schema.clone())
            ),
            vec![
                LanguageModelMessage::System(LanguageModelSystemMessage::new(
                    expected_json_instruction(None, &schema)
                )),
                user_message,
                assistant_message,
            ]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_upstream_system_message_with_empty_content() {
        let schema = basic_person_schema();
        let user_message = LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Generate data")),
        ]));

        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(vec![
                    LanguageModelMessage::System(LanguageModelSystemMessage::new("")),
                    user_message.clone(),
                ])
                .with_schema(schema.clone())
            ),
            vec![
                LanguageModelMessage::System(LanguageModelSystemMessage::new(
                    expected_json_instruction(None, &schema)
                )),
                user_message,
            ]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_upstream_preserves_all_non_system_messages() {
        let schema = basic_person_schema();
        let user_message = LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
        ]));
        let assistant_message =
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("Hi")),
            ]));
        let second_user_message = LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Generate person")),
        ]));

        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(vec![
                    LanguageModelMessage::System(LanguageModelSystemMessage::new(
                        "You are helpful"
                    )),
                    user_message.clone(),
                    assistant_message.clone(),
                    second_user_message.clone(),
                ])
                .with_schema(schema.clone())
            ),
            vec![
                LanguageModelMessage::System(LanguageModelSystemMessage::new(
                    expected_json_instruction(Some("You are helpful"), &schema)
                )),
                user_message,
                assistant_message,
                second_user_message,
            ]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_upstream_case_with_no_schema() {
        assert_eq!(
            inject_json_instruction_into_messages(InjectJsonInstructionIntoMessagesOptions::new(
                vec![LanguageModelMessage::System(
                    LanguageModelSystemMessage::new("Generate data")
                )],
            )),
            vec![LanguageModelMessage::System(
                LanguageModelSystemMessage::new("Generate data\n\nYou MUST answer with JSON.")
            )]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_upstream_custom_schema_prefix_and_suffix() {
        let schema = basic_person_schema();

        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(vec![LanguageModelMessage::System(
                    LanguageModelSystemMessage::new("Generate data")
                ),])
                .with_schema(schema.clone())
                .with_schema_prefix("Custom schema:")
                .with_schema_suffix("Follow this format exactly.")
            ),
            vec![LanguageModelMessage::System(
                LanguageModelSystemMessage::new(format!(
                    "Generate data\n\nCustom schema:\n{}\nFollow this format exactly.",
                    schema_json(&schema)
                ))
            )]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_inserts_system_message() {
        let user_message = LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Generate weather")),
        ]));

        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(vec![user_message.clone()])
                    .with_schema(object_schema())
            ),
            vec![
                LanguageModelMessage::System(LanguageModelSystemMessage::new(format!(
                    "JSON schema:\n{}\nYou MUST answer with a JSON object that matches the JSON schema above.",
                    object_schema_json()
                ))),
                user_message,
            ]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_handles_empty_messages_array() {
        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(Vec::new())
                    .with_schema(object_schema())
            ),
            vec![LanguageModelMessage::System(
                LanguageModelSystemMessage::new(format!(
                    "JSON schema:\n{}\nYou MUST answer with a JSON object that matches the JSON schema above.",
                    object_schema_json()
                ))
            )]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_preserves_all_non_system_messages() {
        let user_message = LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Hello")),
        ]));
        let assistant_message =
            LanguageModelMessage::Assistant(LanguageModelAssistantMessage::new(vec![
                LanguageModelAssistantContentPart::Text(LanguageModelTextPart::new("Hi")),
            ]));
        let second_user_message = LanguageModelMessage::User(LanguageModelUserMessage::new(vec![
            LanguageModelUserContentPart::Text(LanguageModelTextPart::new("Generate person")),
        ]));

        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(vec![
                    LanguageModelMessage::System(LanguageModelSystemMessage::new(
                        "You are helpful"
                    )),
                    user_message.clone(),
                    assistant_message.clone(),
                    second_user_message.clone(),
                ])
                .with_schema(object_schema())
            ),
            vec![
                LanguageModelMessage::System(LanguageModelSystemMessage::new(
                    expected_schema_instruction("You are helpful")
                )),
                user_message,
                assistant_message,
                second_user_message,
            ]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_preserves_system_provider_options() {
        let provider_options = BTreeMap::from([(
            "test-provider".to_string(),
            json!({ "trace": "abc" })
                .as_object()
                .expect("provider options are an object")
                .clone(),
        )]);

        assert_eq!(
            inject_json_instruction_into_messages(InjectJsonInstructionIntoMessagesOptions::new(
                vec![LanguageModelMessage::System(
                    LanguageModelSystemMessage::new("Generate data")
                        .with_provider_options(provider_options.clone()),
                )]
            )),
            vec![LanguageModelMessage::System(
                LanguageModelSystemMessage::new("Generate data\n\nYou MUST answer with JSON.")
                    .with_provider_options(provider_options),
            )]
        );
    }

    #[test]
    fn inject_json_instruction_into_messages_uses_custom_schema_lines() {
        assert_eq!(
            inject_json_instruction_into_messages(
                InjectJsonInstructionIntoMessagesOptions::new(vec![LanguageModelMessage::System(
                    LanguageModelSystemMessage::new("Generate weather"),
                )])
                .with_schema(object_schema())
                .with_schema_prefix("Custom schema:")
                .with_schema_suffix("Follow this exactly.")
            ),
            vec![LanguageModelMessage::System(
                LanguageModelSystemMessage::new(format!(
                    "Generate weather\n\nCustom schema:\n{}\nFollow this exactly.",
                    object_schema_json()
                ))
            )]
        );
    }

    #[test]
    fn reasoning_level_serializes_upstream_strings() {
        assert_eq!(
            serde_json::to_value(ReasoningLevel::Xhigh).expect("reasoning level serializes"),
            json!("xhigh")
        );
        assert_eq!(
            serde_json::from_value::<ReasoningLevel>(json!("minimal"))
                .expect("reasoning level deserializes"),
            ReasoningLevel::Minimal
        );
    }

    #[test]
    fn reasoning_level_converts_from_custom_reasoning_efforts() {
        assert_eq!(
            ReasoningLevel::try_from(LanguageModelReasoningEffort::High),
            Ok(ReasoningLevel::High)
        );
        assert_eq!(
            ReasoningLevel::try_from(LanguageModelReasoningEffort::ProviderDefault),
            Err(LanguageModelReasoningEffort::ProviderDefault)
        );
        assert_eq!(
            ReasoningLevel::try_from(LanguageModelReasoningEffort::None),
            Err(LanguageModelReasoningEffort::None)
        );
    }

    #[test]
    fn is_custom_reasoning_matches_upstream_default_handling() {
        assert!(!is_custom_reasoning(None));
        assert!(!is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::ProviderDefault
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::None
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::Minimal
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::Low
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::Medium
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::High
        )));
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::Xhigh
        )));
    }

    #[test]
    fn map_reasoning_to_provider_effort_returns_direct_match_without_warning() {
        let effort_map = BTreeMap::from([
            (ReasoningLevel::Minimal, "low".to_string()),
            (ReasoningLevel::Low, "low".to_string()),
            (ReasoningLevel::Medium, "medium".to_string()),
            (ReasoningLevel::High, "high".to_string()),
            (ReasoningLevel::Xhigh, "max".to_string()),
        ]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_effort(ReasoningLevel::Medium, &effort_map, &mut warnings),
            Some("medium".to_string())
        );
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn map_reasoning_to_provider_effort_warns_for_renamed_match() {
        let effort_map = BTreeMap::from([
            (ReasoningLevel::Minimal, "low".to_string()),
            (ReasoningLevel::Xhigh, "max".to_string()),
        ]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_effort(ReasoningLevel::Minimal, &effort_map, &mut warnings),
            Some("low".to_string())
        );
        assert_eq!(
            warnings,
            vec![Warning::Compatibility {
                feature: "reasoning".to_string(),
                details: Some(
                    "reasoning \"minimal\" is not directly supported by this model. mapped to effort \"low\"."
                        .to_string()
                ),
            }]
        );

        warnings.clear();
        assert_eq!(
            map_reasoning_to_provider_effort(ReasoningLevel::Xhigh, &effort_map, &mut warnings),
            Some("max".to_string())
        );
        assert_eq!(
            warnings,
            vec![Warning::Compatibility {
                feature: "reasoning".to_string(),
                details: Some(
                    "reasoning \"xhigh\" is not directly supported by this model. mapped to effort \"max\"."
                        .to_string()
                ),
            }]
        );
    }

    #[test]
    fn map_reasoning_to_provider_effort_warns_for_missing_level() {
        let effort_map = BTreeMap::from([(ReasoningLevel::Medium, "medium".to_string())]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_effort(ReasoningLevel::High, &effort_map, &mut warnings),
            None
        );
        assert_eq!(
            warnings,
            vec![Warning::Unsupported {
                feature: "reasoning".to_string(),
                details: Some("reasoning \"high\" is not supported by this model.".to_string()),
            }]
        );
    }

    #[test]
    fn map_reasoning_to_provider_budget_uses_default_percentages_and_clamps() {
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Medium,
                64_000,
                64_000,
                None,
                None,
                &mut warnings,
            ),
            Some(19_200)
        );
        assert_eq!(warnings, Vec::new());

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Xhigh,
                64_000,
                50_000,
                None,
                None,
                &mut warnings,
            ),
            Some(50_000)
        );
        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Minimal,
                10_000,
                10_000,
                None,
                None,
                &mut warnings,
            ),
            Some(1024)
        );
        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Minimal,
                10_000,
                10_000,
                Some(512),
                None,
                &mut warnings,
            ),
            Some(512)
        );
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn map_reasoning_to_provider_budget_uses_custom_percentages() {
        let budget_percentages = BTreeMap::from([(ReasoningLevel::Medium, 0.5)]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Medium,
                10_000,
                10_000,
                None,
                Some(&budget_percentages),
                &mut warnings,
            ),
            Some(5000)
        );
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn map_reasoning_to_provider_budget_warns_for_missing_custom_percentage() {
        let budget_percentages = BTreeMap::from([(ReasoningLevel::Medium, 0.5)]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::High,
                64_000,
                64_000,
                None,
                Some(&budget_percentages),
                &mut warnings,
            ),
            None
        );
        assert_eq!(
            warnings,
            vec![Warning::Unsupported {
                feature: "reasoning".to_string(),
                details: Some("reasoning \"high\" is not supported by this model.".to_string()),
            }]
        );
    }

    fn upstream_reasoning_effort_map() -> BTreeMap<ReasoningLevel, String> {
        BTreeMap::from([
            (ReasoningLevel::Minimal, "low".to_string()),
            (ReasoningLevel::Low, "low".to_string()),
            (ReasoningLevel::Medium, "medium".to_string()),
            (ReasoningLevel::High, "high".to_string()),
            (ReasoningLevel::Xhigh, "max".to_string()),
        ])
    }

    #[test]
    fn map_reasoning_to_provider_effort_upstream_returns_mapped_value_with_no_warning_for_direct_match()
     {
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_effort(
                ReasoningLevel::Medium,
                &upstream_reasoning_effort_map(),
                &mut warnings,
            ),
            Some("medium".to_string())
        );
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn map_reasoning_to_provider_effort_upstream_returns_mapped_value_with_compatibility_warning_for_renamed_match()
     {
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_effort(
                ReasoningLevel::Minimal,
                &upstream_reasoning_effort_map(),
                &mut warnings,
            ),
            Some("low".to_string())
        );
        assert_eq!(
            warnings,
            vec![Warning::Compatibility {
                feature: "reasoning".to_string(),
                details: Some(
                    "reasoning \"minimal\" is not directly supported by this model. mapped to effort \"low\"."
                        .to_string()
                ),
            }]
        );
    }

    #[test]
    fn map_reasoning_to_provider_effort_upstream_returns_mapped_value_with_compatibility_warning_for_xhigh()
     {
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_effort(
                ReasoningLevel::Xhigh,
                &upstream_reasoning_effort_map(),
                &mut warnings,
            ),
            Some("max".to_string())
        );
        assert_eq!(
            warnings,
            vec![Warning::Compatibility {
                feature: "reasoning".to_string(),
                details: Some(
                    "reasoning \"xhigh\" is not directly supported by this model. mapped to effort \"max\"."
                        .to_string()
                ),
            }]
        );
    }

    #[test]
    fn map_reasoning_to_provider_effort_upstream_returns_undefined_with_unsupported_warning_for_key_missing_from_effort_map()
     {
        let effort_map = BTreeMap::from([(ReasoningLevel::Medium, "medium".to_string())]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_effort(ReasoningLevel::High, &effort_map, &mut warnings),
            None
        );
        assert_eq!(
            warnings,
            vec![Warning::Unsupported {
                feature: "reasoning".to_string(),
                details: Some("reasoning \"high\" is not supported by this model.".to_string()),
            }]
        );
    }

    #[test]
    fn is_custom_reasoning_upstream_returns_false_for_undefined() {
        assert!(!is_custom_reasoning(None));
    }

    #[test]
    fn is_custom_reasoning_upstream_returns_false_for_provider_default() {
        assert!(!is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::ProviderDefault
        )));
    }

    #[test]
    fn is_custom_reasoning_upstream_returns_true_for_none() {
        assert!(is_custom_reasoning(Some(
            &LanguageModelReasoningEffort::None
        )));
    }

    #[test]
    fn is_custom_reasoning_upstream_returns_true_for_all_reasoning_levels() {
        for reasoning in [
            LanguageModelReasoningEffort::Minimal,
            LanguageModelReasoningEffort::Low,
            LanguageModelReasoningEffort::Medium,
            LanguageModelReasoningEffort::High,
            LanguageModelReasoningEffort::Xhigh,
        ] {
            assert!(is_custom_reasoning(Some(&reasoning)));
        }
    }

    #[test]
    fn map_reasoning_to_provider_budget_upstream_returns_correct_budget_for_known_key() {
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Medium,
                64_000,
                64_000,
                None,
                None,
                &mut warnings,
            ),
            Some(19_200)
        );
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn map_reasoning_to_provider_budget_upstream_caps_result_at_max_reasoning_budget() {
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Xhigh,
                64_000,
                50_000,
                None,
                None,
                &mut warnings,
            ),
            Some(50_000)
        );
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn map_reasoning_to_provider_budget_upstream_floors_result_at_default_min_reasoning_budget() {
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Minimal,
                10_000,
                10_000,
                None,
                None,
                &mut warnings,
            ),
            Some(1024)
        );
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn map_reasoning_to_provider_budget_upstream_respects_custom_min_reasoning_budget() {
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Minimal,
                10_000,
                10_000,
                Some(512),
                None,
                &mut warnings,
            ),
            Some(512)
        );
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn map_reasoning_to_provider_budget_upstream_respects_custom_budget_percentages() {
        let budget_percentages = BTreeMap::from([(ReasoningLevel::Medium, 0.5)]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::Medium,
                10_000,
                10_000,
                None,
                Some(&budget_percentages),
                &mut warnings,
            ),
            Some(5000)
        );
        assert_eq!(warnings, Vec::new());
    }

    #[test]
    fn map_reasoning_to_provider_budget_upstream_returns_undefined_with_unsupported_warning_for_key_missing_from_custom_budget_percentages()
     {
        let budget_percentages = BTreeMap::from([(ReasoningLevel::Medium, 0.5)]);
        let mut warnings = Vec::new();

        assert_eq!(
            map_reasoning_to_provider_budget(
                ReasoningLevel::High,
                64_000,
                64_000,
                None,
                Some(&budget_percentages),
                &mut warnings,
            ),
            None
        );
        assert_eq!(
            warnings,
            vec![Warning::Unsupported {
                feature: "reasoning".to_string(),
                details: Some("reasoning \"high\" is not supported by this model.".to_string()),
            }]
        );
    }

    #[test]
    fn arrayable_serializes_single_or_array_values() {
        assert_eq!(
            serde_json::to_value(Arrayable::single("value")).expect("single value serializes"),
            json!("value")
        );
        assert_eq!(
            serde_json::to_value(Arrayable::array(vec!["a", "b"])).expect("array value serializes"),
            json!(["a", "b"])
        );
    }

    #[test]
    fn arrayable_deserializes_single_or_array_values() {
        assert_eq!(
            serde_json::from_value::<Arrayable<String>>(json!("value"))
                .expect("single value deserializes"),
            Arrayable::single("value".to_string())
        );
        assert_eq!(
            serde_json::from_value::<Arrayable<String>>(json!(["a", "b"]))
                .expect("array value deserializes"),
            Arrayable::array(vec!["a".to_string(), "b".to_string()])
        );
    }

    #[test]
    fn as_array_upstream_returns_empty_array_for_undefined() {
        assert_eq!(as_array::<String>(None), Vec::<String>::new());
    }

    #[test]
    fn as_array_upstream_wraps_single_value_in_array() {
        assert_eq!(as_array(Some(Arrayable::single("value"))), vec!["value"]);
    }

    #[test]
    fn as_array_upstream_returns_array_value_unchanged() {
        let value = vec!["a", "b"];

        assert_eq!(as_array(Some(Arrayable::array(value.clone()))), value);
    }

    #[test]
    fn add_additional_properties_to_json_schema_upstream_adds_to_objects_recursively() {
        let schema = schema_object(json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    }
                },
                "age": { "type": "number" }
            }
        }));

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            schema_object(json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "user": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "name": { "type": "string" }
                        }
                    },
                    "age": { "type": "number" }
                }
            }))
        );
    }

    #[test]
    fn add_additional_properties_to_json_schema_upstream_adds_to_objects_inside_arrays() {
        let schema = schema_object(json!({
            "type": "object",
            "properties": {
                "ingredients": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" },
                            "amount": { "type": "string" }
                        },
                        "required": ["name", "amount"]
                    }
                }
            },
            "required": ["ingredients"]
        }));

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            schema_object(json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "ingredients": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "properties": {
                                "name": { "type": "string" },
                                "amount": { "type": "string" }
                            },
                            "required": ["name", "amount"]
                        }
                    }
                },
                "required": ["ingredients"]
            }))
        );
    }

    #[test]
    fn add_additional_properties_to_json_schema_upstream_adds_when_union_includes_object() {
        let schema = schema_object(json!({
            "type": "object",
            "properties": {
                "response": {
                    "type": ["object", "null"],
                    "properties": {
                        "name": { "type": "string" }
                    }
                }
            }
        }));

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            schema_object(json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "response": {
                        "type": ["object", "null"],
                        "additionalProperties": false,
                        "properties": {
                            "name": { "type": "string" }
                        }
                    }
                }
            }))
        );
    }

    #[test]
    fn add_additional_properties_to_json_schema_upstream_adds_to_objects_inside_any_of() {
        let schema = schema_object(json!({
            "type": "object",
            "properties": {
                "response": {
                    "anyOf": [
                        { "type": "object", "properties": { "name": { "type": "string" } } },
                        { "type": "object", "properties": { "amount": { "type": "string" } } }
                    ]
                }
            }
        }));

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            schema_object(json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "response": {
                        "anyOf": [
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": { "name": { "type": "string" } }
                            },
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": { "amount": { "type": "string" } }
                            }
                        ]
                    }
                }
            }))
        );
    }

    #[test]
    fn add_additional_properties_to_json_schema_upstream_adds_to_objects_inside_all_of() {
        let schema = schema_object(json!({
            "type": "object",
            "properties": {
                "response": {
                    "allOf": [
                        { "type": "object", "properties": { "name": { "type": "string" } } },
                        { "type": "object", "properties": { "age": { "type": "number" } } }
                    ]
                }
            }
        }));

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            schema_object(json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "response": {
                        "allOf": [
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": { "name": { "type": "string" } }
                            },
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": { "age": { "type": "number" } }
                            }
                        ]
                    }
                }
            }))
        );
    }

    #[test]
    fn add_additional_properties_to_json_schema_upstream_adds_to_objects_inside_one_of() {
        let schema = schema_object(json!({
            "type": "object",
            "properties": {
                "response": {
                    "oneOf": [
                        { "type": "object", "properties": { "success": { "type": "boolean" } } },
                        { "type": "object", "properties": { "error": { "type": "string" } } }
                    ]
                }
            }
        }));

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            schema_object(json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "response": {
                        "oneOf": [
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": { "success": { "type": "boolean" } }
                            },
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": { "error": { "type": "string" } }
                            }
                        ]
                    }
                }
            }))
        );
    }

    #[test]
    fn add_additional_properties_to_json_schema_upstream_adds_to_objects_inside_definitions() {
        let schema = schema_object(json!({
            "type": "object",
            "properties": {
                "node": { "$ref": "#/definitions/Node" }
            },
            "definitions": {
                "Node": {
                    "type": "object",
                    "properties": {
                        "value": { "type": "string" },
                        "next": { "$ref": "#/definitions/Node" }
                    }
                }
            }
        }));

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            schema_object(json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "node": { "$ref": "#/definitions/Node" }
                },
                "definitions": {
                    "Node": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "value": { "type": "string" },
                            "next": { "$ref": "#/definitions/Node" }
                        }
                    }
                }
            }))
        );
    }

    #[test]
    fn add_additional_properties_to_json_schema_upstream_overwrites_existing_flags() {
        let schema = schema_object(json!({
            "type": "object",
            "additionalProperties": true,
            "properties": {
                "meta": {
                    "type": "object",
                    "additionalProperties": true,
                    "properties": {
                        "id": { "type": "string" }
                    }
                }
            }
        }));

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            schema_object(json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "meta": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "id": { "type": "string" }
                        }
                    }
                }
            }))
        );
    }

    #[test]
    fn add_additional_properties_to_json_schema_upstream_leaves_non_object_schemas_unchanged() {
        let schema = schema_object(json!({
            "type": "string"
        }));

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            schema_object(json!({
                "type": "string"
            }))
        );
    }

    #[test]
    fn add_additional_properties_to_json_schema_visits_tuple_items() {
        let schema = schema_object(json!({
            "type": "object",
            "properties": {
                "tuple": {
                    "type": "array",
                    "items": [
                        {
                            "type": "object",
                            "properties": {
                                "first": { "type": "string" }
                            }
                        },
                        {
                            "type": "object",
                            "properties": {
                                "second": { "type": "number" }
                            }
                        }
                    ]
                }
            }
        }));

        assert_eq!(
            add_additional_properties_to_json_schema(schema),
            schema_object(json!({
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "tuple": {
                        "type": "array",
                        "items": [
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": {
                                    "first": { "type": "string" }
                                }
                            },
                            {
                                "type": "object",
                                "additionalProperties": false,
                                "properties": {
                                    "second": { "type": "number" }
                                }
                            }
                        ]
                    }
                }
            }))
        );
    }

    #[test]
    fn is_non_nullable_reports_present_values() {
        assert!(is_non_nullable(&Some("value")));
        assert!(!is_non_nullable::<&str>(&None));
    }

    #[test]
    fn filter_nullable_removes_null_and_undefined_values_from_value_list() {
        let values = vec![Some(1), None, Some(2), None, Some(3)];

        assert_eq!(filter_nullable(values), vec![1, 2, 3]);
    }

    #[test]
    fn filter_nullable_preserves_other_falsy_values() {
        let values = vec![Some(json!(0)), Some(json!(false)), Some(json!("")), None];

        assert_eq!(
            filter_nullable(values),
            vec![json!(0), json!(false), json!("")]
        );
    }

    #[test]
    fn remove_undefined_entries_should_remove_undefined_entries_from_record() {
        let record = remove_undefined_entries([
            ("a", Some(json!(1))),
            ("b", None),
            ("c", Some(json!("test"))),
            ("d", None),
        ]);

        assert_eq!(
            record,
            BTreeMap::from([
                ("a".to_string(), json!(1)),
                ("c".to_string(), json!("test")),
            ])
        );
    }

    #[test]
    fn remove_undefined_entries_should_handle_empty_object() {
        let record: BTreeMap<String, JsonValue> =
            remove_undefined_entries(Vec::<(String, Option<JsonValue>)>::new());

        assert_eq!(record, BTreeMap::new());
    }

    #[test]
    fn remove_undefined_entries_should_handle_object_with_all_undefined_values() {
        let record: BTreeMap<String, JsonValue> =
            remove_undefined_entries([("a", None::<JsonValue>), ("b", None::<JsonValue>)]);

        assert_eq!(record, BTreeMap::new());
    }

    #[test]
    fn remove_undefined_entries_should_remove_null_values() {
        let input: BTreeMap<String, Option<JsonValue>> = serde_json::from_value(json!({
            "a": null,
            "c": "test"
        }))
        .expect("record with null deserializes into optional values");
        let mut entries: Vec<_> = input.into_iter().collect();
        entries.push(("b".to_string(), None));

        assert_eq!(
            remove_undefined_entries(entries),
            BTreeMap::from([("c".to_string(), json!("test"))])
        );
    }

    #[test]
    fn remove_undefined_entries_should_preserve_falsy_values_except_null_and_undefined() {
        let input: BTreeMap<String, Option<JsonValue>> = serde_json::from_value(json!({
            "a": false,
            "b": 0,
            "c": "",
            "e": null
        }))
        .expect("record with falsy and null values deserializes into optional values");
        let mut entries: Vec<_> = input.into_iter().collect();
        entries.push(("d".to_string(), None));

        assert_eq!(
            remove_undefined_entries(entries),
            BTreeMap::from([
                ("a".to_string(), json!(false)),
                ("b".to_string(), json!(0)),
                ("c".to_string(), json!("")),
            ])
        );
    }

    #[test]
    fn remove_undefined_entries_preserves_manual_null_json_values_for_rust_callers() {
        let record = remove_undefined_entries([
            ("zero", Some(json!(0))),
            ("false", Some(json!(false))),
            ("emptyString", Some(json!(""))),
            ("nullJson", Some(json!(null))),
        ]);

        assert_eq!(
            record,
            BTreeMap::from([
                ("emptyString".to_string(), json!("")),
                ("false".to_string(), json!(false)),
                ("nullJson".to_string(), json!(null)),
                ("zero".to_string(), json!(0)),
            ])
        );
    }

    #[test]
    fn serialized_model_options_round_trips_upstream_shape() {
        let options = SerializedModelOptions::new(
            "claude-sonnet-4-20250514",
            json!({
                "provider": "anthropic.messages",
                "baseURL": "https://api.anthropic.com/v1",
                "headers": { "x-api-key": "sk-test" },
                "supportsNativeStructuredOutput": true,
                "supportsStrictTools": false
            })
            .as_object()
            .expect("config is an object")
            .clone(),
        );

        let serialized = serde_json::to_value(&options).expect("model options serialize");

        assert_eq!(
            serialized,
            json!({
                "modelId": "claude-sonnet-4-20250514",
                "config": {
                    "provider": "anthropic.messages",
                    "baseURL": "https://api.anthropic.com/v1",
                    "headers": { "x-api-key": "sk-test" },
                    "supportsNativeStructuredOutput": true,
                    "supportsStrictTools": false
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<SerializedModelOptions>(serialized)
                .expect("model options deserialize"),
            options
        );
    }

    #[test]
    fn serialize_model_options_upstream_returns_model_id_and_serializable_config() {
        let result = serialize_model_options(
            "claude-sonnet-4-20250514",
            [
                ("provider", Some(json!("anthropic.messages"))),
                ("baseURL", Some(json!("https://api.anthropic.com/v1"))),
                ("headers", Some(json!({ "x-api-key": "sk-test" }))),
                ("fetch", None),
                ("generateId", None),
                ("supportedUrls", None),
                ("supportsNativeStructuredOutput", Some(json!(true))),
                ("supportsStrictTools", Some(json!(false))),
            ],
        );

        assert_eq!(
            serde_json::to_value(&result).expect("result serializes"),
            json!({
                "modelId": "claude-sonnet-4-20250514",
                "config": {
                    "provider": "anthropic.messages",
                    "baseURL": "https://api.anthropic.com/v1",
                    "headers": { "x-api-key": "sk-test" },
                    "supportsNativeStructuredOutput": true,
                    "supportsStrictTools": false
                }
            })
        );
    }

    #[test]
    fn serialize_model_options_upstream_resolves_headers_functions_but_filters_out_other_functions()
    {
        let result = serialize_model_options(
            "gpt-4",
            [
                ("provider", Some(json!("openai"))),
                (
                    "headers",
                    Some(json!({ "authorization": "Bearer sk-test" })),
                ),
                ("url", None),
            ],
        );

        assert_eq!(
            serde_json::to_value(&result).expect("result serializes"),
            json!({
                "modelId": "gpt-4",
                "config": {
                    "provider": "openai",
                    "headers": { "authorization": "Bearer sk-test" }
                }
            })
        );
    }

    #[test]
    fn serialize_model_options_upstream_filters_out_objects_containing_functions() {
        let result = serialize_model_options(
            "model",
            [
                ("provider", Some(json!("openai-compatible"))),
                ("errorStructure", None),
                ("metadataExtractor", None),
            ],
        );

        assert_eq!(
            serde_json::to_value(&result).expect("result serializes"),
            json!({
                "modelId": "model",
                "config": {
                    "provider": "openai-compatible"
                }
            })
        );
    }

    #[test]
    fn serialize_model_options_upstream_keeps_arrays_of_primitives() {
        let result = serialize_model_options(
            "model",
            [
                ("provider", Some(json!("test"))),
                ("tags", Some(json!(["a", "b"]))),
                ("fn", None),
            ],
        );

        assert_eq!(
            serde_json::to_value(&result).expect("result serializes"),
            json!({
                "modelId": "model",
                "config": {
                    "provider": "test",
                    "tags": ["a", "b"]
                }
            })
        );
    }

    #[test]
    fn serialize_model_options_upstream_filters_out_class_instances() {
        let result = serialize_model_options(
            "model",
            [
                ("provider", Some(json!("test"))),
                ("date", None),
                ("regex", None),
            ],
        );

        assert_eq!(
            serde_json::to_value(&result).expect("result serializes"),
            json!({
                "modelId": "model",
                "config": {
                    "provider": "test"
                }
            })
        );
    }

    #[test]
    fn serialize_model_options_filters_missing_entries_and_preserves_json_null() {
        let result = serialize_model_options(
            "gpt-4",
            [
                ("provider", Some(json!("openai"))),
                (
                    "headers",
                    Some(json!({ "authorization": "Bearer sk-test" })),
                ),
                ("metadata", Some(JsonValue::Null)),
                ("fetch", None),
                ("generateId", None),
            ],
        );

        assert_eq!(
            serde_json::to_value(&result).expect("result serializes"),
            json!({
                "modelId": "gpt-4",
                "config": {
                    "provider": "openai",
                    "headers": { "authorization": "Bearer sk-test" },
                    "metadata": null
                }
            })
        );
    }

    #[test]
    fn serialize_model_options_accepts_already_json_config_objects() {
        let config = json!({
            "provider": "test",
            "tags": ["a", "b"],
            "supportsStrictTools": false
        })
        .as_object()
        .expect("config is an object")
        .clone();

        let result = serialize_model_options("model", config);

        assert_eq!(
            result,
            SerializedModelOptions::new(
                "model",
                json!({
                    "provider": "test",
                    "tags": ["a", "b"],
                    "supportsStrictTools": false
                })
                .as_object()
                .expect("config is an object")
                .clone()
            )
        );
    }

    #[test]
    fn is_json_serializable_upstream_returns_true_for_null_and_undefined() {
        assert!(is_json_serializable(&JsonSerializableValue::json(
            JsonValue::Null
        )));
        assert!(is_json_serializable(&JsonSerializableValue::Undefined));
    }

    #[test]
    fn is_json_serializable_upstream_returns_true_for_json_primitives() {
        for value in [json!("test"), json!(42), json!(true), json!(false)] {
            assert!(is_json_serializable(&JsonSerializableValue::json(value)));
        }
    }

    #[test]
    fn is_json_serializable_upstream_returns_false_for_unsupported_primitives() {
        for value in [
            JsonSerializableValue::Function,
            JsonSerializableValue::Symbol,
            JsonSerializableValue::BigInt,
        ] {
            assert!(!is_json_serializable(&value));
        }
    }

    #[test]
    fn is_json_serializable_upstream_returns_true_for_serializable_arrays() {
        let value = JsonSerializableValue::array([
            JsonSerializableValue::json(json!("test")),
            JsonSerializableValue::json(json!(42)),
            JsonSerializableValue::json(json!(true)),
            JsonSerializableValue::json(JsonValue::Null),
            JsonSerializableValue::Undefined,
            JsonSerializableValue::plain_object([(
                "nested",
                JsonSerializableValue::array([JsonSerializableValue::json(json!("value"))]),
            )]),
        ]);

        assert!(is_json_serializable(&value));
    }

    #[test]
    fn is_json_serializable_upstream_returns_false_for_arrays_with_non_serializable_values() {
        let value = JsonSerializableValue::array([
            JsonSerializableValue::json(json!("test")),
            JsonSerializableValue::Function,
        ]);

        assert!(!is_json_serializable(&value));
    }

    #[test]
    fn is_json_serializable_upstream_returns_true_for_serializable_plain_objects() {
        let value = JsonSerializableValue::plain_object([
            ("string", JsonSerializableValue::json(json!("test"))),
            ("number", JsonSerializableValue::json(json!(42))),
            ("boolean", JsonSerializableValue::json(json!(true))),
            ("nullValue", JsonSerializableValue::json(JsonValue::Null)),
            ("undefinedValue", JsonSerializableValue::Undefined),
            (
                "nested",
                JsonSerializableValue::plain_object([(
                    "array",
                    JsonSerializableValue::array([JsonSerializableValue::json(json!("value"))]),
                )]),
            ),
        ]);

        assert!(is_json_serializable(&value));
    }

    #[test]
    fn is_json_serializable_upstream_returns_false_for_plain_objects_with_non_serializable_values()
    {
        let value = JsonSerializableValue::plain_object([(
            "nested",
            JsonSerializableValue::plain_object([("callback", JsonSerializableValue::Function)]),
        )]);

        assert!(!is_json_serializable(&value));
    }

    #[test]
    fn is_json_serializable_upstream_returns_false_for_non_plain_objects() {
        for value in [
            JsonSerializableValue::non_plain_object("Date"),
            JsonSerializableValue::non_plain_object("RegExp"),
            JsonSerializableValue::non_plain_object("TestClass"),
            JsonSerializableValue::non_plain_object("Object.create(null)"),
        ] {
            assert!(!is_json_serializable(&value));
        }
    }

    #[test]
    fn form_data_contracts_round_trip_ordered_text_and_bytes_entries() {
        let form_data = FormData {
            entries: vec![
                FormDataEntry::new("model", FormDataValue::text("gpt-image-1")),
                FormDataEntry::new("image", FormDataValue::bytes(vec![1, 2, 3])),
            ],
        };

        let serialized = serde_json::to_value(&form_data).expect("form data serializes");

        assert_eq!(
            serialized,
            json!({
                "entries": [
                    {
                        "name": "model",
                        "value": {
                            "type": "text",
                            "value": "gpt-image-1"
                        }
                    },
                    {
                        "name": "image",
                        "value": {
                            "type": "bytes",
                            "value": [1, 2, 3]
                        }
                    }
                ]
            })
        );
        assert_eq!(
            serde_json::from_value::<FormData>(serialized).expect("form data deserializes"),
            form_data
        );

        let input_value =
            FormDataInputValue::array(vec![FormDataValue::text("cat"), FormDataValue::text("dog")]);
        let input_serialized =
            serde_json::to_value(&input_value).expect("form data input serializes");
        assert_eq!(
            input_serialized,
            json!({
                "type": "array",
                "values": [
                    {
                        "type": "text",
                        "value": "cat"
                    },
                    {
                        "type": "text",
                        "value": "dog"
                    }
                ]
            })
        );
        assert_eq!(
            serde_json::from_value::<FormDataInputValue>(input_serialized)
                .expect("form data input deserializes"),
            input_value
        );

        assert_eq!(
            serde_json::from_value::<ConvertToFormDataOptions>(json!({}))
                .expect("options deserialize with defaults"),
            ConvertToFormDataOptions::new()
        );
        assert_eq!(
            serde_json::to_value(ConvertToFormDataOptions::new().with_use_array_brackets(false))
                .expect("options serialize"),
            json!({ "useArrayBrackets": false })
        );
    }

    #[test]
    fn convert_to_form_data_skips_missing_and_uses_upstream_array_key_rules() {
        let form_data = convert_to_form_data(
            vec![
                (
                    "model".to_string(),
                    Some(FormDataInputValue::text("gpt-image-1")),
                ),
                ("mask".to_string(), None),
                (
                    "image".to_string(),
                    Some(FormDataInputValue::array(vec![
                        FormDataValue::bytes(vec![1, 2]),
                        FormDataValue::bytes(vec![3, 4]),
                    ])),
                ),
                (
                    "quality".to_string(),
                    Some(FormDataInputValue::array(vec![FormDataValue::text("high")])),
                ),
                (
                    "empty".to_string(),
                    Some(FormDataInputValue::array(Vec::new())),
                ),
            ],
            ConvertToFormDataOptions::new(),
        );

        assert_eq!(
            form_data.entries,
            vec![
                FormDataEntry::new("model", FormDataValue::text("gpt-image-1")),
                FormDataEntry::new("image[]", FormDataValue::bytes(vec![1, 2])),
                FormDataEntry::new("image[]", FormDataValue::bytes(vec![3, 4])),
                FormDataEntry::new("quality", FormDataValue::text("high")),
            ]
        );
        assert!(form_data.has("model"));
        assert!(!form_data.has("mask"));
        assert!(!form_data.has("empty"));
        assert_eq!(form_data.get("quality"), Some(&FormDataValue::text("high")));
        assert_eq!(form_data.get_all("image[]").len(), 2);
    }

    #[test]
    fn convert_to_form_data_can_disable_array_bracket_suffix() {
        let form_data = convert_to_form_data(
            vec![(
                "image".to_string(),
                Some(FormDataInputValue::array(vec![
                    FormDataValue::bytes(vec![1]),
                    FormDataValue::bytes(vec![2]),
                ])),
            )],
            ConvertToFormDataOptions::new().with_use_array_brackets(false),
        );

        assert_eq!(
            form_data.entries,
            vec![
                FormDataEntry::new("image", FormDataValue::bytes(vec![1])),
                FormDataEntry::new("image", FormDataValue::bytes(vec![2])),
            ]
        );
        assert!(!form_data.has("image[]"));
        assert_eq!(form_data.get_all("image").len(), 2);
    }

    #[test]
    fn convert_to_form_data_upstream_adds_string_values_to_form_data() {
        let form_data = convert_to_form_data(
            vec![
                (
                    "model".to_string(),
                    Some(FormDataInputValue::text("gpt-image-1")),
                ),
                (
                    "prompt".to_string(),
                    Some(FormDataInputValue::text("A cute cat")),
                ),
            ],
            ConvertToFormDataOptions::new(),
        );

        assert_eq!(
            form_data.get("model").cloned(),
            Some(FormDataValue::text("gpt-image-1"))
        );
        assert_eq!(
            form_data.get("prompt").cloned(),
            Some(FormDataValue::text("A cute cat"))
        );
    }

    #[test]
    fn convert_to_form_data_upstream_adds_number_values_as_strings() {
        let form_data = convert_to_form_data(
            vec![
                ("n".to_string(), Some(FormDataInputValue::text("2"))),
                ("seed".to_string(), Some(FormDataInputValue::text("42"))),
            ],
            ConvertToFormDataOptions::new(),
        );

        assert_eq!(form_data.get("n").cloned(), Some(FormDataValue::text("2")));
        assert_eq!(
            form_data.get("seed").cloned(),
            Some(FormDataValue::text("42"))
        );
    }

    #[test]
    fn convert_to_form_data_upstream_adds_blob_values_to_form_data() {
        let form_data = convert_to_form_data(
            vec![(
                "image".to_string(),
                Some(FormDataInputValue::bytes(b"test".to_vec())),
            )],
            ConvertToFormDataOptions::new(),
        );

        assert_eq!(
            form_data.get("image").cloned(),
            Some(FormDataValue::bytes(b"test".to_vec()))
        );
    }

    #[test]
    fn convert_to_form_data_upstream_skips_null_values() {
        let form_data = convert_to_form_data(
            vec![
                (
                    "model".to_string(),
                    Some(FormDataInputValue::text("gpt-image-1")),
                ),
                ("mask".to_string(), None),
            ],
            ConvertToFormDataOptions::new(),
        );

        assert_eq!(
            form_data.get("model").cloned(),
            Some(FormDataValue::text("gpt-image-1"))
        );
        assert!(!form_data.has("mask"));
    }

    #[test]
    fn convert_to_form_data_upstream_skips_undefined_values() {
        let form_data = convert_to_form_data(
            vec![
                (
                    "model".to_string(),
                    Some(FormDataInputValue::text("gpt-image-1")),
                ),
                ("size".to_string(), None),
            ],
            ConvertToFormDataOptions::new(),
        );

        assert_eq!(
            form_data.get("model").cloned(),
            Some(FormDataValue::text("gpt-image-1"))
        );
        assert!(!form_data.has("size"));
    }

    #[test]
    fn convert_to_form_data_upstream_adds_single_element_arrays_without_suffix() {
        let form_data = convert_to_form_data(
            vec![(
                "image".to_string(),
                Some(FormDataInputValue::array(vec![FormDataValue::bytes(
                    b"test".to_vec(),
                )])),
            )],
            ConvertToFormDataOptions::new(),
        );

        assert_eq!(
            form_data.get("image").cloned(),
            Some(FormDataValue::bytes(b"test".to_vec()))
        );
        assert!(!form_data.has("image[]"));
    }

    #[test]
    fn convert_to_form_data_upstream_adds_multi_element_arrays_with_suffix() {
        let form_data = convert_to_form_data(
            vec![(
                "image".to_string(),
                Some(FormDataInputValue::array(vec![
                    FormDataValue::bytes(b"test1".to_vec()),
                    FormDataValue::bytes(b"test2".to_vec()),
                ])),
            )],
            ConvertToFormDataOptions::new(),
        );

        assert!(!form_data.has("image"));
        let images: Vec<_> = form_data.get_all("image[]").into_iter().cloned().collect();
        assert_eq!(
            images,
            vec![
                FormDataValue::bytes(b"test1".to_vec()),
                FormDataValue::bytes(b"test2".to_vec()),
            ]
        );
    }

    #[test]
    fn convert_to_form_data_upstream_adds_multi_element_arrays_without_suffix_when_disabled() {
        let form_data = convert_to_form_data(
            vec![(
                "image".to_string(),
                Some(FormDataInputValue::array(vec![
                    FormDataValue::bytes(b"test1".to_vec()),
                    FormDataValue::bytes(b"test2".to_vec()),
                ])),
            )],
            ConvertToFormDataOptions::new().with_use_array_brackets(false),
        );

        assert!(!form_data.has("image[]"));
        let images: Vec<_> = form_data.get_all("image").into_iter().cloned().collect();
        assert_eq!(
            images,
            vec![
                FormDataValue::bytes(b"test1".to_vec()),
                FormDataValue::bytes(b"test2".to_vec()),
            ]
        );
    }

    #[test]
    fn convert_to_form_data_upstream_handles_empty_arrays_by_not_adding_values() {
        let form_data = convert_to_form_data(
            vec![
                ("model".to_string(), Some(FormDataInputValue::text("test"))),
                (
                    "images".to_string(),
                    Some(FormDataInputValue::array(Vec::new())),
                ),
            ],
            ConvertToFormDataOptions::new(),
        );

        assert_eq!(
            form_data.get("model").cloned(),
            Some(FormDataValue::text("test"))
        );
        assert!(!form_data.has("images"));
        assert!(!form_data.has("images[]"));
    }

    #[test]
    fn convert_to_form_data_upstream_adds_string_arrays_with_suffix() {
        let form_data = convert_to_form_data(
            vec![(
                "tags".to_string(),
                Some(FormDataInputValue::array(vec![
                    FormDataValue::text("cat"),
                    FormDataValue::text("cute"),
                    FormDataValue::text("animal"),
                ])),
            )],
            ConvertToFormDataOptions::new(),
        );

        let tags: Vec<_> = form_data.get_all("tags[]").into_iter().cloned().collect();
        assert_eq!(
            tags,
            vec![
                FormDataValue::text("cat"),
                FormDataValue::text("cute"),
                FormDataValue::text("animal"),
            ]
        );
    }

    #[test]
    fn convert_to_form_data_upstream_accepts_typed_input_objects() {
        let form_data = convert_to_form_data(
            vec![
                (
                    "model".to_string(),
                    Some(FormDataInputValue::text("dall-e-3")),
                ),
                (
                    "prompt".to_string(),
                    Some(FormDataInputValue::text("A sunset")),
                ),
                ("n".to_string(), Some(FormDataInputValue::text("1"))),
                (
                    "size".to_string(),
                    Some(FormDataInputValue::text("1024x1024")),
                ),
            ],
            ConvertToFormDataOptions::new(),
        );

        assert_eq!(
            form_data.get("model").cloned(),
            Some(FormDataValue::text("dall-e-3"))
        );
        assert_eq!(
            form_data.get("prompt").cloned(),
            Some(FormDataValue::text("A sunset"))
        );
        assert_eq!(form_data.get("n").cloned(), Some(FormDataValue::text("1")));
        assert_eq!(
            form_data.get("size").cloned(),
            Some(FormDataValue::text("1024x1024"))
        );
    }

    #[test]
    fn convert_to_form_data_upstream_handles_complex_input_with_various_types() {
        let form_data = convert_to_form_data(
            vec![
                (
                    "model".to_string(),
                    Some(FormDataInputValue::text("gpt-image-1")),
                ),
                (
                    "prompt".to_string(),
                    Some(FormDataInputValue::text("Edit this image")),
                ),
                (
                    "image".to_string(),
                    Some(FormDataInputValue::array(vec![FormDataValue::bytes(
                        b"image data".to_vec(),
                    )])),
                ),
                ("mask".to_string(), None),
                ("n".to_string(), Some(FormDataInputValue::text("1"))),
                (
                    "size".to_string(),
                    Some(FormDataInputValue::text("1024x1024")),
                ),
                (
                    "quality".to_string(),
                    Some(FormDataInputValue::text("high")),
                ),
            ],
            ConvertToFormDataOptions::new(),
        );

        assert_eq!(
            form_data.get("model").cloned(),
            Some(FormDataValue::text("gpt-image-1"))
        );
        assert_eq!(
            form_data.get("prompt").cloned(),
            Some(FormDataValue::text("Edit this image"))
        );
        assert_eq!(
            form_data.get("image").cloned(),
            Some(FormDataValue::bytes(b"image data".to_vec()))
        );
        assert!(!form_data.has("mask"));
        assert_eq!(form_data.get("n").cloned(), Some(FormDataValue::text("1")));
        assert_eq!(
            form_data.get("size").cloned(),
            Some(FormDataValue::text("1024x1024"))
        );
        assert_eq!(
            form_data.get("quality").cloned(),
            Some(FormDataValue::text("high"))
        );
    }

    #[test]
    fn download_blob_contracts_serialize_with_upstream_camel_case_fields() {
        let options = DownloadBlobOptions::new("https://example.com/image.png").with_max_bytes(4);
        assert_eq!(
            serde_json::to_value(&options).expect("options serialize"),
            json!({
                "url": "https://example.com/image.png",
                "maxBytes": 4
            })
        );
        assert_eq!(
            serde_json::from_value::<DownloadBlobOptions>(json!({
                "url": "https://example.com/image.png"
            }))
            .expect("options deserialize"),
            DownloadBlobOptions::new("https://example.com/image.png")
        );

        let response = DownloadBlobResponse::bytes(200, "OK", vec![1, 2, 3])
            .with_headers(BTreeMap::from([(
                "content-type".to_string(),
                "image/png".to_string(),
            )]))
            .with_final_url("https://cdn.example.com/image.png");
        assert_eq!(
            serde_json::to_value(&response).expect("response serializes"),
            json!({
                "statusCode": 200,
                "statusText": "OK",
                "headers": {
                    "content-type": "image/png"
                },
                "body": [1, 2, 3],
                "finalUrl": "https://cdn.example.com/image.png"
            })
        );
        assert_eq!(
            serde_json::from_value::<DownloadBlobResponse>(
                serde_json::to_value(&response).expect("response serializes")
            )
            .expect("response deserializes"),
            response
        );

        let blob = DownloadedBlob::new(vec![1, 2, 3]).with_media_type("image/png");
        assert_eq!(
            serde_json::to_value(&blob).expect("blob serializes"),
            json!({
                "data": [1, 2, 3],
                "mediaType": "image/png"
            })
        );
    }

    #[test]
    fn download_blob_downloads_bytes_and_content_type_through_injected_transport() {
        let result = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/image.png"),
            |url| {
                assert_eq!(url, "https://example.com/image.png");
                ready(Ok(DownloadBlobResponse::bytes(
                    200,
                    "OK",
                    b"test content".to_vec(),
                )
                .with_headers(BTreeMap::from([
                    ("Content-Type".to_string(), "image/png".to_string()),
                    ("Content-Length".to_string(), "12".to_string()),
                ]))))
            },
        ))
        .expect("download succeeds");

        assert_eq!(
            result,
            DownloadedBlob::new(b"test content".to_vec()).with_media_type("image/png")
        );
    }

    #[test]
    fn download_blob_returns_empty_blob_for_missing_body() {
        let result = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/empty.bin"),
            |_| {
                ready(Ok(DownloadBlobResponse::new(200, "OK").with_headers(
                    BTreeMap::from([(
                        "content-type".to_string(),
                        "application/octet-stream".to_string(),
                    )]),
                )))
            },
        ))
        .expect("download succeeds");

        assert_eq!(
            result,
            DownloadedBlob::new(Vec::new()).with_media_type("application/octet-stream")
        );
    }

    #[test]
    fn download_blob_turns_non_success_status_into_download_error() {
        let error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/not-found.png"),
            |_| ready(Ok(DownloadBlobResponse::new(404, "Not Found"))),
        ))
        .expect_err("non-success status fails");

        assert_eq!(error.url(), "https://example.com/not-found.png");
        assert_eq!(error.status_code(), Some(404));
        assert_eq!(error.status_text(), Some("Not Found"));
        assert_eq!(
            error.message(),
            "Failed to download https://example.com/not-found.png: 404 Not Found"
        );
    }

    #[test]
    fn download_blob_enforces_size_limit_from_headers_and_body_bytes() {
        let content_length_error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/huge.bin").with_max_bytes(3),
            |_| {
                ready(Ok(DownloadBlobResponse::bytes(200, "OK", vec![1, 2, 3, 4])
                    .with_headers(BTreeMap::from([(
                        "content-length".to_string(),
                        "4 bytes".to_string(),
                    )]))))
            },
        ))
        .expect_err("content-length over limit fails");
        assert_eq!(
            content_length_error.message(),
            "Download of https://example.com/huge.bin exceeded maximum size of 3 bytes (Content-Length: 4)."
        );

        let body_error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/liar.bin").with_max_bytes(3),
            |_| ready(Ok(DownloadBlobResponse::bytes(200, "OK", vec![1, 2, 3, 4]))),
        ))
        .expect_err("body over limit fails");
        assert_eq!(
            body_error.message(),
            "Download of https://example.com/liar.bin exceeded maximum size of 3 bytes."
        );
    }

    #[test]
    fn download_blob_validates_redirect_final_url() {
        let error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/redirect"),
            |_| {
                ready(Ok(DownloadBlobResponse::bytes(
                    200,
                    "OK",
                    b"secret".to_vec(),
                )
                .with_final_url("http://localhost/admin")))
            },
        ))
        .expect_err("unsafe redirect URL fails");

        assert_eq!(
            error.message(),
            "URL with hostname localhost is not allowed"
        );
    }

    #[test]
    fn download_blob_propagates_transport_download_errors() {
        let error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/network-error.png"),
            |_| {
                ready(Err(DownloadError::with_cause_message(
                    "https://example.com/network-error.png",
                    "Network error",
                )))
            },
        ))
        .expect_err("transport error propagates");

        assert_eq!(
            error.message(),
            "Failed to download https://example.com/network-error.png: Network error"
        );
    }

    fn expect_download_blob_rejected_before_transport(url: &str) -> DownloadError {
        let called = Arc::new(AtomicUsize::new(0));
        let called_by_transport = Arc::clone(&called);

        let error = poll_ready(download_blob(DownloadBlobOptions::new(url), move |_| {
            called_by_transport.fetch_add(1, Ordering::SeqCst);
            ready(Ok(DownloadBlobResponse::new(200, "OK")))
        }))
        .expect_err("unsafe URL should be rejected before transport");

        assert_eq!(
            called.load(Ordering::SeqCst),
            0,
            "transport must not be called for unsafe URL"
        );

        error
    }

    #[test]
    fn download_blob_upstream_should_download_a_blob_successfully() {
        let called_with = Arc::new(Mutex::new(Vec::<String>::new()));
        let called_with_transport = Arc::clone(&called_with);
        let content = b"test content".to_vec();

        let result = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/image.png"),
            move |url| {
                called_with_transport.lock().unwrap().push(url.to_string());
                ready(Ok(DownloadBlobResponse::bytes(200, "OK", content)
                    .with_headers(BTreeMap::from([(
                        "content-type".to_string(),
                        "image/png".to_string(),
                    )]))))
            },
        ))
        .expect("download succeeds");

        assert_eq!(result.media_type.as_deref(), Some("image/png"));
        assert_eq!(result.data, b"test content");
        assert_eq!(
            called_with.lock().unwrap().as_slice(),
            ["https://example.com/image.png"]
        );
    }

    #[test]
    fn download_blob_upstream_should_throw_download_error_on_non_ok_response() {
        let first_error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/not-found.png"),
            |_| ready(Ok(DownloadBlobResponse::new(404, "Not Found"))),
        ))
        .expect_err("non-ok response throws DownloadError");

        assert_eq!(first_error.name(), DownloadError::NAME);

        let error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/not-found.png"),
            |_| ready(Ok(DownloadBlobResponse::new(404, "Not Found"))),
        ))
        .expect_err("non-ok response exposes DownloadError details");

        assert_eq!(error.url(), "https://example.com/not-found.png");
        assert_eq!(error.status_code(), Some(404));
        assert_eq!(error.status_text(), Some("Not Found"));
        assert_eq!(
            error.message(),
            "Failed to download https://example.com/not-found.png: 404 Not Found"
        );
    }

    #[test]
    fn download_blob_upstream_should_throw_download_error_on_network_error() {
        let error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/network-error.png"),
            |_| {
                ready(Err(DownloadError::with_cause_message(
                    "https://example.com/network-error.png",
                    "Network error",
                )))
            },
        ))
        .expect_err("network error throws DownloadError");

        assert_eq!(error.name(), DownloadError::NAME);
        assert_eq!(error.url(), "https://example.com/network-error.png");
        assert_eq!(error.cause_message(), Some("Network error"));
        assert!(
            error.message().contains("Network error"),
            "message should include the lower-level network error"
        );
    }

    #[test]
    fn download_blob_upstream_should_rethrow_download_error_without_wrapping() {
        let original_error = DownloadError::with_status(
            "https://example.com/original.png",
            500,
            "Internal Server Error",
        );
        let expected_error = original_error.clone();

        let error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/test.png"),
            move |_| ready(Err(original_error)),
        ))
        .expect_err("DownloadError from transport is propagated");

        assert_eq!(error, expected_error);
        assert_eq!(error.url(), "https://example.com/original.png");
        assert_eq!(error.status_code(), Some(500));
    }

    #[test]
    fn download_blob_upstream_should_abort_when_response_exceeds_default_size_limit() {
        let error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/huge.bin"),
            |_| {
                ready(Ok(DownloadBlobResponse::bytes(200, "OK", vec![0; 10])
                    .with_headers(BTreeMap::from([(
                        "content-length".to_string(),
                        (3_u128 * 1024 * 1024 * 1024).to_string(),
                    )]))))
            },
        ))
        .expect_err("oversized default content-length should fail");

        assert_eq!(error.name(), DownloadError::NAME);
        assert!(
            error.message().contains("exceeded maximum size"),
            "message should explain the size-limit failure"
        );
    }

    #[test]
    fn download_blob_ssrf_upstream_should_reject_private_ipv4_addresses() {
        for url in [
            "http://127.0.0.1/file",
            "http://10.0.0.1/file",
            "http://169.254.169.254/latest/meta-data/",
        ] {
            let error = expect_download_blob_rejected_before_transport(url);
            assert_eq!(error.name(), DownloadError::NAME);
        }
    }

    #[test]
    fn download_blob_ssrf_upstream_should_reject_localhost() {
        let error = expect_download_blob_rejected_before_transport("http://localhost/file");
        assert_eq!(error.name(), DownloadError::NAME);
        assert_eq!(
            error.message(),
            "URL with hostname localhost is not allowed"
        );
    }

    #[test]
    fn download_blob_ssrf_upstream_should_reject_non_http_protocols() {
        let error = expect_download_blob_rejected_before_transport("file:///etc/passwd");
        assert_eq!(error.name(), DownloadError::NAME);
        assert_eq!(
            error.message(),
            "URL scheme must be http, https, or data, got file:"
        );
    }

    #[test]
    fn download_blob_ssrf_upstream_should_reject_redirects_to_private_ip_addresses() {
        let error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://evil.com/redirect"),
            |_| {
                ready(Ok(DownloadBlobResponse::bytes(
                    200,
                    "OK",
                    b"secret".to_vec(),
                )
                .with_headers(BTreeMap::from([(
                    "content-type".to_string(),
                    "text/plain".to_string(),
                )]))
                .with_final_url("http://169.254.169.254/latest/meta-data/")))
            },
        ))
        .expect_err("redirect to private IP should fail");

        assert_eq!(error.name(), DownloadError::NAME);
    }

    #[test]
    fn download_blob_ssrf_upstream_should_reject_redirects_to_localhost() {
        let error = poll_ready(download_blob(
            DownloadBlobOptions::new("https://evil.com/redirect"),
            |_| {
                ready(Ok(DownloadBlobResponse::bytes(
                    200,
                    "OK",
                    b"secret".to_vec(),
                )
                .with_headers(BTreeMap::from([(
                    "content-type".to_string(),
                    "text/plain".to_string(),
                )]))
                .with_final_url("http://localhost:8080/admin")))
            },
        ))
        .expect_err("redirect to localhost should fail");

        assert_eq!(error.name(), DownloadError::NAME);
        assert_eq!(
            error.message(),
            "URL with hostname localhost is not allowed"
        );
    }

    #[test]
    fn download_blob_ssrf_upstream_should_allow_redirects_to_safe_urls() {
        let content = b"safe content".to_vec();

        let result = poll_ready(download_blob(
            DownloadBlobOptions::new("https://example.com/image.png"),
            |_| {
                ready(Ok(DownloadBlobResponse::bytes(200, "OK", content)
                    .with_headers(BTreeMap::from([(
                        "content-type".to_string(),
                        "image/png".to_string(),
                    )]))
                    .with_final_url("https://cdn.example.com/image.png")))
            },
        ))
        .expect("safe redirect succeeds");

        assert_eq!(result.media_type.as_deref(), Some("image/png"));
        assert_eq!(result.data, b"safe content");
    }

    #[test]
    fn download_error_upstream_should_create_error_with_status_code_and_text() {
        let error = DownloadError::with_status("https://example.com/test.png", 403, "Forbidden");

        assert_eq!(error.name(), "AI_DownloadError");
        assert_eq!(error.url(), "https://example.com/test.png");
        assert_eq!(error.status_code(), Some(403));
        assert_eq!(error.status_text(), Some("Forbidden"));
        assert_eq!(
            error.message(),
            "Failed to download https://example.com/test.png: 403 Forbidden"
        );
    }

    #[test]
    fn download_error_upstream_should_create_error_with_cause() {
        let error =
            DownloadError::with_cause_message("https://example.com/test.png", "Connection refused");

        assert_eq!(error.url(), "https://example.com/test.png");
        assert_eq!(error.cause_message(), Some("Connection refused"));
        assert!(
            error.message().contains("Connection refused"),
            "message should contain the cause message"
        );
    }

    #[test]
    fn download_error_upstream_should_create_error_with_custom_message() {
        let error = DownloadError::new("https://example.com/test.png", "Custom error message");

        assert_eq!(error.message(), "Custom error message");
    }

    #[test]
    fn download_error_upstream_should_identify_download_error_instances_correctly() {
        let download_error = DownloadError::new("https://example.com/test.png", "download failed");
        let regular_error = std::io::Error::other("Not a download error");

        let download_error_ref: &(dyn std::error::Error + 'static) = &download_error;
        let regular_error_ref: &(dyn std::error::Error + 'static) = &regular_error;

        assert!(download_error_ref.is::<DownloadError>());
        assert!(!regular_error_ref.is::<DownloadError>());
    }

    #[test]
    fn streaming_tool_call_tracker_options_serialize_and_deserialize_validation_mode() {
        let options = StreamingToolCallTrackerOptions::new()
            .with_type_validation(StreamingToolCallTypeValidation::IfPresent);

        let serialized = serde_json::to_value(&options).expect("tracker options serialize");

        assert_eq!(
            serialized,
            json!({
                "typeValidation": "if-present"
            })
        );
        assert_eq!(
            serde_json::from_value::<StreamingToolCallTrackerOptions>(serialized)
                .expect("tracker options deserialize"),
            options
        );
        assert_eq!(
            serde_json::from_value::<StreamingToolCallTrackerOptions>(json!({}))
                .expect("empty options deserialize"),
            StreamingToolCallTrackerOptions::default()
        );
    }

    #[test]
    fn streaming_tool_call_delta_round_trips_upstream_shape_with_extensions() {
        let delta = StreamingToolCallDelta::new()
            .with_index(0)
            .with_id("call_1")
            .with_type("function")
            .with_function(
                StreamingToolCallDeltaFunction::new()
                    .with_name("get_weather")
                    .with_arguments(r#"{"city":"London"}"#),
            )
            .with_extra_value(
                "extra_content",
                json!({ "google": { "thought_signature": "sig123" } }),
            );

        let serialized = serde_json::to_value(&delta).expect("delta serializes");

        assert_eq!(
            serialized,
            json!({
                "index": 0,
                "id": "call_1",
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "arguments": r#"{"city":"London"}"#
                },
                "extra_content": {
                    "google": { "thought_signature": "sig123" }
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<StreamingToolCallDelta>(serialized)
                .expect("delta deserializes"),
            delta
        );
    }

    fn stream_parts_json(parts: Vec<LanguageModelStreamPart>) -> JsonValue {
        serde_json::to_value(parts).expect("stream parts serialize")
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_handle_single_tool_call_accumulated_across_multiple_deltas()
     {
        let mut tracker = StreamingToolCallTracker::new();

        let first_parts = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("get_weather")
                            .with_arguments(r#"{"ci"#),
                    ),
            )
            .expect("first delta succeeds");

        assert_eq!(
            stream_parts_json(first_parts),
            json!([
                { "type": "tool-input-start", "id": "call_1", "toolName": "get_weather" },
                { "type": "tool-input-delta", "id": "call_1", "delta": r#"{"ci"# }
            ])
        );

        let second_parts = tracker
            .process_delta(StreamingToolCallDelta::new().with_index(0).with_function(
                StreamingToolCallDeltaFunction::new().with_arguments(r#"ty": "San"#),
            ))
            .expect("second delta succeeds");

        assert_eq!(
            stream_parts_json(second_parts),
            json!([
                { "type": "tool-input-delta", "id": "call_1", "delta": r#"ty": "San"# }
            ])
        );

        let third_parts = tracker
            .process_delta(StreamingToolCallDelta::new().with_index(0).with_function(
                StreamingToolCallDeltaFunction::new().with_arguments(r#" Francisco"}"#),
            ))
            .expect("third delta succeeds");

        assert_eq!(
            stream_parts_json(third_parts),
            json!([
                { "type": "tool-input-delta", "id": "call_1", "delta": r#" Francisco"}"# },
                { "type": "tool-input-end", "id": "call_1" },
                {
                    "type": "tool-call",
                    "toolCallId": "call_1",
                    "toolName": "get_weather",
                    "input": r#"{"city": "San Francisco"}"#
                }
            ])
        );
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_handle_full_tool_call_in_single_chunk() {
        let mut tracker = StreamingToolCallTracker::new();

        let parts = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("get_weather")
                            .with_arguments(r#"{"city": "London"}"#),
                    ),
            )
            .expect("single-chunk delta succeeds");

        assert_eq!(
            stream_parts_json(parts),
            json!([
                { "type": "tool-input-start", "id": "call_1", "toolName": "get_weather" },
                { "type": "tool-input-delta", "id": "call_1", "delta": r#"{"city": "London"}"# },
                { "type": "tool-input-end", "id": "call_1" },
                {
                    "type": "tool-call",
                    "toolCallId": "call_1",
                    "toolName": "get_weather",
                    "input": r#"{"city": "London"}"#
                }
            ])
        );
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_handle_multiple_concurrent_tool_calls() {
        let mut tracker = StreamingToolCallTracker::new();
        let mut parts = Vec::new();

        parts.extend(
            tracker
                .process_delta(
                    StreamingToolCallDelta::new()
                        .with_index(0)
                        .with_id("call_1")
                        .with_type("function")
                        .with_function(
                            StreamingToolCallDeltaFunction::new()
                                .with_name("get_weather")
                                .with_arguments(""),
                        ),
                )
                .expect("first tool call starts"),
        );
        parts.extend(
            tracker
                .process_delta(
                    StreamingToolCallDelta::new()
                        .with_index(1)
                        .with_id("call_2")
                        .with_type("function")
                        .with_function(
                            StreamingToolCallDeltaFunction::new()
                                .with_name("get_time")
                                .with_arguments(""),
                        ),
                )
                .expect("second tool call starts"),
        );

        assert_eq!(
            stream_parts_json(parts),
            json!([
                { "type": "tool-input-start", "id": "call_1", "toolName": "get_weather" },
                { "type": "tool-input-start", "id": "call_2", "toolName": "get_time" }
            ])
        );
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_skip_deltas_for_already_finished_tool_calls() {
        let mut tracker = StreamingToolCallTracker::new();

        tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments("{}"),
                    ),
            )
            .expect("tool call completes");

        let parts = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_function(StreamingToolCallDeltaFunction::new().with_arguments("extra")),
            )
            .expect("late delta is ignored");

        assert_eq!(parts, Vec::new());
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_skip_delta_emission_when_arguments_are_null() {
        let mut tracker = StreamingToolCallTracker::new();

        tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments(""),
                    ),
            )
            .expect("tool call starts");

        let parts = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_function(StreamingToolCallDeltaFunction::new()),
            )
            .expect("null arguments are ignored");

        assert_eq!(parts, Vec::new());
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_use_index_fallback_when_index_is_not_provided() {
        let mut tracker = StreamingToolCallTracker::new();
        let mut parts = Vec::new();

        parts.extend(
            tracker
                .process_delta(
                    StreamingToolCallDelta::new()
                        .with_id("call_1")
                        .with_type("function")
                        .with_function(
                            StreamingToolCallDeltaFunction::new()
                                .with_name("fn1")
                                .with_arguments("{}"),
                        ),
                )
                .expect("first fallback-index tool call succeeds"),
        );
        parts.extend(
            tracker
                .process_delta(
                    StreamingToolCallDelta::new()
                        .with_id("call_2")
                        .with_type("function")
                        .with_function(
                            StreamingToolCallDeltaFunction::new()
                                .with_name("fn2")
                                .with_arguments("{}"),
                        ),
                )
                .expect("second fallback-index tool call succeeds"),
        );

        let start_parts: Vec<JsonValue> = stream_parts_json(parts)
            .as_array()
            .expect("parts are array")
            .iter()
            .filter(|part| part.get("type") == Some(&json!("tool-input-start")))
            .cloned()
            .collect();

        assert_eq!(
            start_parts,
            vec![
                json!({ "type": "tool-input-start", "id": "call_1", "toolName": "fn1" }),
                json!({ "type": "tool-input-start", "id": "call_2", "toolName": "fn2" }),
            ]
        );
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_throw_when_id_is_missing() {
        let mut tracker = StreamingToolCallTracker::new();

        let error = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_type("function")
                    .with_function(StreamingToolCallDeltaFunction::new().with_name("fn")),
            )
            .expect_err("missing id should fail");

        assert_eq!(error.to_string(), "Expected 'id' to be a string.");
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_throw_when_function_name_is_missing() {
        let mut tracker = StreamingToolCallTracker::new();

        let error = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(StreamingToolCallDeltaFunction::new()),
            )
            .expect_err("missing function name should fail");

        assert_eq!(
            error.to_string(),
            "Expected 'function.name' to be a string."
        );
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_not_validate_type_with_type_validation_none() {
        let mut tracker = StreamingToolCallTracker::from_options(
            StreamingToolCallTrackerOptions::new()
                .with_type_validation(StreamingToolCallTypeValidation::None),
        );

        tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("custom")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments(""),
                    ),
            )
            .expect("custom type is accepted without validation");
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_validate_type_when_present_with_type_validation_if_present()
     {
        let mut tracker = StreamingToolCallTracker::from_options(
            StreamingToolCallTrackerOptions::new()
                .with_type_validation(StreamingToolCallTypeValidation::IfPresent),
        );

        let error = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("custom")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments(""),
                    ),
            )
            .expect_err("custom type is rejected when type is present");
        assert_eq!(error.to_string(), "Expected 'function' type.");

        tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments(""),
                    ),
            )
            .expect("missing type is accepted in if-present mode");
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_require_function_type_with_type_validation_required()
     {
        let mut tracker = StreamingToolCallTracker::from_options(
            StreamingToolCallTrackerOptions::new()
                .with_type_validation(StreamingToolCallTypeValidation::Required),
        );

        let error = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments(""),
                    ),
            )
            .expect_err("missing type is rejected when required");
        assert_eq!(error.to_string(), "Expected 'function' type.");

        tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments(""),
                    ),
            )
            .expect("function type is accepted when required");
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_finalize_unfinished_tool_calls_on_flush() {
        let mut tracker = StreamingToolCallTracker::new();

        tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments(r#"{"key": "val"#),
                    ),
            )
            .expect("unfinished tool call starts");

        assert_eq!(
            stream_parts_json(tracker.flush()),
            json!([
                { "type": "tool-input-end", "id": "call_1" },
                {
                    "type": "tool-call",
                    "toolCallId": "call_1",
                    "toolName": "fn",
                    "input": r#"{"key": "val"#
                }
            ])
        );
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_not_refinalize_already_finished_tool_calls() {
        let mut tracker = StreamingToolCallTracker::new();

        tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments("{}"),
                    ),
            )
            .expect("tool call completes");

        assert!(tracker.flush().is_empty());
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_extract_and_include_provider_metadata_in_tool_call_events()
     {
        let mut tracker = StreamingToolCallTracker::new()
            .with_extract_metadata(|delta| {
                let signature = delta
                    .extra
                    .get("extra_content")?
                    .get("google")?
                    .get("thought_signature")?
                    .as_str()?;

                Some(ProviderMetadata::from([(
                    "google".to_string(),
                    json!({ "thoughtSignature": signature })
                        .as_object()
                        .expect("metadata is an object")
                        .clone(),
                )]))
            })
            .with_tool_call_provider_metadata(|metadata| metadata.cloned());

        let parts = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments("{}"),
                    )
                    .with_extra_value(
                        "extra_content",
                        json!({ "google": { "thought_signature": "sig123" } }),
                    ),
            )
            .expect("metadata delta succeeds");

        let tool_call_event = stream_parts_json(parts)
            .as_array()
            .expect("parts are array")
            .iter()
            .find(|part| part.get("type") == Some(&json!("tool-call")))
            .cloned();

        assert_eq!(
            tool_call_event,
            Some(json!({
                "type": "tool-call",
                "toolCallId": "call_1",
                "toolName": "fn",
                "input": "{}",
                "providerMetadata": {
                    "google": { "thoughtSignature": "sig123" }
                }
            }))
        );
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_include_provider_metadata_for_unfinished_tool_calls_finalized_in_flush()
     {
        let mut tracker = StreamingToolCallTracker::new()
            .with_extract_metadata(|_| {
                Some(ProviderMetadata::from([(
                    "custom".to_string(),
                    json!({ "key": "value" })
                        .as_object()
                        .expect("metadata is an object")
                        .clone(),
                )]))
            })
            .with_tool_call_provider_metadata(|metadata| {
                metadata.map(|metadata| {
                    ProviderMetadata::from([(
                        "provider".to_string(),
                        json!({ "custom": metadata.get("custom").expect("custom metadata") })
                            .as_object()
                            .expect("metadata is an object")
                            .clone(),
                    )])
                })
            });

        tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments(r#"{"incomplete"#),
                    ),
            )
            .expect("unfinished metadata tool call starts");

        let tool_call_event = stream_parts_json(tracker.flush())
            .as_array()
            .expect("parts are array")
            .iter()
            .find(|part| part.get("type") == Some(&json!("tool-call")))
            .cloned();

        assert_eq!(
            tool_call_event,
            Some(json!({
                "type": "tool-call",
                "toolCallId": "call_1",
                "toolName": "fn",
                "input": r#"{"incomplete"#,
                "providerMetadata": {
                    "provider": {
                        "custom": { "key": "value" }
                    }
                }
            }))
        );
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_not_include_provider_metadata_when_builder_returns_none()
     {
        let mut tracker = StreamingToolCallTracker::new()
            .with_extract_metadata(|_| None)
            .with_tool_call_provider_metadata(|_| None);

        let parts = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments("{}"),
                    ),
            )
            .expect("metadata-free tool call succeeds");

        let tool_call_event = stream_parts_json(parts)
            .as_array()
            .expect("parts are array")
            .iter()
            .find(|part| part.get("type") == Some(&json!("tool-call")))
            .cloned()
            .expect("tool-call event exists");

        assert_eq!(
            tool_call_event,
            json!({
                "type": "tool-call",
                "toolCallId": "call_1",
                "toolName": "fn",
                "input": "{}"
            })
        );
        assert!(tool_call_event.get("providerMetadata").is_none());
    }

    #[test]
    fn streaming_tool_call_tracker_upstream_should_use_custom_generate_id_for_tool_call_ids_when_id_is_missing_in_fallback()
     {
        let generate_id_calls = Arc::new(AtomicUsize::new(0));
        let generate_id_calls_for_tracker = Arc::clone(&generate_id_calls);
        let mut tracker = StreamingToolCallTracker::new().with_generate_id(move || {
            generate_id_calls_for_tracker.fetch_add(1, Ordering::SeqCst);
            "custom-id".to_string()
        });

        tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments(r#"{"key": "val"#),
                    ),
            )
            .expect("unfinished tool call starts");

        let tool_call_event = stream_parts_json(tracker.flush())
            .as_array()
            .expect("parts are array")
            .iter()
            .find(|part| part.get("type") == Some(&json!("tool-call")))
            .cloned();

        assert_eq!(
            tool_call_event,
            Some(json!({
                "type": "tool-call",
                "toolCallId": "call_1",
                "toolName": "fn",
                "input": r#"{"key": "val"#
            }))
        );
        assert_eq!(generate_id_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn streaming_tool_call_tracker_accumulates_and_finishes_json_arguments() {
        let mut tracker = StreamingToolCallTracker::new();

        let first_parts = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("get_weather")
                            .with_arguments(r#"{"ci"#),
                    ),
            )
            .expect("first delta succeeds");

        assert_eq!(
            serde_json::to_value(first_parts).expect("parts serialize"),
            json!([
                { "type": "tool-input-start", "id": "call_1", "toolName": "get_weather" },
                { "type": "tool-input-delta", "id": "call_1", "delta": r#"{"ci"# }
            ])
        );

        let middle_parts =
            tracker
                .process_delta(StreamingToolCallDelta::new().with_index(0).with_function(
                    StreamingToolCallDeltaFunction::new().with_arguments(r#"ty":"San"#),
                ))
                .expect("middle delta succeeds");

        assert_eq!(
            serde_json::to_value(middle_parts).expect("parts serialize"),
            json!([
                { "type": "tool-input-delta", "id": "call_1", "delta": r#"ty":"San"# }
            ])
        );

        let final_parts = tracker
            .process_delta(StreamingToolCallDelta::new().with_index(0).with_function(
                StreamingToolCallDeltaFunction::new().with_arguments(r#" Francisco"}"#),
            ))
            .expect("final delta succeeds");

        assert_eq!(
            serde_json::to_value(final_parts).expect("parts serialize"),
            json!([
                { "type": "tool-input-delta", "id": "call_1", "delta": r#" Francisco"}"# },
                { "type": "tool-input-end", "id": "call_1" },
                {
                    "type": "tool-call",
                    "toolCallId": "call_1",
                    "toolName": "get_weather",
                    "input": r#"{"city":"San Francisco"}"#
                }
            ])
        );
    }

    #[test]
    fn streaming_tool_call_tracker_flushes_unfinished_tool_calls_once() {
        let mut tracker = StreamingToolCallTracker::new();

        tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments(r#"{"key":"val"#),
                    ),
            )
            .expect("delta succeeds");

        assert_eq!(
            serde_json::to_value(tracker.flush()).expect("flush parts serialize"),
            json!([
                { "type": "tool-input-end", "id": "call_1" },
                {
                    "type": "tool-call",
                    "toolCallId": "call_1",
                    "toolName": "fn",
                    "input": r#"{"key":"val"#
                }
            ])
        );
        assert!(tracker.flush().is_empty());
    }

    #[test]
    fn streaming_tool_call_tracker_validates_required_delta_type_and_fields() {
        let mut tracker = StreamingToolCallTracker::from_options(
            StreamingToolCallTrackerOptions::new()
                .with_type_validation(StreamingToolCallTypeValidation::Required),
        );

        let type_error = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_function(StreamingToolCallDeltaFunction::new().with_name("fn")),
            )
            .expect_err("missing function type is rejected");

        assert_eq!(type_error.to_string(), "Expected 'function' type.");
        assert_eq!(
            type_error.data(),
            &json!({
                "index": 0,
                "id": "call_1",
                "function": { "name": "fn" }
            })
        );

        let mut tracker = StreamingToolCallTracker::new();
        let id_error = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_type("function")
                    .with_function(StreamingToolCallDeltaFunction::new().with_name("fn")),
            )
            .expect_err("missing id is rejected");

        assert_eq!(id_error.to_string(), "Expected 'id' to be a string.");
    }

    #[test]
    fn streaming_tool_call_tracker_attaches_provider_metadata_to_tool_call_events() {
        let mut tracker = StreamingToolCallTracker::new()
            .with_extract_metadata(|delta| {
                let signature = delta
                    .extra
                    .get("extra_content")?
                    .get("google")?
                    .get("thought_signature")?
                    .as_str()?;

                Some(ProviderMetadata::from([(
                    "google".to_string(),
                    json!({ "thoughtSignature": signature })
                        .as_object()
                        .expect("metadata is an object")
                        .clone(),
                )]))
            })
            .with_tool_call_provider_metadata(|metadata| metadata.cloned());

        let parts = tracker
            .process_delta(
                StreamingToolCallDelta::new()
                    .with_index(0)
                    .with_id("call_1")
                    .with_type("function")
                    .with_function(
                        StreamingToolCallDeltaFunction::new()
                            .with_name("fn")
                            .with_arguments("{}"),
                    )
                    .with_extra_value(
                        "extra_content",
                        json!({ "google": { "thought_signature": "sig123" } }),
                    ),
            )
            .expect("delta succeeds");

        assert_eq!(
            serde_json::to_value(parts).expect("parts serialize"),
            json!([
                { "type": "tool-input-start", "id": "call_1", "toolName": "fn" },
                { "type": "tool-input-delta", "id": "call_1", "delta": "{}" },
                { "type": "tool-input-end", "id": "call_1" },
                {
                    "type": "tool-call",
                    "toolCallId": "call_1",
                    "toolName": "fn",
                    "input": "{}",
                    "providerMetadata": {
                        "google": { "thoughtSignature": "sig123" }
                    }
                }
            ])
        );
    }

    #[test]
    fn id_generator_options_serialize_and_deserialize_camel_case_shape() {
        let options = IdGeneratorOptions::new()
            .with_prefix("msg")
            .with_separator("_")
            .with_size(8)
            .with_alphabet("abc");

        assert_eq!(
            serde_json::to_value(&options).expect("options serialize"),
            json!({
                "prefix": "msg",
                "separator": "_",
                "size": 8,
                "alphabet": "abc"
            })
        );
        assert_eq!(
            serde_json::from_value::<IdGeneratorOptions>(json!({}))
                .expect("default options deserialize"),
            IdGeneratorOptions::default()
        );
    }

    #[test]
    fn create_id_generator_creates_random_part_with_configured_size_and_alphabet() {
        let generator =
            create_id_generator(IdGeneratorOptions::new().with_size(12).with_alphabet("ab"))
                .expect("generator is valid");

        let id = generator();

        assert_eq!(id.len(), 12);
        assert!(id.chars().all(|character| "ab".contains(character)));
    }

    #[test]
    fn create_id_generator_adds_prefix_and_separator() {
        let generator = create_id_generator(
            IdGeneratorOptions::new()
                .with_prefix("msg")
                .with_separator("_")
                .with_size(6)
                .with_alphabet("xyz"),
        )
        .expect("generator is valid");

        let id = generator();
        let random_part = id
            .strip_prefix("msg_")
            .expect("prefix and separator are present");

        assert_eq!(random_part.len(), 6);
        assert!(
            random_part
                .chars()
                .all(|character| "xyz".contains(character))
        );
    }

    #[test]
    fn create_id_generator_rejects_separator_inside_alphabet_when_prefixed() {
        let error = match create_id_generator(
            IdGeneratorOptions::new()
                .with_prefix("tool")
                .with_separator("a"),
        ) {
            Ok(_) => panic!("separator in alphabet is invalid when prefixed"),
            Err(error) => error,
        };

        assert_eq!(error.argument(), "separator");
        assert_eq!(
            error.message(),
            "The separator \"a\" must not be part of the alphabet \"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz\"."
        );
    }

    #[test]
    fn create_id_generator_allows_default_separator_without_prefix() {
        let generator = create_id_generator(IdGeneratorOptions::new())
            .expect("default unprefixed generator is valid");

        assert_eq!(generator().len(), 16);
    }

    #[test]
    fn generate_id_uses_upstream_default_random_part_length() {
        let id = generate_id();

        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|character| {
            "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz".contains(character)
        }));
    }

    #[test]
    fn create_id_generator_upstream_generates_id_with_correct_length() {
        let id_generator =
            create_id_generator(IdGeneratorOptions::new().with_size(10)).expect("generator valid");

        let id = id_generator();

        assert_eq!(id.len(), 10);
    }

    #[test]
    fn create_id_generator_upstream_generates_id_with_correct_default_length() {
        let id_generator = create_id_generator(IdGeneratorOptions::new()).expect("generator valid");

        let id = id_generator();

        assert_eq!(id.len(), 16);
    }

    #[test]
    fn create_id_generator_upstream_throws_error_when_separator_is_part_of_alphabet() {
        let error = match create_id_generator(
            IdGeneratorOptions::new()
                .with_separator("a")
                .with_prefix("b"),
        ) {
            Ok(_) => panic!("separator in alphabet is invalid when prefixed"),
            Err(error) => error,
        };

        assert_eq!(error.argument(), "separator");
        assert_eq!(
            error.message(),
            "The separator \"a\" must not be part of the alphabet \"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz\"."
        );
    }

    #[test]
    fn generate_id_upstream_generates_unique_ids() {
        let id1 = generate_id();
        let id2 = generate_id();

        assert_ne!(id1, id2);
    }

    #[test]
    fn is_provider_reference_accepts_plain_records() {
        assert!(is_provider_reference(&json!({
            "openai": "file-abc123"
        })));
        assert!(is_provider_reference(&json!({
            "fileId": "abc"
        })));
    }

    #[test]
    fn is_provider_reference_rejects_tagged_file_data_objects() {
        assert!(!is_provider_reference(&json!({
            "type": "reference",
            "reference": {
                "fileId": "abc"
            }
        })));
        assert!(!is_provider_reference(&json!({
            "type": "data",
            "data": "x"
        })));
    }

    #[test]
    fn is_provider_reference_rejects_non_objects_and_arrays() {
        assert!(!is_provider_reference(&JsonValue::Null));
        assert!(!is_provider_reference(&json!("some-string")));
        assert!(!is_provider_reference(&json!(42)));
        assert!(!is_provider_reference(&json!([1, 2, 3])));
    }

    #[test]
    fn is_provider_reference_upstream_returns_true_for_plain_record_of_provider_ids() {
        assert!(is_provider_reference(&json!({
            "openai": "file-abc123"
        })));
    }

    #[test]
    fn is_provider_reference_upstream_returns_true_for_record_with_single_file_id_like_key() {
        assert!(is_provider_reference(&json!({
            "fileId": "abc"
        })));
    }

    #[test]
    fn is_provider_reference_upstream_returns_false_for_object_carrying_type_property() {
        assert!(!is_provider_reference(&json!({
            "type": "reference",
            "reference": {
                "fileId": "abc"
            }
        })));
    }

    #[test]
    fn is_provider_reference_upstream_returns_false_for_tagged_data_object() {
        assert!(!is_provider_reference(&json!({
            "type": "data",
            "data": "x"
        })));
    }

    #[test]
    fn is_provider_reference_upstream_returns_false_for_uint8_array_json_boundary() {
        assert!(!is_provider_reference(&json!([1, 2, 3])));
    }

    #[test]
    fn is_provider_reference_upstream_returns_false_for_null() {
        assert!(!is_provider_reference(&JsonValue::Null));
    }

    #[test]
    fn is_provider_reference_upstream_returns_false_for_string_primitive() {
        assert!(!is_provider_reference(&json!("some-string")));
    }

    #[test]
    fn is_provider_reference_upstream_returns_false_for_number_primitive() {
        assert!(!is_provider_reference(&json!(42)));
    }

    #[test]
    fn validate_types_returns_validated_values() {
        let value = json!({ "name": "John", "age": 30 });

        let person = validate_types(value, person_schema(), None).expect("person validates");

        assert_eq!(
            person,
            Person {
                name: "John".to_string(),
                age: 30,
            }
        );
    }

    #[test]
    fn validate_types_upstream_should_return_validated_object_for_valid_input() {
        let input = json!({ "name": "John", "age": 30 });

        let person = validate_types(input, person_schema(), None).expect("person validates");

        assert_eq!(
            person,
            Person {
                name: "John".to_string(),
                age: 30,
            }
        );
    }

    #[test]
    fn validate_types_wraps_validation_errors_with_context() {
        let value = json!({ "name": "John", "age": "30" });
        let context = TypeValidationContext::new()
            .with_field("person.age")
            .with_entity_name("person")
            .with_entity_id("user-1");

        let error = validate_types(value.clone(), person_schema(), Some(context.clone()))
            .expect_err("invalid person should fail validation");

        assert_eq!(error.value(), &value);
        assert_eq!(error.context(), Some(&context));
        assert_eq!(error.cause_message(), "Invalid input");
        assert!(
            error
                .message()
                .starts_with("Type validation failed for person.age (person, id: \"user-1\"):")
        );
    }

    #[test]
    fn validate_types_upstream_should_throw_type_validation_error_for_invalid_input() {
        let input = json!({ "name": "John", "age": "30" });

        let error =
            validate_types(input.clone(), person_schema(), None).expect_err("input is invalid");

        assert_eq!(error.value(), &input);
        assert_eq!(error.cause_message(), "Invalid input");
        assert!(error.message().contains("Type validation failed"));
    }

    #[test]
    fn safe_validate_types_preserves_raw_value_after_transformation() {
        let value = json!({ "count": "42" });
        let schema = Schema::new(object_schema()).with_validator(|value| {
            match value
                .get("count")
                .and_then(JsonValue::as_str)
                .and_then(|count| count.parse::<u64>().ok())
            {
                Some(count) => ValidationResult::success(json!({ "count": count })),
                None => ValidationResult::failure("Expected numeric string"),
            }
        });

        let parsed = safe_validate_types(value.clone(), schema, None);

        assert_eq!(
            parsed,
            ValidateTypesResult::success(json!({ "count": 42 }), value.clone())
        );
        assert!(parsed.is_success());
        assert!(!parsed.is_failure());
        assert_eq!(parsed.value(), Some(&json!({ "count": 42 })));
        assert_eq!(parsed.raw_value(), &value);
        assert!(parsed.error().is_none());
    }

    #[test]
    fn safe_validate_types_upstream_should_return_validated_object_for_valid_input() {
        let input = json!({ "name": "John", "age": 30 });

        let result = safe_validate_types(input.clone(), person_schema(), None);

        assert_eq!(
            result,
            ValidateTypesResult::success(
                Person {
                    name: "John".to_string(),
                    age: 30,
                },
                input,
            )
        );
    }

    #[test]
    fn safe_validate_types_returns_error_and_raw_value_on_failure() {
        let value = json!({ "name": "John", "age": "30" });
        let parsed = safe_validate_types(value.clone(), person_schema(), None);

        assert!(parsed.is_failure());
        assert!(parsed.value().is_none());
        assert_eq!(parsed.raw_value(), &value);

        let error = parsed.error().expect("validation error is returned");
        assert_eq!(error.value(), &value);
        assert_eq!(error.cause_message(), "Invalid input");
    }

    #[test]
    fn safe_validate_types_upstream_should_return_error_object_for_invalid_input() {
        let input = json!({ "name": "John", "age": "30" });

        let result = safe_validate_types(input.clone(), person_schema(), None);

        assert!(result.is_failure());
        assert_eq!(result.raw_value(), &input);

        let error = result.error().expect("validation error is returned");
        assert_eq!(error.value(), &input);
        assert!(error.message().contains("Type validation failed"));
    }

    #[test]
    fn safe_validate_types_passes_through_json_when_schema_has_no_validator() {
        let value = json!({ "name": "John", "age": 30 });

        let parsed: ValidateTypesResult<JsonValue> =
            safe_validate_types(value.clone(), json_schema(object_schema()), None);

        assert_eq!(parsed, ValidateTypesResult::success(value.clone(), value));
    }

    #[test]
    fn parse_json_with_schema_returns_validated_values() {
        let person = parse_json_with_schema(r#"{"name":"John","age":30}"#, person_schema())
            .expect("JSON parses and validates");

        assert_eq!(
            person,
            Person {
                name: "John".to_string(),
                age: 30
            }
        );
    }

    #[test]
    fn parse_json_with_schema_wraps_type_validation_errors() {
        let error = parse_json_with_schema(r#"{"name":"John","age":"old"}"#, person_schema())
            .expect_err("invalid typed JSON fails validation");

        let validation_error = error
            .as_type_validation_error()
            .expect("schema failure is returned");
        assert_eq!(
            validation_error.value(),
            &json!({ "name": "John", "age": "old" })
        );
        assert_eq!(validation_error.cause_message(), "Invalid input");
        assert!(error.as_json_parse_error().is_none());
    }

    #[test]
    fn safe_parse_json_with_schema_preserves_raw_value_after_transformation() {
        let schema = Schema::new(object_schema()).with_validator(|value| {
            let count = value
                .get("count")
                .and_then(JsonValue::as_str)
                .and_then(|count| count.parse::<u64>().ok())
                .expect("test input has a numeric count string");

            ValidationResult::success(json!({ "count": count }))
        });

        let parsed = safe_parse_json_with_schema(r#"{"count":"42"}"#, schema);

        assert_eq!(
            parsed,
            ParseJsonResult::success(json!({ "count": 42 }), json!({ "count": "42" }))
        );
    }

    #[test]
    fn safe_parse_json_with_schema_preserves_raw_value_on_validation_failure() {
        let parsed = safe_parse_json_with_schema(r#"{"name":"John","age":"old"}"#, person_schema());

        assert!(parsed.is_failure());
        assert_eq!(
            parsed.raw_value(),
            Some(&json!({ "name": "John", "age": "old" }))
        );

        let validation_error = parsed
            .error()
            .and_then(ParseJsonError::as_type_validation_error)
            .expect("schema failure is returned");
        assert_eq!(validation_error.cause_message(), "Invalid input");
    }

    #[test]
    fn safe_parse_json_with_schema_has_no_raw_value_on_parse_failure() {
        let parsed: ParseJsonResult<Person> =
            safe_parse_json_with_schema("invalid json", person_schema());

        assert!(parsed.is_failure());
        assert!(parsed.raw_value().is_none());
        assert!(
            parsed
                .error()
                .and_then(ParseJsonError::as_json_parse_error)
                .is_some()
        );
    }

    #[test]
    fn parse_provider_options_returns_none_for_missing_provider_options() {
        let provider_options = BTreeMap::from([(
            "openai".to_string(),
            json!({ "name": "John", "age": 30 })
                .as_object()
                .expect("provider options are an object")
                .clone(),
        )]);

        assert_eq!(
            parse_provider_options("anthropic", Some(&provider_options), |_| {
                Result::<Person, &'static str>::Err("validator should not run")
            })
            .expect("missing provider options are ignored"),
            None
        );
        assert_eq!(
            parse_provider_options("openai", None, |_| {
                Result::<Person, &'static str>::Err("validator should not run")
            })
            .expect("missing provider options map is ignored"),
            None
        );
    }

    #[test]
    fn parse_provider_options_returns_validated_provider_options() {
        let provider_options = BTreeMap::from([(
            "openai".to_string(),
            json!({ "name": "John", "age": 30 })
                .as_object()
                .expect("provider options are an object")
                .clone(),
        )]);

        assert_eq!(
            parse_provider_options("openai", Some(&provider_options), validate_person)
                .expect("provider options validate"),
            Some(Person {
                name: "John".to_string(),
                age: 30,
            })
        );
    }

    #[test]
    fn parse_provider_options_reports_invalid_argument_on_validation_failure() {
        let provider_options = BTreeMap::from([(
            "openai".to_string(),
            json!({ "name": "John", "age": "30" })
                .as_object()
                .expect("provider options are an object")
                .clone(),
        )]);

        let error = parse_provider_options("openai", Some(&provider_options), validate_person)
            .expect_err("invalid provider options are rejected");

        assert_eq!(error.argument(), "providerOptions");
        assert_eq!(error.message(), "invalid openai provider options");
    }

    fn upstream_foo_string_schema() -> Schema<JsonValue> {
        Schema::new(schema_object(json!({
            "type": "object",
            "properties": {
                "foo": { "type": "string" }
            },
            "required": ["foo"]
        })))
        .with_validator(|value| match value.get("foo").and_then(JsonValue::as_str) {
            Some(_) => ValidationResult::success(value.clone()),
            None => ValidationResult::failure("Invalid input"),
        })
    }

    fn upstream_age_number_schema() -> Schema<JsonValue> {
        Schema::new(schema_object(json!({
            "type": "object",
            "properties": {
                "age": { "type": "number" }
            },
            "required": ["age"]
        })))
        .with_validator(|value| match value.get("age").and_then(JsonValue::as_f64) {
            Some(_) => ValidationResult::success(value.clone()),
            None => ValidationResult::failure("Invalid input"),
        })
    }

    #[test]
    fn parse_json_upstream_should_parse_basic_json_without_schema() {
        let result = parse_json(r#"{"foo": "bar"}"#).expect("JSON parses");

        assert_eq!(result, json!({ "foo": "bar" }));
    }

    #[test]
    fn parse_json_upstream_should_parse_json_with_schema_validation() {
        let result = parse_json_with_schema(r#"{"foo": "bar"}"#, upstream_foo_string_schema())
            .expect("JSON parses and validates");

        assert_eq!(result, json!({ "foo": "bar" }));
    }

    #[test]
    fn parse_json_upstream_should_throw_json_parse_error_for_invalid_json() {
        let error = parse_json("invalid json").expect_err("invalid JSON fails");

        assert_eq!(error.text(), "invalid json");
    }

    #[test]
    fn parse_json_upstream_should_throw_type_validation_error_for_schema_validation_failures() {
        let error = parse_json_with_schema(r#"{"foo": "bar"}"#, upstream_age_number_schema())
            .expect_err("schema validation fails");

        let validation_error = error
            .as_type_validation_error()
            .expect("schema failure is returned");
        assert_eq!(validation_error.value(), &json!({ "foo": "bar" }));
    }

    #[test]
    fn safe_parse_json_upstream_should_safely_parse_basic_json_without_schema_and_include_raw_value()
     {
        let result = safe_parse_json(r#"{"foo": "bar"}"#);

        assert_eq!(
            result,
            ParseJsonResult::success(json!({ "foo": "bar" }), json!({ "foo": "bar" }))
        );
    }

    #[test]
    fn safe_parse_json_upstream_should_preserve_raw_value_even_after_schema_transformation() {
        let schema = Schema::new(schema_object(json!({
            "type": "object",
            "properties": {
                "count": { "type": "string" }
            },
            "required": ["count"]
        })))
        .with_validator(|value| {
            let count = value
                .get("count")
                .and_then(JsonValue::as_str)
                .and_then(|count| count.parse::<u64>().ok())
                .expect("test input has a numeric count string");

            ValidationResult::success(json!({ "count": count }))
        });

        let result = safe_parse_json_with_schema(r#"{"count": "42"}"#, schema);

        assert_eq!(
            result,
            ParseJsonResult::success(json!({ "count": 42 }), json!({ "count": "42" }))
        );
    }

    #[test]
    fn safe_parse_json_upstream_should_handle_failed_parsing_with_error_details() {
        let result = safe_parse_json("invalid json");

        assert!(result.is_failure());
        assert!(result.raw_value().is_none());
        assert!(
            result
                .error()
                .and_then(ParseJsonError::as_json_parse_error)
                .is_some()
        );
    }

    #[test]
    fn safe_parse_json_upstream_should_handle_schema_validation_failures() {
        let result =
            safe_parse_json_with_schema(r#"{"age": "twenty"}"#, upstream_age_number_schema());

        assert!(result.is_failure());
        assert_eq!(result.raw_value(), Some(&json!({ "age": "twenty" })));
        assert!(
            result
                .error()
                .and_then(ParseJsonError::as_type_validation_error)
                .is_some()
        );
    }

    #[test]
    fn safe_parse_json_upstream_should_handle_nested_objects_and_preserve_raw_values() {
        let schema = Schema::new(schema_object(json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "name": { "type": "string" }
                    },
                    "required": ["id", "name"]
                }
            },
            "required": ["user"]
        })))
        .with_validator(|value| {
            let user = value
                .get("user")
                .and_then(JsonValue::as_object)
                .expect("test input has user object");
            let id = user
                .get("id")
                .and_then(JsonValue::as_str)
                .and_then(|id| id.parse::<u64>().ok())
                .expect("test input has numeric string id");
            let name = user
                .get("name")
                .and_then(JsonValue::as_str)
                .expect("test input has name");

            ValidationResult::success(json!({ "user": { "id": id, "name": name } }))
        });

        let result =
            safe_parse_json_with_schema(r#"{"user": {"id": "123", "name": "John"}}"#, schema);

        assert_eq!(
            result,
            ParseJsonResult::success(
                json!({ "user": { "id": 123, "name": "John" } }),
                json!({ "user": { "id": "123", "name": "John" } })
            )
        );
    }

    #[test]
    fn safe_parse_json_upstream_should_handle_arrays_and_preserve_raw_values() {
        let schema = Schema::new(schema_object(json!({
            "type": "array",
            "items": { "type": "string" }
        })))
        .with_validator(|value| {
            let values = value.as_array().expect("test input is an array");
            let uppercased = values
                .iter()
                .map(|value| {
                    JsonValue::String(
                        value
                            .as_str()
                            .expect("test input has string array item")
                            .to_uppercase(),
                    )
                })
                .collect::<Vec<_>>();

            ValidationResult::success(JsonValue::Array(uppercased))
        });

        let result = safe_parse_json_with_schema(r#"["hello", "world"]"#, schema);

        assert_eq!(
            result,
            ParseJsonResult::success(json!(["HELLO", "WORLD"]), json!(["hello", "world"]))
        );
    }

    #[test]
    fn safe_parse_json_upstream_should_handle_discriminated_unions_in_schema() {
        let schema = Schema::new(schema_object(json!({
            "anyOf": [
                {
                    "type": "object",
                    "properties": {
                        "type": { "const": "text" },
                        "content": { "type": "string" }
                    },
                    "required": ["type", "content"]
                },
                {
                    "type": "object",
                    "properties": {
                        "type": { "const": "number" },
                        "value": { "type": "number" }
                    },
                    "required": ["type", "value"]
                }
            ]
        })))
        .with_validator(
            |value| match value.get("type").and_then(JsonValue::as_str) {
                Some("text") if value.get("content").and_then(JsonValue::as_str).is_some() => {
                    ValidationResult::success(value.clone())
                }
                Some("number") if value.get("value").and_then(JsonValue::as_f64).is_some() => {
                    ValidationResult::success(value.clone())
                }
                _ => ValidationResult::failure("Invalid input"),
            },
        );

        let result = safe_parse_json_with_schema(r#"{"type": "text", "content": "hello"}"#, schema);

        assert_eq!(
            result,
            ParseJsonResult::success(
                json!({ "type": "text", "content": "hello" }),
                json!({ "type": "text", "content": "hello" })
            )
        );
    }

    #[test]
    fn safe_parse_json_upstream_should_handle_nullable_fields_in_schema() {
        let schema = Schema::new(schema_object(json!({
            "type": "object",
            "properties": {
                "id": {
                    "anyOf": [{ "type": "string" }, { "type": "null" }]
                },
                "data": { "type": "string" }
            },
            "required": ["data"]
        })))
        .with_validator(
            |value| match value.get("data").and_then(JsonValue::as_str) {
                Some(_) => ValidationResult::success(value.clone()),
                None => ValidationResult::failure("Invalid input"),
            },
        );

        let result = safe_parse_json_with_schema(r#"{"id": null, "data": "test"}"#, schema);

        assert_eq!(
            result,
            ParseJsonResult::success(
                json!({ "id": null, "data": "test" }),
                json!({ "id": null, "data": "test" })
            )
        );
    }

    #[test]
    fn safe_parse_json_upstream_should_handle_union_types_in_schema() {
        let schema = Schema::new(schema_object(json!({
            "type": "object",
            "properties": {
                "value": {
                    "anyOf": [{ "type": "string" }, { "type": "number" }]
                }
            },
            "required": ["value"]
        })))
        .with_validator(|value| {
            let Some(value_property) = value.get("value") else {
                return ValidationResult::failure("Invalid input");
            };

            if value_property.is_string() || value_property.is_number() {
                ValidationResult::success(value.clone())
            } else {
                ValidationResult::failure("Invalid input")
            }
        });

        let string_result = safe_parse_json_with_schema(r#"{"value": "test"}"#, schema.clone());
        let number_result = safe_parse_json_with_schema(r#"{"value": 123}"#, schema);

        assert_eq!(
            string_result,
            ParseJsonResult::success(json!({ "value": "test" }), json!({ "value": "test" }))
        );
        assert_eq!(
            number_result,
            ParseJsonResult::success(json!({ "value": 123 }), json!({ "value": 123 }))
        );
    }

    #[test]
    fn is_parsable_json_upstream_should_return_true_for_valid_json() {
        assert!(is_parsable_json(r#"{"foo": "bar"}"#));
        assert!(is_parsable_json("[1, 2, 3]"));
        assert!(is_parsable_json(r#""hello""#));
    }

    #[test]
    fn is_parsable_json_upstream_should_return_false_for_invalid_json() {
        assert!(!is_parsable_json("invalid"));
        assert!(!is_parsable_json(r#"{foo: "bar"}"#));
        assert!(!is_parsable_json(r#"{"foo": }"#));
    }

    #[test]
    fn parse_json_parses_json_values_without_schema() {
        assert_eq!(
            parse_json(r#"{"foo":"bar","items":[1,true,null]}"#).expect("JSON parses"),
            json!({
                "foo": "bar",
                "items": [1, true, null],
            })
        );
        assert_eq!(parse_json("0").expect("number JSON parses"), json!(0));
        assert_eq!(
            parse_json(r#""hello""#).expect("string JSON parses"),
            json!("hello")
        );
    }

    fn expect_secure_json_parse_rejected(text: &str) {
        let error = secure_json_parse(text).expect_err("secure JSON parse rejects text");

        assert_eq!(
            error.to_string(),
            "Object contains forbidden prototype property"
        );
    }

    #[test]
    fn secure_json_parse_upstream_parses_object_string() {
        assert_eq!(
            secure_json_parse(r#"{"a": 5, "b": 6}"#).expect("JSON parses"),
            json!({ "a": 5, "b": 6 })
        );
    }

    #[test]
    fn secure_json_parse_upstream_parses_null_string() {
        assert_eq!(
            secure_json_parse("null").expect("JSON parses"),
            JsonValue::Null
        );
    }

    #[test]
    fn secure_json_parse_upstream_parses_zero_string() {
        assert_eq!(secure_json_parse("0").expect("JSON parses"), json!(0));
    }

    #[test]
    fn secure_json_parse_upstream_parses_string_string() {
        assert_eq!(
            secure_json_parse(r#""X""#).expect("JSON parses"),
            json!("X")
        );
    }

    #[test]
    fn secure_json_parse_upstream_allows_constructor_property_with_non_object_value() {
        assert_eq!(
            secure_json_parse(r#"{ "constructor": "string value" }"#).expect("JSON parses"),
            json!({ "constructor": "string value" })
        );
    }

    #[test]
    fn secure_json_parse_upstream_allows_constructor_property_with_null_value() {
        assert_eq!(
            secure_json_parse(r#"{ "constructor": null }"#).expect("JSON parses"),
            json!({ "constructor": null })
        );
    }

    #[test]
    fn secure_json_parse_upstream_errors_on_constructor_property() {
        expect_secure_json_parse_rejected(
            r#"{ "a": 5, "b": 6, "constructor": { "x": 7 }, "c": { "d": 0, "e": "text", "__proto__": { "y": 8 }, "f": { "g": 2 } } }"#,
        );
    }

    #[test]
    fn secure_json_parse_upstream_errors_on_proto_property() {
        expect_secure_json_parse_rejected(
            r#"{ "a": 5, "b": 6, "__proto__": { "x": 7 }, "c": { "d": 0, "e": "text", "__proto__": { "y": 8 }, "f": { "g": 2 } } }"#,
        );
    }

    #[test]
    fn secure_json_parse_upstream_errors_on_unicode_escaped_proto_property() {
        expect_secure_json_parse_rejected(r#"{ "\u005f\u005fproto__": { "isAdmin": true } }"#);
    }

    #[test]
    fn secure_json_parse_upstream_errors_on_fully_unicode_escaped_proto_property() {
        expect_secure_json_parse_rejected(
            r#"{ "\u005f\u005f\u0070\u0072\u006f\u0074\u006f\u005f\u005f": { "isAdmin": true } }"#,
        );
    }

    #[test]
    fn secure_json_parse_upstream_errors_on_unicode_escaped_constructor_property() {
        expect_secure_json_parse_rejected(
            r#"{ "\u0063\u006fnstructor": { "prototype": { "isAdmin": true } } }"#,
        );
    }

    #[test]
    fn secure_json_parse_upstream_errors_on_fully_unicode_escaped_constructor_property() {
        expect_secure_json_parse_rejected(
            r#"{ "\u0063\u006f\u006e\u0073\u0074\u0072\u0075\u0063\u0074\u006f\u0072": { "prototype": { "isAdmin": true } } }"#,
        );
    }

    #[test]
    fn parse_json_wraps_invalid_json_in_provider_error() {
        let error = parse_json("invalid json").expect_err("invalid JSON fails");

        assert_eq!(error.text(), "invalid json");
        assert!(
            error
                .message()
                .starts_with("JSON parsing failed: Text: invalid json.\nError message:")
        );
    }

    #[test]
    fn parse_json_rejects_proto_properties() {
        let error = parse_json(r#"{ "a": 5, "c": { "d": 0, "__proto__": { "isAdmin": true } } }"#)
            .expect_err("prototype keys are rejected");

        assert_eq!(
            error.cause_message(),
            "Object contains forbidden prototype property"
        );
    }

    #[test]
    fn parse_json_rejects_constructor_prototype_objects() {
        let error = parse_json(r#"{ "constructor": { "prototype": { "isAdmin": true } } }"#)
            .expect_err("constructor prototype objects are rejected");

        assert_eq!(
            error.cause_message(),
            "Object contains forbidden prototype property"
        );
    }

    #[test]
    fn parse_json_allows_safe_constructor_properties() {
        assert_eq!(
            parse_json(r#"{ "constructor": "string value" }"#).expect("JSON parses"),
            json!({ "constructor": "string value" })
        );
        assert_eq!(
            parse_json(r#"{ "constructor": null }"#).expect("JSON parses"),
            json!({ "constructor": null })
        );
    }

    #[test]
    fn safe_parse_json_returns_success_with_raw_value() {
        let parsed = safe_parse_json(r#"{"foo":"bar","items":[1,true,null]}"#);
        let expected_value = json!({
            "foo": "bar",
            "items": [1, true, null],
        });

        assert_eq!(
            parsed,
            ParseJsonResult::success(expected_value.clone(), expected_value.clone())
        );
        assert!(parsed.is_success());
        assert!(!parsed.is_failure());
        assert_eq!(parsed.value(), Some(&expected_value));
        assert_eq!(parsed.raw_value(), Some(&expected_value));
        assert!(parsed.error().is_none());
    }

    #[test]
    fn safe_parse_json_returns_json_parse_error_without_raw_value_on_invalid_json() {
        let parsed = safe_parse_json("invalid json");

        assert!(parsed.is_failure());
        assert!(parsed.value().is_none());
        assert!(parsed.raw_value().is_none());

        let error = parsed.error().expect("parse error is returned");
        let json_parse_error = error
            .as_json_parse_error()
            .expect("failure is a JSON parse error");
        assert_eq!(json_parse_error.text(), "invalid json");
        assert!(
            json_parse_error
                .message()
                .starts_with("JSON parsing failed: Text: invalid json.\nError message:")
        );
    }

    #[test]
    fn safe_parse_json_returns_json_parse_error_for_forbidden_prototype_properties() {
        let parsed = safe_parse_json(r#"{ "__proto__": { "isAdmin": true } }"#);
        let error = parsed.error().expect("parse error is returned");

        assert_eq!(
            error
                .as_json_parse_error()
                .expect("secure parse failure uses JSON parse error")
                .cause_message(),
            "Object contains forbidden prototype property"
        );
        assert!(parsed.raw_value().is_none());
    }

    #[test]
    fn parse_json_error_can_wrap_type_validation_failures() {
        let validation_error =
            TypeValidationError::new(json!({ "age": "30" }), "Expected number", None);
        let parse_error = ParseJsonError::from(validation_error.clone());

        assert_eq!(
            parse_error.as_type_validation_error(),
            Some(&validation_error)
        );
        assert!(parse_error.as_json_parse_error().is_none());
        assert_eq!(parse_error.to_string(), validation_error.to_string());
    }

    #[test]
    fn is_parsable_json_matches_secure_parse_result() {
        assert!(is_parsable_json(r#"{"foo":"bar"}"#));
        assert!(is_parsable_json("[1,2,3]"));
        assert!(is_parsable_json(r#""hello""#));
        assert!(!is_parsable_json("invalid"));
        assert!(!is_parsable_json(r#"{ "foo": }"#));
        assert!(!is_parsable_json(
            r#"{ "\u005f\u005fproto__": { "isAdmin": true } }"#
        ));
    }

    #[test]
    fn convert_inline_file_data_to_bytes_encodes_text_as_utf8() {
        assert_eq!(
            convert_inline_file_data_to_bytes(&FileData::Text {
                text: "hello\nworld".to_string(),
            })
            .expect("text data converts"),
            b"hello\nworld".to_vec()
        );
    }

    #[test]
    fn convert_inline_file_data_to_bytes_returns_raw_bytes_unchanged() {
        assert_eq!(
            convert_inline_file_data_to_bytes(&FileData::Data {
                data: FileDataContent::Bytes(vec![0, 1, 2, 255]),
            })
            .expect("raw bytes convert"),
            vec![0, 1, 2, 255]
        );
    }

    #[test]
    fn convert_inline_file_data_to_bytes_decodes_base64_data() {
        assert_eq!(
            convert_inline_file_data_to_bytes(&FileData::Data {
                data: FileDataContent::Base64("SGVsbG8=".to_string()),
            })
            .expect("base64 data converts"),
            b"Hello".to_vec()
        );
        assert_eq!(
            convert_inline_file_data_to_bytes(&FileData::Data {
                data: FileDataContent::Base64("-_8=".to_string()),
            })
            .expect("base64url data converts"),
            vec![251, 255]
        );
    }

    #[test]
    fn convert_inline_file_data_to_bytes_rejects_non_inline_file_data() {
        assert_eq!(
            convert_inline_file_data_to_bytes(&FileData::Url {
                url: Url::parse("https://example.com/file.txt").expect("valid URL"),
            })
            .expect_err("URL file data is not inline"),
            InlineFileDataBytesError::NonInlineFileData
        );

        let reference = ProviderReference::try_from(BTreeMap::from([(
            "openai".to_string(),
            "file-abc123".to_string(),
        )]))
        .expect("provider reference is valid");

        assert_eq!(
            convert_inline_file_data_to_bytes(&FileData::Reference { reference })
                .expect_err("provider references are not inline"),
            InlineFileDataBytesError::NonInlineFileData
        );
    }

    #[test]
    fn convert_inline_file_data_to_bytes_rejects_invalid_base64_data() {
        assert_eq!(
            convert_inline_file_data_to_bytes(&FileData::Data {
                data: FileDataContent::Base64("not valid base64!".to_string()),
            })
            .expect_err("invalid base64 does not convert"),
            InlineFileDataBytesError::InvalidBase64Data
        );
    }

    #[test]
    fn convert_base64_to_bytes_decodes_standard_and_url_safe_data() {
        assert_eq!(
            convert_base64_to_bytes("SGVsbG8=").expect("base64 decodes"),
            b"Hello".to_vec()
        );
        assert_eq!(
            convert_base64_to_bytes("-_8=").expect("base64url decodes"),
            vec![251, 255]
        );
        assert_eq!(
            convert_base64_to_bytes("SG V sb\tG8=\n").expect("whitespace is ignored"),
            b"Hello".to_vec()
        );
    }

    #[test]
    fn convert_base64_to_bytes_rejects_invalid_data() {
        assert_eq!(
            convert_base64_to_bytes("not valid base64!").expect_err("invalid data fails"),
            Base64DecodeError
        );
    }

    #[test]
    fn convert_bytes_to_base64_encodes_raw_bytes() {
        assert_eq!(convert_bytes_to_base64(b"Hello"), "SGVsbG8=");
        assert_eq!(convert_bytes_to_base64(&[251, 255]), "+/8=");
        assert_eq!(convert_bytes_to_base64(&[]), "");
    }

    #[test]
    fn convert_to_base64_passes_strings_through_and_encodes_bytes() {
        assert_eq!(
            convert_to_base64(&FileDataContent::Base64("already-base64".to_string())),
            "already-base64"
        );
        assert_eq!(
            convert_to_base64(&FileDataContent::Bytes(b"Hello".to_vec())),
            "SGVsbG8="
        );
    }

    #[test]
    fn get_top_level_media_type_matches_upstream_edge_cases() {
        assert_eq!(get_top_level_media_type("image/png"), "image");
        assert_eq!(get_top_level_media_type("audio/*"), "audio");
        assert_eq!(get_top_level_media_type("text"), "text");
        assert_eq!(get_top_level_media_type(""), "");
        assert_eq!(get_top_level_media_type("/"), "");
        assert_eq!(get_top_level_media_type("image/"), "image");
    }

    #[test]
    fn is_full_media_type_requires_concrete_subtype() {
        assert!(is_full_media_type("image/png"));
        assert!(is_full_media_type("application/pdf"));
        assert!(!is_full_media_type("image"));
        assert!(!is_full_media_type("image/*"));
        assert!(!is_full_media_type("image/"));
        assert!(!is_full_media_type("/"));
    }

    #[test]
    fn is_url_supported_matches_media_type_and_url_patterns() {
        let supported_urls = BTreeMap::from([
            (
                "text/plain".to_string(),
                vec![r"^https://docs\.example\.com/.+\.txt$".to_string()],
            ),
            (
                "image/png".to_string(),
                vec![r"^https://images\.example\.com/.+".to_string()],
            ),
        ]);

        assert!(is_url_supported(
            "text/plain",
            "https://docs.example.com/readme.txt",
            &supported_urls
        ));
        assert!(!is_url_supported(
            "text/plain",
            "https://docs.example.com/readme.md",
            &supported_urls
        ));
        assert!(!is_url_supported(
            "image/png",
            "https://docs.example.com/readme.txt",
            &supported_urls
        ));
    }

    #[test]
    fn is_url_supported_matches_wildcards_and_top_level_media_types() {
        let supported_urls = BTreeMap::from([
            (
                "image/*".to_string(),
                vec![r"^https://cdn\.example\.com/images/".to_string()],
            ),
            (
                "*/*".to_string(),
                vec![r"^https://public\.example\.com/".to_string()],
            ),
        ]);

        assert!(is_url_supported(
            "image/png",
            "https://cdn.example.com/images/cat.png",
            &supported_urls
        ));
        assert!(is_url_supported(
            "image",
            "https://cdn.example.com/images/cat.png",
            &supported_urls
        ));
        assert!(is_url_supported(
            "video/mp4",
            "https://public.example.com/video.mp4",
            &supported_urls
        ));
        assert!(!is_url_supported(
            "audio",
            "https://cdn.example.com/images/cat.png",
            &supported_urls
        ));
    }

    #[test]
    fn is_url_supported_lowercases_media_type_keys_and_urls_before_matching() {
        let supported_urls = BTreeMap::from([(
            "TEXT/PLAIN".to_string(),
            vec![r"^https://example\.com/path$".to_string()],
        )]);

        assert!(is_url_supported(
            "text/plain",
            "https://EXAMPLE.com/PATH",
            &supported_urls
        ));
    }

    #[test]
    fn is_url_supported_ignores_invalid_regex_sources() {
        let supported_urls = BTreeMap::from([(
            "*".to_string(),
            vec!["[".to_string(), r"^https://example\.com$".to_string()],
        )]);

        assert!(is_url_supported(
            "text/plain",
            "https://example.com",
            &supported_urls
        ));
        assert!(!is_url_supported(
            "text/plain",
            "https://another.example.com",
            &supported_urls
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_false_when_model_does_not_support_any_urls() {
        assert!(!is_url_supported(
            "text/plain",
            "https://example.com",
            &BTreeMap::new()
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_true_for_exact_media_type_and_exact_url_match() {
        assert!(is_url_supported(
            "text/plain",
            "https://example.com",
            &supported_urls(vec![("text/plain", vec![r"https://example\.com"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_true_for_exact_media_type_and_regex_url_match() {
        assert!(is_url_supported(
            "image/png",
            "https://images.example.com/cat.png",
            &supported_urls(vec![(
                "image/png",
                vec![r"https://images\.example\.com/.+"]
            )])
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_true_for_one_of_multiple_regex_url_matches() {
        assert!(is_url_supported(
            "image/png",
            "https://another.com/img.png",
            &supported_urls(vec![(
                "image/png",
                vec![
                    r"https://images\.example\.com/.+",
                    r"https://another\.com/img\.png"
                ]
            )])
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_false_for_exact_media_type_but_url_mismatch() {
        assert!(!is_url_supported(
            "text/plain",
            "https://another.com",
            &supported_urls(vec![("text/plain", vec![r"https://example\.com"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_false_for_url_match_but_media_type_mismatch() {
        assert!(!is_url_supported(
            "image/png",
            "https://example.com",
            &supported_urls(vec![("text/plain", vec![r"https://example\.com"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_true_for_wildcard_media_type_and_exact_url_match() {
        assert!(is_url_supported(
            "text/plain",
            "https://example.com",
            &supported_urls(vec![("*", vec![r"https://example\.com"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_true_for_wildcard_media_type_and_regex_url_match() {
        assert!(is_url_supported(
            "image/jpeg",
            "https://images.example.com/dog.jpg",
            &supported_urls(vec![("*", vec![r"https://images\.example\.com/.+"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_false_for_wildcard_media_type_but_url_mismatch() {
        assert!(!is_url_supported(
            "video/mp4",
            "https://another.com",
            &supported_urls(vec![("*", vec![r"https://example\.com"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_true_if_url_matches_under_specific_media_type() {
        let supported_urls = supported_urls(vec![
            ("text/plain", vec![r"https://text\.com"]),
            ("*", vec![r"https://any\.com"]),
        ]);

        assert!(is_url_supported(
            "text/plain",
            "https://text.com",
            &supported_urls
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_true_if_wildcard_matches_even_if_specific_exists() {
        let supported_urls = supported_urls(vec![
            ("text/plain", vec![r"https://text\.com"]),
            ("*", vec![r"https://any\.com"]),
        ]);

        assert!(is_url_supported(
            "text/plain",
            "https://any.com",
            &supported_urls
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_true_if_wildcard_matches_non_specified_media_type() {
        let supported_urls = supported_urls(vec![
            ("text/plain", vec![r"https://text\.com"]),
            ("*", vec![r"https://any\.com"]),
        ]);

        assert!(is_url_supported(
            "image/png",
            "https://any.com",
            &supported_urls
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_false_if_url_matches_neither_specific_nor_wildcard() {
        let supported_urls = supported_urls(vec![
            ("text/plain", vec![r"https://text\.com"]),
            ("*", vec![r"https://any\.com"]),
        ]);

        assert!(!is_url_supported(
            "text/plain",
            "https://other.com",
            &supported_urls
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_false_if_non_specified_media_type_misses_wildcard() {
        let supported_urls = supported_urls(vec![
            ("text/plain", vec![r"https://text\.com"]),
            ("*", vec![r"https://any\.com"]),
        ]);

        assert!(!is_url_supported(
            "image/png",
            "https://other.com",
            &supported_urls
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_true_if_empty_url_matches_pattern() {
        assert!(is_url_supported(
            "text/plain",
            "",
            &supported_urls(vec![("text/plain", vec![r".*"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_false_if_empty_url_does_not_match_pattern() {
        assert!(!is_url_supported(
            "text/plain",
            "",
            &supported_urls(vec![("text/plain", vec![r"https://.+"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_is_case_insensitive_for_media_types() {
        assert!(is_url_supported(
            "TEXT/PLAIN",
            "https://example.com",
            &supported_urls(vec![("text/plain", vec![r"https://example\.com"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_handles_case_insensitive_regex_for_urls_if_specified() {
        assert!(is_url_supported(
            "text/plain",
            "https://EXAMPLE.com/path",
            &supported_urls(vec![("text/plain", vec![r"https://example\.com/path"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_is_case_insensitive_for_url_paths_by_default_regex() {
        assert!(is_url_supported(
            "text/plain",
            "https://example.com/PATH",
            &supported_urls(vec![("text/plain", vec![r"https://example\.com/path"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_true_for_wildcard_subtype_match() {
        assert!(is_url_supported(
            "image/png",
            "https://example.com",
            &supported_urls(vec![("image/*", vec![r"https://example\.com"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_uses_full_wildcard_if_subtype_wildcard_is_not_matched() {
        assert!(is_url_supported(
            "image/png",
            "https://any.com",
            &supported_urls(vec![
                ("image/*", vec![r"https://images\.com"]),
                ("*", vec![r"https://any\.com"]),
            ])
        ));
    }

    #[test]
    fn is_url_supported_upstream_matches_type_wildcard_key_for_top_level_only_media_type() {
        assert!(is_url_supported(
            "image",
            "https://example.com/cat.png",
            &supported_urls(vec![("image/*", vec![r"https://example\.com/.+"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_matches_wildcard_key_for_top_level_only_media_type() {
        assert!(is_url_supported(
            "image",
            "https://example.com",
            &supported_urls(vec![("*", vec![r"https://example\.com"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_does_not_match_specific_subtype_key_for_top_level_only_media_type()
    {
        assert!(!is_url_supported(
            "image",
            "https://example.com/cat.png",
            &supported_urls(vec![("image/png", vec![r"https://example\.com/.+"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_does_not_match_different_top_level_wildcard_key() {
        assert!(!is_url_supported(
            "image",
            "https://example.com/audio.mp3",
            &supported_urls(vec![("audio/*", vec![r"https://example\.com/.+"])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_false_if_specific_media_type_has_empty_url_array() {
        assert!(!is_url_supported(
            "text/plain",
            "https://example.com",
            &supported_urls(vec![("text/plain", vec![])])
        ));
    }

    #[test]
    fn is_url_supported_upstream_falls_back_to_wildcard_if_specific_media_type_empty_array() {
        assert!(is_url_supported(
            "text/plain",
            "https://any.com",
            &supported_urls(vec![
                ("text/plain", vec![]),
                ("*", vec![r"https://any\.com"]),
            ])
        ));
    }

    #[test]
    fn is_url_supported_upstream_returns_false_if_empty_specific_array_and_wildcard_mismatch() {
        assert!(!is_url_supported(
            "text/plain",
            "https://another.com",
            &supported_urls(vec![
                ("text/plain", vec![]),
                ("*", vec![r"https://any\.com"]),
            ])
        ));
    }

    fn upstream_response_body_chunks(body: Option<&[u8]>) -> Vec<Vec<u8>> {
        body.map(|body| body.chunks(4).map(<[u8]>::to_vec).collect())
            .unwrap_or_default()
    }

    fn assert_download_error_message_contains(error: &DownloadError, expected: &str) {
        assert!(
            error.message().contains(expected),
            "expected {message:?} to contain {expected:?}",
            message = error.message()
        );
    }

    #[test]
    fn read_response_with_size_limit_upstream_should_read_response_within_limit_successfully() {
        let data = vec![1, 2, 3, 4, 5, 6, 7, 8];

        let body = read_response_with_size_limit(
            "http://example.com/file",
            upstream_response_body_chunks(Some(&data)),
            Some("8"),
            Some(100),
        )
        .expect("body is within limit");

        assert_eq!(body, data);
    }

    #[test]
    fn read_response_with_size_limit_upstream_rejects_oversized_content_length_early() {
        let body = vec![0; 10];

        let error = read_response_with_size_limit(
            "http://example.com/large",
            upstream_response_body_chunks(Some(&body)),
            Some("1000"),
            Some(100),
        )
        .expect_err("content-length exceeds limit");

        assert_eq!(error.url(), "http://example.com/large");
        assert_download_error_message_contains(&error, "Content-Length: 1000");
    }

    #[test]
    fn read_response_with_size_limit_upstream_should_abort_when_streamed_bytes_exceed_limit() {
        let large_body = vec![42; 200];

        let error = read_response_with_size_limit(
            "http://example.com/streaming",
            upstream_response_body_chunks(Some(&large_body)),
            None,
            Some(50),
        )
        .expect_err("streamed bytes exceed limit");

        assert_download_error_message_contains(&error, "exceeded maximum size of 50 bytes");
    }

    #[test]
    fn read_response_with_size_limit_upstream_should_handle_lying_content_length() {
        let large_body = vec![42; 200];

        let error = read_response_with_size_limit(
            "http://example.com/liar",
            upstream_response_body_chunks(Some(&large_body)),
            Some("10"),
            Some(50),
        )
        .expect_err("actual body still exceeds limit");

        assert_download_error_message_contains(&error, "exceeded maximum size of 50 bytes");
    }

    #[test]
    fn read_response_with_size_limit_upstream_should_handle_empty_body_null() {
        let body = read_response_with_size_limit(
            "http://example.com/empty",
            upstream_response_body_chunks(None),
            None,
            Some(100),
        )
        .expect("null body reads as empty bytes");

        assert_eq!(body, Vec::<u8>::new());
    }

    #[test]
    fn read_response_with_size_limit_upstream_should_handle_empty_body_zero_length() {
        let data = Vec::<u8>::new();

        let body = read_response_with_size_limit(
            "http://example.com/empty",
            upstream_response_body_chunks(Some(&data)),
            None,
            Some(100),
        )
        .expect("zero-length body reads as empty bytes");

        assert_eq!(body, data);
    }

    #[test]
    fn read_response_with_size_limit_upstream_should_respect_custom_max_bytes() {
        let data = vec![1; 10];

        let body = read_response_with_size_limit(
            "http://example.com/custom",
            upstream_response_body_chunks(Some(&data)),
            Some("10"),
            Some(10),
        )
        .expect("body matches custom maxBytes");

        assert_eq!(body, data);
    }

    #[test]
    fn read_response_with_size_limit_upstream_should_reject_at_exact_boundary_max_bytes_plus_one() {
        let data = vec![1; 11];

        let error = read_response_with_size_limit(
            "http://example.com/boundary",
            upstream_response_body_chunks(Some(&data)),
            None,
            Some(10),
        )
        .expect_err("maxBytes plus one exceeds limit");

        assert_download_error_message_contains(&error, "exceeded maximum size of 10 bytes");
    }

    #[test]
    fn read_response_with_size_limit_reads_chunks_within_limit() {
        let chunks = [b"abcd".as_slice(), b"efgh".as_slice()];

        let body =
            read_response_with_size_limit("https://example.com/file", chunks, Some("8"), Some(100))
                .expect("body is within limit");

        assert_eq!(body, b"abcdefgh");
    }

    #[test]
    fn read_response_with_size_limit_rejects_large_content_length_early() {
        let error = read_response_with_size_limit(
            "https://example.com/large",
            [b"small".as_slice()],
            Some("1000 bytes"),
            Some(100),
        )
        .expect_err("content-length exceeds limit");

        assert_eq!(error.url(), "https://example.com/large");
        assert_eq!(
            error.message(),
            "Download of https://example.com/large exceeded maximum size of 100 bytes (Content-Length: 1000)."
        );
    }

    #[test]
    fn read_response_with_size_limit_rejects_streams_that_exceed_limit() {
        let chunks = [vec![1; 40], vec![2; 40]];

        let error =
            read_response_with_size_limit("https://example.com/stream", chunks, None, Some(50))
                .expect_err("streamed bytes exceed limit");

        assert_eq!(
            error.message(),
            "Download of https://example.com/stream exceeded maximum size of 50 bytes."
        );
    }

    #[test]
    fn read_response_with_size_limit_checks_larger_actual_body_even_when_length_claims_small() {
        let chunks = [vec![42; 60]];

        let error =
            read_response_with_size_limit("https://example.com/liar", chunks, Some("10"), Some(50))
                .expect_err("actual body still exceeds limit");

        assert_eq!(
            error.message(),
            "Download of https://example.com/liar exceeded maximum size of 50 bytes."
        );
    }

    #[test]
    fn read_response_with_size_limit_uses_upstream_default_limit_and_ignores_invalid_lengths() {
        assert_eq!(DEFAULT_MAX_DOWNLOAD_SIZE, 2 * 1024 * 1024 * 1024);

        let body = read_response_with_size_limit(
            "https://example.com/empty",
            [b"ok".as_slice()],
            Some("not-a-number"),
            None,
        )
        .expect("invalid content-length is ignored");

        assert_eq!(body, b"ok");
    }

    fn media_bytes(bytes: &[u8]) -> FileDataContent {
        FileDataContent::Bytes(bytes.to_vec())
    }

    fn media_base64(base64: &str) -> FileDataContent {
        FileDataContent::Base64(base64.to_string())
    }

    fn upstream_webp_bytes() -> Vec<u8> {
        vec![
            0x52, 0x49, 0x46, 0x46, 0x24, 0x00, 0x00, 0x00, 0x57, 0x45, 0x42, 0x50, 0x56, 0x50,
            0x38, 0x20,
        ]
    }

    fn upstream_wav_bytes() -> Vec<u8> {
        vec![
            0x52, 0x49, 0x46, 0x46, 0x24, 0x00, 0x00, 0x00, 0x57, 0x41, 0x56, 0x45, 0x66, 0x6d,
            0x74, 0x20,
        ]
    }

    fn upstream_mp3_with_id3_bytes() -> Vec<u8> {
        vec![
            0x49, 0x44, 0x33, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xfb, 0x00, 0x00,
        ]
    }

    #[test]
    fn detect_media_type_matches_top_level_signature_tables() {
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x89, 0x50, 0x4e, 0x47, 0xff]),
                Some("image"),
            ),
            Some("image/png")
        );
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x25, 0x50, 0x44, 0x46, 0x00]),
                Some("application"),
            ),
            Some("application/pdf")
        );
        assert_eq!(
            detect_media_type(&FileDataContent::Bytes(vec![0xff, 0xfb]), Some("audio")),
            Some("audio/mpeg")
        );
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x1a, 0x45, 0xdf, 0xa3]),
                Some("video"),
            ),
            Some("video/webm")
        );
    }

    #[test]
    fn detect_media_type_handles_base64_and_id3_prefixed_mp3() {
        assert_eq!(
            detect_media_type(
                &FileDataContent::Base64("iVBORw0KGgo=".to_string()),
                Some("image"),
            ),
            Some("image/png")
        );
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![
                    0x49, 0x44, 0x33, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xfb,
                ]),
                Some("audio"),
            ),
            Some("audio/mpeg")
        );
    }

    #[test]
    fn detect_media_type_returns_none_for_unsupported_or_unmatched_data() {
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x89, 0x50, 0x4e, 0x47, 0xff]),
                Some("text"),
            ),
            None
        );
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x00, 0x01, 0x02]),
                Some("image"),
            ),
            None
        );
        assert_eq!(
            detect_media_type(
                &FileDataContent::Base64("not valid base64!".to_string()),
                None,
            ),
            None
        );
    }

    #[test]
    fn detect_media_type_without_top_level_type_uses_upstream_order() {
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(vec![0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70]),
                None,
            ),
            Some("video/mp4")
        );
        assert_eq!(
            detect_media_type(&FileDataContent::Bytes(vec![0x1a, 0x45, 0xdf, 0xa3]), None,),
            Some("audio/webm")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_gif_from_bytes() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x47, 0x49, 0x46, 0xff, 0xff]), Some("image")),
            Some("image/gif")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_gif_from_base64() {
        assert_eq!(
            detect_media_type(&media_base64("R0lGabc123"), Some("image")),
            Some("image/gif")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_png_from_bytes() {
        assert_eq!(
            detect_media_type(
                &media_bytes(&[0x89, 0x50, 0x4e, 0x47, 0xff, 0xff]),
                Some("image"),
            ),
            Some("image/png")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_png_from_base64() {
        assert_eq!(
            detect_media_type(&media_base64("iVBORwabc123"), Some("image")),
            Some("image/png")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_jpeg_from_bytes() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0xff, 0xd8, 0xff, 0xff]), Some("image")),
            Some("image/jpeg")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_jpeg_from_base64() {
        assert_eq!(
            detect_media_type(&media_base64("/9j/abc123"), Some("image")),
            Some("image/jpeg")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_webp_from_bytes() {
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(upstream_webp_bytes()),
                Some("image")
            ),
            Some("image/webp")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_webp_from_base64() {
        let webp_base64 = convert_bytes_to_base64(&upstream_webp_bytes());

        assert_eq!(
            detect_media_type(&media_base64(&webp_base64), Some("image")),
            Some("image/webp")
        );
    }

    #[test]
    fn detect_media_type_upstream_does_not_detect_riff_audio_as_webp_from_bytes() {
        assert_eq!(
            detect_media_type(&FileDataContent::Bytes(upstream_wav_bytes()), Some("image")),
            None
        );
    }

    #[test]
    fn detect_media_type_upstream_does_not_detect_riff_audio_as_webp_from_base64() {
        let wav_base64 = convert_bytes_to_base64(&upstream_wav_bytes());

        assert_eq!(
            detect_media_type(&media_base64(&wav_base64), Some("image")),
            None
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_bmp_from_bytes() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x42, 0x4d, 0xff, 0xff]), Some("image")),
            Some("image/bmp")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_bmp_from_base64() {
        let bmp_base64 = convert_bytes_to_base64(&[0x42, 0x4d, 0xff, 0xff]);

        assert_eq!(
            detect_media_type(&media_base64(&bmp_base64), Some("image")),
            Some("image/bmp")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_tiff_little_endian_from_bytes() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x49, 0x49, 0x2a, 0x00, 0xff]), Some("image"),),
            Some("image/tiff")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_tiff_little_endian_from_base64() {
        assert_eq!(
            detect_media_type(&media_base64("SUkqAAabc123"), Some("image")),
            Some("image/tiff")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_tiff_big_endian_from_bytes() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x4d, 0x4d, 0x00, 0x2a, 0xff]), Some("image"),),
            Some("image/tiff")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_tiff_big_endian_from_base64() {
        assert_eq!(
            detect_media_type(&media_base64("TU0AKgabc123"), Some("image")),
            Some("image/tiff")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_avif_from_bytes() {
        assert_eq!(
            detect_media_type(
                &media_bytes(&[
                    0x00, 0x00, 0x00, 0x20, 0x66, 0x74, 0x79, 0x70, 0x61, 0x76, 0x69, 0x66, 0xff,
                ]),
                Some("image"),
            ),
            Some("image/avif")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_avif_from_base64() {
        assert_eq!(
            detect_media_type(&media_base64("AAAAIGZ0eXBhdmlmabc123"), Some("image")),
            Some("image/avif")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_heic_from_bytes() {
        assert_eq!(
            detect_media_type(
                &media_bytes(&[
                    0x00, 0x00, 0x00, 0x20, 0x66, 0x74, 0x79, 0x70, 0x68, 0x65, 0x69, 0x63, 0xff,
                ]),
                Some("image"),
            ),
            Some("image/heic")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_heic_from_base64() {
        assert_eq!(
            detect_media_type(&media_base64("AAAAIGZ0eXBoZWljabc123"), Some("image")),
            Some("image/heic")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_mp3_from_bytes() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0xff, 0xfb]), Some("audio")),
            Some("audio/mpeg")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_mp3_from_base64() {
        assert_eq!(
            detect_media_type(&media_base64("//s="), Some("audio")),
            Some("audio/mpeg")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_mp3_with_id3_tags_from_bytes() {
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(upstream_mp3_with_id3_bytes()),
                Some("audio"),
            ),
            Some("audio/mpeg")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_mp3_with_id3_tags_from_base64() {
        let mp3_base64 = convert_bytes_to_base64(&upstream_mp3_with_id3_bytes());

        assert_eq!(
            detect_media_type(&media_base64(&mp3_base64), Some("audio")),
            Some("audio/mpeg")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_wav_from_bytes() {
        assert_eq!(
            detect_media_type(&FileDataContent::Bytes(upstream_wav_bytes()), Some("audio")),
            Some("audio/wav")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_wav_from_base64() {
        let wav_base64 = convert_bytes_to_base64(&upstream_wav_bytes());

        assert_eq!(
            detect_media_type(&media_base64(&wav_base64), Some("audio")),
            Some("audio/wav")
        );
    }

    #[test]
    fn detect_media_type_upstream_does_not_detect_webp_as_wav_from_bytes() {
        assert_eq!(
            detect_media_type(
                &FileDataContent::Bytes(upstream_webp_bytes()),
                Some("audio")
            ),
            None
        );
    }

    #[test]
    fn detect_media_type_upstream_does_not_detect_webp_as_wav_from_base64() {
        let webp_base64 = convert_bytes_to_base64(&upstream_webp_bytes());

        assert_eq!(
            detect_media_type(&media_base64(&webp_base64), Some("audio")),
            None
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_ogg_from_bytes() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x4f, 0x67, 0x67, 0x53]), Some("audio")),
            Some("audio/ogg")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_ogg_from_base64() {
        assert_eq!(
            detect_media_type(&media_base64("T2dnUw"), Some("audio")),
            Some("audio/ogg")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_flac_from_bytes() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x66, 0x4c, 0x61, 0x43]), Some("audio")),
            Some("audio/flac")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_flac_from_base64() {
        assert_eq!(
            detect_media_type(&media_base64("ZkxhQw"), Some("audio")),
            Some("audio/flac")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_aac_from_bytes() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x40, 0x15, 0x00, 0x00]), Some("audio")),
            Some("audio/aac")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_aac_from_base64() {
        let aac_base64 = convert_bytes_to_base64(&[0x40, 0x15, 0x00, 0x00]);

        assert_eq!(
            detect_media_type(&media_base64(&aac_base64), Some("audio")),
            Some("audio/aac")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_mp4_from_bytes() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x66, 0x74, 0x79, 0x70]), Some("audio")),
            Some("audio/mp4")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_mp4_from_base64() {
        assert_eq!(
            detect_media_type(&media_base64("ZnR5cA"), Some("audio")),
            Some("audio/mp4")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_webm_from_bytes() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x1a, 0x45, 0xdf, 0xa3]), Some("audio")),
            Some("audio/webm")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_webm_from_base64() {
        assert_eq!(
            detect_media_type(&media_base64("GkXfow=="), Some("audio")),
            Some("audio/webm")
        );
    }

    #[test]
    fn detect_media_type_upstream_returns_undefined_for_unknown_image_formats() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x00, 0x01, 0x02, 0x03]), Some("image")),
            None
        );
    }

    #[test]
    fn detect_media_type_upstream_returns_undefined_for_unknown_audio_formats() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x00, 0x01, 0x02, 0x03]), Some("audio")),
            None
        );
    }

    #[test]
    fn detect_media_type_upstream_returns_undefined_for_empty_arrays_for_image() {
        assert_eq!(detect_media_type(&media_bytes(&[]), Some("image")), None);
    }

    #[test]
    fn detect_media_type_upstream_returns_undefined_for_empty_arrays_for_audio() {
        assert_eq!(detect_media_type(&media_bytes(&[]), Some("audio")), None);
    }

    #[test]
    fn detect_media_type_upstream_returns_undefined_for_short_arrays_for_image() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x89, 0x50]), Some("image")),
            None
        );
    }

    #[test]
    fn detect_media_type_upstream_returns_undefined_for_short_arrays_for_audio() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x4f, 0x67]), Some("audio")),
            None
        );
    }

    #[test]
    fn detect_media_type_upstream_returns_undefined_for_invalid_base64_strings_for_image() {
        assert_eq!(
            detect_media_type(&media_base64("invalid123"), Some("image")),
            None
        );
    }

    #[test]
    fn detect_media_type_upstream_returns_undefined_for_invalid_base64_strings_for_audio() {
        assert_eq!(
            detect_media_type(&media_base64("invalid123"), Some("audio")),
            None
        );
    }

    #[test]
    fn get_top_level_media_type_upstream_returns_top_level_segment_for_full_media_type() {
        assert_eq!(get_top_level_media_type("image/png"), "image");
        assert_eq!(get_top_level_media_type("audio/mpeg"), "audio");
        assert_eq!(get_top_level_media_type("video/mp4"), "video");
        assert_eq!(get_top_level_media_type("application/pdf"), "application");
        assert_eq!(get_top_level_media_type("text/plain"), "text");
    }

    #[test]
    fn get_top_level_media_type_upstream_returns_input_for_top_level_segment() {
        assert_eq!(get_top_level_media_type("image"), "image");
        assert_eq!(get_top_level_media_type("audio"), "audio");
        assert_eq!(get_top_level_media_type("video"), "video");
        assert_eq!(get_top_level_media_type("application"), "application");
        assert_eq!(get_top_level_media_type("text"), "text");
    }

    #[test]
    fn get_top_level_media_type_upstream_normalizes_wildcards_to_top_level_segment() {
        assert_eq!(get_top_level_media_type("image/*"), "image");
        assert_eq!(get_top_level_media_type("audio/*"), "audio");
        assert_eq!(get_top_level_media_type("video/*"), "video");
        assert_eq!(get_top_level_media_type("application/*"), "application");
        assert_eq!(get_top_level_media_type("text/*"), "text");
    }

    #[test]
    fn get_top_level_media_type_upstream_handles_edge_cases() {
        assert_eq!(get_top_level_media_type(""), "");
        assert_eq!(get_top_level_media_type("/"), "");
        assert_eq!(get_top_level_media_type("image/"), "image");
    }

    #[test]
    fn is_full_media_type_upstream_returns_true_for_concrete_subtype() {
        assert!(is_full_media_type("image/png"));
        assert!(is_full_media_type("audio/mpeg"));
        assert!(is_full_media_type("video/mp4"));
        assert!(is_full_media_type("application/pdf"));
        assert!(is_full_media_type("text/plain"));
    }

    #[test]
    fn is_full_media_type_upstream_returns_false_for_top_level_only_media_types() {
        assert!(!is_full_media_type("image"));
        assert!(!is_full_media_type("audio"));
        assert!(!is_full_media_type("video"));
        assert!(!is_full_media_type("application"));
        assert!(!is_full_media_type("text"));
    }

    #[test]
    fn is_full_media_type_upstream_returns_false_for_wildcards() {
        assert!(!is_full_media_type("image/*"));
        assert!(!is_full_media_type("audio/*"));
        assert!(!is_full_media_type("video/*"));
        assert!(!is_full_media_type("application/*"));
        assert!(!is_full_media_type("text/*"));
    }

    #[test]
    fn is_full_media_type_upstream_returns_false_for_edge_cases() {
        assert!(!is_full_media_type(""));
        assert!(!is_full_media_type("/"));
        assert!(!is_full_media_type("image/"));
    }

    #[test]
    fn detect_media_type_upstream_detects_image_types_when_top_level_type_is_image() {
        assert_eq!(
            detect_media_type(
                &media_bytes(&[0x89, 0x50, 0x4e, 0x47, 0xff, 0xff]),
                Some("image"),
            ),
            Some("image/png")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_audio_types_when_top_level_type_is_audio() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0xff, 0xfb]), Some("audio")),
            Some("audio/mpeg")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_video_types_when_top_level_type_is_video() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x1a, 0x45, 0xdf, 0xa3]), Some("video")),
            Some("video/webm")
        );
    }

    #[test]
    fn detect_media_type_upstream_detects_document_types_when_top_level_type_is_application() {
        assert_eq!(
            detect_media_type(
                &media_bytes(&[0x25, 0x50, 0x44, 0x46, 0x00]),
                Some("application"),
            ),
            Some("application/pdf")
        );
    }

    #[test]
    fn detect_media_type_upstream_returns_undefined_for_text_top_level_segment() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x48, 0x65, 0x6c, 0x6c, 0x6f]), Some("text"),),
            None
        );
    }

    #[test]
    fn detect_media_type_upstream_returns_undefined_for_unknown_top_level_segments() {
        assert_eq!(
            detect_media_type(
                &media_bytes(&[0x89, 0x50, 0x4e, 0x47, 0xff, 0xff]),
                Some("not-a-real-segment"),
            ),
            None
        );
    }

    #[test]
    fn detect_media_type_upstream_returns_undefined_when_segment_table_does_not_match() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x00, 0x01, 0x02, 0x03]), Some("image")),
            None
        );
    }

    #[test]
    fn detect_media_type_upstream_without_top_level_type_detects_image_types() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x89, 0x50, 0x4e, 0x47, 0xff, 0xff]), None,),
            Some("image/png")
        );
    }

    #[test]
    fn detect_media_type_upstream_without_top_level_type_detects_audio_types() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0xff, 0xfb]), None),
            Some("audio/mpeg")
        );
    }

    #[test]
    fn detect_media_type_upstream_without_top_level_type_detects_video_types() {
        assert_eq!(
            detect_media_type(
                &media_bytes(&[0x00, 0x00, 0x00, 0x18, 0x66, 0x74, 0x79, 0x70]),
                None,
            ),
            Some("video/mp4")
        );
    }

    #[test]
    fn detect_media_type_upstream_without_top_level_type_detects_document_types() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x25, 0x50, 0x44, 0x46, 0x00]), None),
            Some("application/pdf")
        );
    }

    #[test]
    fn detect_media_type_upstream_without_top_level_type_returns_undefined_for_no_signature() {
        assert_eq!(
            detect_media_type(&media_bytes(&[0x00, 0x01, 0x02, 0x03]), None),
            None
        );
    }

    #[test]
    fn resolve_full_media_type_returns_full_media_type_as_is() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Bytes(vec![0x89, 0x50, 0x4e, 0x47]),
            },
            "image/jpeg",
        );

        assert_eq!(
            resolve_full_media_type(&part).expect("full media type resolves"),
            "image/jpeg"
        );
    }

    #[test]
    fn resolve_full_media_type_detects_inline_byte_subtype() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Bytes(vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a]),
            },
            "image",
        );

        assert_eq!(
            resolve_full_media_type(&part).expect("inline bytes resolve"),
            "image/png"
        );
    }

    #[test]
    fn resolve_full_media_type_treats_wildcard_as_top_level() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Base64("iVBORw0KGgo=".to_string()),
            },
            "image/*",
        );

        assert_eq!(
            resolve_full_media_type(&part).expect("wildcard media type resolves"),
            "image/png"
        );
    }

    #[test]
    fn resolve_full_media_type_detects_application_pdf() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Bytes(vec![0x25, 0x50, 0x44, 0x46, 0x2d]),
            },
            "application",
        );

        assert_eq!(
            resolve_full_media_type(&part).expect("application subtype resolves"),
            "application/pdf"
        );
    }

    #[test]
    fn resolve_full_media_type_rejects_non_inline_byte_data() {
        let part = LanguageModelFilePart::new(
            FileData::Url {
                url: Url::parse("https://example.com/file.png").expect("valid URL"),
            },
            "image",
        );

        let error = resolve_full_media_type(&part)
            .expect_err("top-level URL media type requires a subtype");

        assert_eq!(
            error.functionality(),
            "file of media type \"image\" must specify subtype since it is not passed as inline bytes"
        );
    }

    #[test]
    fn resolve_full_media_type_rejects_unrecognized_inline_bytes() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Bytes(vec![0x00, 0x01, 0x02]),
            },
            "image",
        );

        let error = resolve_full_media_type(&part)
            .expect_err("unrecognized inline bytes require a subtype");

        assert_eq!(
            error.functionality(),
            "file of media type \"image\" must specify subtype since it could not be auto-detected"
        );
    }

    #[test]
    fn resolve_full_media_type_rejects_unsupported_top_level_segment() {
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Base64("hello".to_string()),
            },
            "text",
        );

        let error = resolve_full_media_type(&part)
            .expect_err("unsupported top-level segment requires a subtype");

        assert_eq!(
            error.functionality(),
            "file of media type \"text\" must specify subtype since it could not be auto-detected"
        );
    }

    #[test]
    fn resolve_full_media_type_accepts_base64_string_data() {
        let png_base64 = convert_bytes_to_base64(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a]);
        let part = LanguageModelFilePart::new(
            FileData::Data {
                data: FileDataContent::Base64(png_base64),
            },
            "image",
        );

        assert_eq!(
            resolve_full_media_type(&part).expect("base64 data resolves"),
            "image/png"
        );
    }

    #[test]
    fn convert_image_model_file_to_data_uri_returns_url_as_is() {
        let file = ImageModelFile::url(
            Url::parse("https://example.com/image.png?width=100&height=200").expect("valid URL"),
        );

        assert_eq!(
            convert_image_model_file_to_data_uri(&file),
            "https://example.com/image.png?width=100&height=200"
        );
    }

    #[test]
    fn convert_image_model_file_to_data_uri_upstream_returns_url_as_is_for_url_files() {
        let file =
            ImageModelFile::url(Url::parse("https://example.com/image.png").expect("valid URL"));

        assert_eq!(
            convert_image_model_file_to_data_uri(&file),
            "https://example.com/image.png"
        );
    }

    #[test]
    fn convert_image_model_file_to_data_uri_upstream_handles_urls_with_query_parameters() {
        let file = ImageModelFile::url(
            Url::parse("https://example.com/image.png?width=100&height=200").expect("valid URL"),
        );

        assert_eq!(
            convert_image_model_file_to_data_uri(&file),
            "https://example.com/image.png?width=100&height=200"
        );
    }

    #[test]
    fn convert_image_model_file_to_data_uri_embeds_base64_data() {
        let file = ImageModelFile::file(
            "image/png",
            FileDataContent::Base64("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJ".to_string()),
        );

        assert_eq!(
            convert_image_model_file_to_data_uri(&file),
            "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJ"
        );
    }

    #[test]
    fn convert_image_model_file_to_data_uri_upstream_returns_data_uri_for_base64_string_data() {
        let data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let file = ImageModelFile::file("image/png", FileDataContent::Base64(data.to_string()));

        assert_eq!(
            convert_image_model_file_to_data_uri(&file),
            format!("data:image/png;base64,{data}")
        );
    }

    #[test]
    fn convert_image_model_file_to_data_uri_upstream_handles_different_media_types() {
        let file = ImageModelFile::file(
            "image/jpeg",
            FileDataContent::Base64("base64data".to_string()),
        );

        assert_eq!(
            convert_image_model_file_to_data_uri(&file),
            "data:image/jpeg;base64,base64data"
        );
    }

    #[test]
    fn convert_image_model_file_to_data_uri_encodes_raw_bytes() {
        let file = ImageModelFile::file("image/webp", FileDataContent::Bytes(b"Hello".to_vec()));

        assert_eq!(
            convert_image_model_file_to_data_uri(&file),
            "data:image/webp;base64,SGVsbG8="
        );
    }

    #[test]
    fn convert_image_model_file_to_data_uri_upstream_converts_uint8_array_to_base64_data_uri() {
        let file = ImageModelFile::file("image/png", FileDataContent::Bytes(b"Hello".to_vec()));

        assert_eq!(
            convert_image_model_file_to_data_uri(&file),
            "data:image/png;base64,SGVsbG8="
        );
    }

    #[test]
    fn convert_image_model_file_to_data_uri_handles_empty_raw_bytes() {
        let file = ImageModelFile::file("image/png", FileDataContent::Bytes(Vec::new()));

        assert_eq!(
            convert_image_model_file_to_data_uri(&file),
            "data:image/png;base64,"
        );
    }

    #[test]
    fn convert_image_model_file_to_data_uri_upstream_handles_empty_uint8_array() {
        let file = ImageModelFile::file("image/png", FileDataContent::Bytes(Vec::new()));

        assert_eq!(
            convert_image_model_file_to_data_uri(&file),
            "data:image/png;base64,"
        );
    }

    #[test]
    fn convert_image_model_file_to_data_uri_upstream_handles_different_media_types_with_uint8_array()
     {
        let file = ImageModelFile::file("image/webp", FileDataContent::Bytes(b"Hello".to_vec()));

        assert_eq!(
            convert_image_model_file_to_data_uri(&file),
            "data:image/webp;base64,SGVsbG8="
        );
    }

    #[test]
    fn download_error_retains_status_and_cause_messages() {
        let status_error =
            DownloadError::with_status("https://example.com/missing.png", 404, "Not Found");
        assert_eq!(status_error.url(), "https://example.com/missing.png");
        assert_eq!(status_error.status_code(), Some(404));
        assert_eq!(status_error.status_text(), Some("Not Found"));
        assert_eq!(
            status_error.message(),
            "Failed to download https://example.com/missing.png: 404 Not Found"
        );
        assert_eq!(status_error.to_string(), status_error.message());

        let cause_error =
            DownloadError::with_cause_message("https://example.com/file", "connection refused");
        assert_eq!(
            cause_error.message(),
            "Failed to download https://example.com/file: connection refused"
        );
        assert_eq!(cause_error.status_code(), None);
        assert_eq!(cause_error.status_text(), None);
    }

    fn expect_download_url_allowed(url: &str) {
        assert!(
            validate_download_url(url).is_ok(),
            "{url} should be allowed"
        );
    }

    fn expect_download_url_rejected(url: &str) -> DownloadError {
        validate_download_url(url).expect_err("download URL should be rejected")
    }

    #[test]
    fn validate_download_url_upstream_should_allow_https_urls() {
        expect_download_url_allowed("https://example.com/image.png");
    }

    #[test]
    fn validate_download_url_upstream_should_allow_http_urls() {
        expect_download_url_allowed("http://example.com/image.png");
    }

    #[test]
    fn validate_download_url_upstream_should_allow_public_ip_addresses() {
        expect_download_url_allowed("https://203.0.113.1/file");
    }

    #[test]
    fn validate_download_url_upstream_should_allow_urls_with_ports() {
        expect_download_url_allowed("https://example.com:8080/file");
    }

    #[test]
    fn validate_download_url_upstream_should_allow_data_urls() {
        expect_download_url_allowed("data:text/plain;base64,aGVsbG8=");
    }

    #[test]
    fn validate_download_url_upstream_should_block_file_urls() {
        assert_eq!(
            expect_download_url_rejected("file:///etc/passwd").message(),
            "URL scheme must be http, https, or data, got file:"
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_ftp_urls() {
        assert_eq!(
            expect_download_url_rejected("ftp://example.com/file").message(),
            "URL scheme must be http, https, or data, got ftp:"
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_javascript_urls() {
        assert_eq!(
            expect_download_url_rejected("javascript:alert(1)").message(),
            "URL scheme must be http, https, or data, got javascript:"
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_invalid_urls() {
        assert_eq!(
            expect_download_url_rejected("not-a-url").message(),
            "Invalid URL: not-a-url"
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_localhost() {
        assert!(
            expect_download_url_rejected("http://localhost/file")
                .message()
                .contains("is not allowed")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_localhost_with_port() {
        assert!(
            expect_download_url_rejected("http://localhost:3000/file")
                .message()
                .contains("is not allowed")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_local_domains() {
        assert!(
            expect_download_url_rejected("http://myhost.local/file")
                .message()
                .contains("is not allowed")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_localhost_domains() {
        assert!(
            expect_download_url_rejected("http://app.localhost/file")
                .message()
                .contains("is not allowed")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_127_0_0_1_loopback() {
        assert!(
            expect_download_url_rejected("http://127.0.0.1/file")
                .message()
                .contains("IP address")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_127_range() {
        assert!(
            expect_download_url_rejected("http://127.255.0.1/file")
                .message()
                .contains("IP address")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_10_range_private() {
        assert!(
            expect_download_url_rejected("http://10.0.0.1/file")
                .message()
                .contains("IP address")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_172_16_to_172_31_private() {
        assert!(
            expect_download_url_rejected("http://172.16.0.1/file")
                .message()
                .contains("IP address")
        );
        assert!(
            expect_download_url_rejected("http://172.31.255.255/file")
                .message()
                .contains("IP address")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_allow_172_15_and_172_32_public() {
        expect_download_url_allowed("http://172.15.0.1/file");
        expect_download_url_allowed("http://172.32.0.1/file");
    }

    #[test]
    fn validate_download_url_upstream_should_block_192_168_range_private() {
        assert!(
            expect_download_url_rejected("http://192.168.1.1/file")
                .message()
                .contains("IP address")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_169_254_link_local_cloud_metadata() {
        assert!(
            expect_download_url_rejected("http://169.254.169.254/latest/meta-data/")
                .message()
                .contains("IP address")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_0_0_0_0() {
        assert!(
            expect_download_url_rejected("http://0.0.0.0/file")
                .message()
                .contains("IP address")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_ipv6_loopback() {
        assert!(
            expect_download_url_rejected("http://[::1]/file")
                .message()
                .contains("IPv6 address")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_ipv6_unspecified() {
        assert!(
            expect_download_url_rejected("http://[::]/file")
                .message()
                .contains("IPv6 address")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_fc00_7_unique_local() {
        assert!(
            expect_download_url_rejected("http://[fc00::1]/file")
                .message()
                .contains("IPv6 address")
        );
        assert!(
            expect_download_url_rejected("http://[fd12::1]/file")
                .message()
                .contains("IPv6 address")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_fe80_10_link_local() {
        assert!(
            expect_download_url_rejected("http://[fe80::1]/file")
                .message()
                .contains("IPv6 address")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_ipv4_mapped_127_0_0_1() {
        assert!(
            expect_download_url_rejected("http://[::ffff:127.0.0.1]/file")
                .message()
                .contains("IPv6 address")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_ipv4_mapped_10_0_0_1() {
        assert!(
            expect_download_url_rejected("http://[::ffff:10.0.0.1]/file")
                .message()
                .contains("IPv6 address")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_block_ipv4_mapped_169_254_169_254() {
        assert!(
            expect_download_url_rejected("http://[::ffff:169.254.169.254]/file")
                .message()
                .contains("IPv6 address")
        );
    }

    #[test]
    fn validate_download_url_upstream_should_allow_ipv4_mapped_public_ip() {
        expect_download_url_allowed("http://[::ffff:203.0.113.1]/file");
    }

    #[test]
    fn extract_response_headers_preserves_response_header_entries() {
        let headers = extract_response_headers([
            ("content-type", "application/json"),
            ("x-request-id", "req_123"),
        ]);

        assert_eq!(
            headers,
            BTreeMap::from([
                ("content-type".to_string(), "application/json".to_string()),
                ("x-request-id".to_string(), "req_123".to_string()),
            ])
        );
    }

    #[test]
    fn extract_response_headers_lets_later_entries_override_duplicates() {
        let headers = extract_response_headers([
            ("x-provider", "first"),
            ("x-provider", "second"),
            ("x-empty", ""),
        ]);

        assert_eq!(
            headers,
            BTreeMap::from([
                ("x-empty".to_string(), "".to_string()),
                ("x-provider".to_string(), "second".to_string()),
            ])
        );
    }

    #[test]
    fn response_handler_result_serializes_optional_metadata() {
        let value = json!({ "name": "John" });
        let raw_value = json!({ "name": "John", "extraField": "ignored" });
        let result = ResponseHandlerResult::new(value.clone())
            .with_raw_value(raw_value.clone())
            .with_response_headers(BTreeMap::from([(
                "x-request-id".to_string(),
                "req_123".to_string(),
            )]));

        let serialized = serde_json::to_value(&result).expect("result serializes");

        assert_eq!(
            serialized,
            json!({
                "value": value,
                "rawValue": raw_value,
                "responseHeaders": {
                    "x-request-id": "req_123"
                }
            })
        );
    }

    #[test]
    fn response_handler_result_deserializes_minimal_result() {
        let result: ResponseHandlerResult<JsonValue> =
            serde_json::from_value(json!({ "value": "ok" })).expect("result deserializes");

        assert_eq!(result.value(), &json!("ok"));
        assert_eq!(result.raw_value(), None);
        assert_eq!(result.response_headers(), None);
    }

    #[test]
    fn event_source_response_handler_options_use_camel_case_json() {
        let options = EventSourceResponseHandlerOptions::new(
            b"data: {\"name\":\"John\",\"age\":30}\n\n".to_vec(),
        )
        .with_response_headers(BTreeMap::from([(
            "content-type".to_string(),
            "text/event-stream".to_string(),
        )]));

        let serialized = serde_json::to_value(&options).expect("options serialize");

        assert_eq!(
            serialized,
            json!({
                "responseHeaders": {
                    "content-type": "text/event-stream"
                },
                "responseBody": [
                    100, 97, 116, 97, 58, 32, 123, 34, 110, 97, 109, 101, 34, 58,
                    34, 74, 111, 104, 110, 34, 44, 34, 97, 103, 101, 34, 58,
                    51, 48, 125, 10, 10
                ]
            })
        );

        let deserialized: EventSourceResponseHandlerOptions =
            serde_json::from_value(serialized).expect("options deserialize");

        assert_eq!(deserialized, options);
    }

    #[test]
    fn event_source_response_handler_options_deserialize_missing_body() {
        let options: EventSourceResponseHandlerOptions = serde_json::from_value(json!({
            "responseHeaders": {}
        }))
        .expect("options deserialize");

        assert_eq!(options.response_body, None);
        assert_eq!(options.response_headers, BTreeMap::new());
    }

    #[test]
    fn parse_json_event_stream_parses_data_events_and_ignores_done() {
        let events = parse_json_event_stream(
            [
                b": keepalive\r\n".as_slice(),
                b"event: message\r\ndata: {\"name\":\r\n".as_slice(),
                b"data: \"John\",\"age\":30}\r\n\r\n".as_slice(),
                b"data: [DONE]\n\n".as_slice(),
            ],
            validate_person,
        );

        assert_eq!(
            events,
            vec![ParseJsonResult::success(
                Person {
                    name: "John".to_string(),
                    age: 30,
                },
                json!({ "name": "John", "age": 30 })
            )]
        );
    }

    #[test]
    fn parse_json_event_stream_preserves_parse_and_validation_failures() {
        let events = parse_json_event_stream(
            [
                b"data: {not json}\n\n".as_slice(),
                b"data: {\"name\":\"John\"}\n\n".as_slice(),
            ],
            validate_person,
        );

        assert_eq!(events.len(), 2);

        let parse_error = events[0].error().expect("parse error is returned");
        assert!(parse_error.as_json_parse_error().is_some());
        assert_eq!(events[0].raw_value(), None);

        let validation_error = events[1].error().expect("validation error is returned");
        assert!(validation_error.as_type_validation_error().is_some());
        assert_eq!(events[1].raw_value(), Some(&json!({ "name": "John" })));
    }

    #[test]
    fn create_event_source_response_handler_returns_results_and_headers() {
        let response_headers =
            BTreeMap::from([("content-type".to_string(), "text/event-stream".to_string())]);
        let options = EventSourceResponseHandlerOptions::new(
            b"data: {\"name\":\"John\",\"age\":30}\n\n".to_vec(),
        )
        .with_response_headers(response_headers.clone());

        let result = create_event_source_response_handler(options, validate_person)
            .expect("event source response is handled");

        assert_eq!(result.response_headers(), Some(&response_headers));
        assert_eq!(
            result.value(),
            &vec![ParseJsonResult::success(
                Person {
                    name: "John".to_string(),
                    age: 30,
                },
                json!({ "name": "John", "age": 30 })
            )]
        );
        assert_eq!(result.raw_value(), None);
    }

    #[test]
    fn create_event_source_response_handler_returns_empty_body_error_for_missing_body() {
        let error = create_event_source_response_handler(
            EventSourceResponseHandlerOptions::empty(),
            validate_person,
        )
        .expect_err("missing body is rejected");

        assert_eq!(error.message(), "Empty response body");
    }

    #[test]
    fn binary_response_handler_options_use_camel_case_json() {
        let options = BinaryResponseHandlerOptions::new(
            "https://api.example.com/files",
            json!({ "file": "test" }),
            200,
            vec![1, 2, 3, 4],
        )
        .with_response_headers(BTreeMap::from([(
            "content-type".to_string(),
            "application/octet-stream".to_string(),
        )]));

        let serialized = serde_json::to_value(&options).expect("options serialize");

        assert_eq!(
            serialized,
            json!({
                "url": "https://api.example.com/files",
                "requestBodyValues": { "file": "test" },
                "statusCode": 200,
                "responseHeaders": {
                    "content-type": "application/octet-stream"
                },
                "responseBody": [1, 2, 3, 4]
            })
        );

        let deserialized: BinaryResponseHandlerOptions =
            serde_json::from_value(serialized).expect("options deserialize");

        assert_eq!(deserialized, options);
    }

    #[test]
    fn binary_response_handler_options_deserialize_missing_body() {
        let options: BinaryResponseHandlerOptions = serde_json::from_value(json!({
            "url": "https://api.example.com/files",
            "requestBodyValues": {},
            "statusCode": 204,
            "responseHeaders": {}
        }))
        .expect("options deserialize");

        assert_eq!(options.response_body, None);
        assert_eq!(options.response_headers, BTreeMap::new());
    }

    #[test]
    fn create_binary_response_handler_returns_bytes_and_headers() {
        let response_headers = BTreeMap::from([(
            "content-type".to_string(),
            "application/octet-stream".to_string(),
        )]);
        let options = BinaryResponseHandlerOptions::new(
            "https://api.example.com/files",
            json!({ "file": "test" }),
            200,
            vec![1, 2, 3, 4],
        )
        .with_response_headers(response_headers.clone());

        let result = create_binary_response_handler(options).expect("binary response is handled");

        assert_eq!(result.value(), &vec![1, 2, 3, 4]);
        assert_eq!(result.response_headers(), Some(&response_headers));
        assert_eq!(result.raw_value(), None);
    }

    #[test]
    fn response_handler_upstream_binary_handler_handles_binary_response_successfully() {
        let binary_data = vec![1, 2, 3, 4];
        let options =
            BinaryResponseHandlerOptions::new("test-url", json!({}), 200, binary_data.clone());

        let result = create_binary_response_handler(options).expect("binary response is handled");

        assert_eq!(result.value(), &binary_data);
    }

    #[test]
    fn create_binary_response_handler_preserves_empty_byte_body() {
        let options = BinaryResponseHandlerOptions::new(
            "https://api.example.com/files",
            json!({}),
            200,
            Vec::<u8>::new(),
        );

        let result =
            create_binary_response_handler(options).expect("empty binary body is still readable");

        assert_eq!(result.value(), &Vec::<u8>::new());
    }

    #[test]
    fn response_handler_upstream_binary_handler_throws_api_call_error_for_null_body() {
        let options = BinaryResponseHandlerOptions::empty("test-url", json!({}), 200);

        let error = create_binary_response_handler(options).expect_err("missing body is rejected");

        assert_eq!(error.message(), "Response body is empty");
    }

    #[test]
    fn create_binary_response_handler_returns_api_call_error_for_missing_body() {
        let response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_500".to_string())]);
        let options = BinaryResponseHandlerOptions::empty(
            "https://api.example.com/files",
            json!({ "file": "test" }),
            500,
        )
        .with_response_headers(response_headers.clone());

        let error = create_binary_response_handler(options).expect_err("missing body is rejected");

        assert_eq!(error.message(), "Response body is empty");
        assert_eq!(error.url(), "https://api.example.com/files");
        assert_eq!(error.request_body_values(), &json!({ "file": "test" }));
        assert_eq!(error.status_code(), Some(500));
        assert_eq!(error.response_headers(), Some(&response_headers));
        assert_eq!(error.response_body(), None);
        assert!(error.is_retryable());
    }

    #[test]
    fn json_error_response_handler_options_use_camel_case_json() {
        let options = JsonErrorResponseHandlerOptions::new(
            "https://api.example.com/models",
            json!({ "model": "test" }),
            400,
            "Bad Request",
            r#"{"code":"bad_request","message":"Invalid model"}"#,
        )
        .with_response_headers(BTreeMap::from([(
            "x-request-id".to_string(),
            "req_400".to_string(),
        )]));

        let serialized = serde_json::to_value(&options).expect("options serialize");

        assert_eq!(
            serialized,
            json!({
                "url": "https://api.example.com/models",
                "requestBodyValues": { "model": "test" },
                "statusCode": 400,
                "statusText": "Bad Request",
                "responseHeaders": {
                    "x-request-id": "req_400"
                },
                "responseBody": "{\"code\":\"bad_request\",\"message\":\"Invalid model\"}"
            })
        );

        let deserialized: JsonErrorResponseHandlerOptions =
            serde_json::from_value(serialized).expect("options deserialize");

        assert_eq!(deserialized, options);
    }

    #[test]
    fn create_json_error_response_handler_uses_parsed_error_data() {
        let response_headers =
            BTreeMap::from([("content-type".to_string(), "application/json".to_string())]);
        let options = JsonErrorResponseHandlerOptions::new(
            "https://api.example.com/models",
            json!({ "model": "test" }),
            400,
            "Bad Request",
            r#"{"code":"bad_request","message":"Invalid model"}"#,
        )
        .with_response_headers(response_headers.clone());

        let result = create_json_error_response_handler(
            options,
            validate_error_payload,
            |error| format!("{}: {}", error.code, error.message),
            |status_code, error| {
                assert_eq!(status_code, 400);
                assert_eq!(error.map(|error| error.code.as_str()), Some("bad_request"));
                Some(false)
            },
        );
        let error = result.value();

        assert_eq!(result.response_headers(), Some(&response_headers));
        assert_eq!(error.message(), "bad_request: Invalid model");
        assert_eq!(error.url(), "https://api.example.com/models");
        assert_eq!(error.request_body_values(), &json!({ "model": "test" }));
        assert_eq!(error.status_code(), Some(400));
        assert_eq!(error.response_headers(), Some(&response_headers));
        assert_eq!(
            error.response_body(),
            Some("{\"code\":\"bad_request\",\"message\":\"Invalid model\"}")
        );
        assert_eq!(
            error.data(),
            Some(&json!({ "code": "bad_request", "message": "Invalid model" }))
        );
        assert!(!error.is_retryable());
    }

    #[test]
    fn create_json_error_response_handler_falls_back_for_empty_body() {
        let options = JsonErrorResponseHandlerOptions::new(
            "https://api.example.com/models",
            json!({ "model": "test" }),
            400,
            "Bad Request",
            " \n\t ",
        );

        let result = create_json_error_response_handler(
            options,
            validate_error_payload,
            |error| error.message.clone(),
            |status_code, error: Option<&ErrorPayload>| {
                assert_eq!(status_code, 400);
                assert!(error.is_none());
                Some(true)
            },
        );
        let error = result.value();

        assert_eq!(error.message(), "Bad Request");
        assert_eq!(error.response_body(), Some(" \n\t "));
        assert_eq!(error.data(), None);
        assert!(error.is_retryable());
    }

    #[test]
    fn create_json_error_response_handler_falls_back_for_invalid_json() {
        let response_headers =
            BTreeMap::from([("content-type".to_string(), "application/json".to_string())]);
        let options = JsonErrorResponseHandlerOptions::new(
            "https://api.example.com/models",
            json!({ "model": "test" }),
            502,
            "Bad Gateway",
            "{not json",
        )
        .with_response_headers(response_headers.clone());

        let result = create_json_error_response_handler(
            options,
            validate_error_payload,
            |error| error.message.clone(),
            |_, error: Option<&ErrorPayload>| {
                assert!(error.is_none());
                None
            },
        );
        let error = result.value();

        assert_eq!(result.response_headers(), Some(&response_headers));
        assert_eq!(error.message(), "Bad Gateway");
        assert_eq!(error.status_code(), Some(502));
        assert_eq!(error.response_body(), Some("{not json"));
        assert_eq!(error.data(), None);
        assert!(error.is_retryable());
    }

    #[test]
    fn json_response_handler_options_use_camel_case_json() {
        let options = JsonResponseHandlerOptions::new(
            "https://api.example.com/models",
            json!({ "model": "test" }),
            200,
            r#"{"name":"John"}"#,
        )
        .with_response_headers(BTreeMap::from([(
            "content-type".to_string(),
            "application/json".to_string(),
        )]));

        let serialized = serde_json::to_value(&options).expect("options serialize");

        assert_eq!(
            serialized,
            json!({
                "url": "https://api.example.com/models",
                "requestBodyValues": { "model": "test" },
                "statusCode": 200,
                "responseHeaders": {
                    "content-type": "application/json"
                },
                "responseBody": "{\"name\":\"John\"}"
            })
        );

        let deserialized: JsonResponseHandlerOptions =
            serde_json::from_value(serialized).expect("options deserialize");

        assert_eq!(deserialized, options);
    }

    #[test]
    fn create_json_response_handler_returns_validated_value_raw_value_and_headers() {
        let response_headers =
            BTreeMap::from([("content-type".to_string(), "application/json".to_string())]);
        let options = JsonResponseHandlerOptions::new(
            "https://api.example.com/users",
            json!({ "query": "john" }),
            200,
            r#"{"name":"John","age":30,"extraField":"ignored"}"#,
        )
        .with_response_headers(response_headers.clone());

        let result = create_json_response_handler(options, validate_person)
            .expect("valid JSON response is handled");

        assert_eq!(
            result.value(),
            &Person {
                name: "John".to_string(),
                age: 30,
            }
        );
        assert_eq!(
            result.raw_value(),
            Some(&json!({ "name": "John", "age": 30, "extraField": "ignored" }))
        );
        assert_eq!(result.response_headers(), Some(&response_headers));
    }

    #[test]
    fn response_handler_upstream_json_handler_returns_parsed_value_and_raw_value() {
        let options = JsonResponseHandlerOptions::new(
            "test-url",
            json!({}),
            200,
            r#"{"name":"John","age":30,"extraField":"ignored"}"#,
        );

        let result = create_json_response_handler(options, validate_person)
            .expect("valid JSON response is handled");

        assert_eq!(
            result.value(),
            &Person {
                name: "John".to_string(),
                age: 30,
            }
        );
        assert_eq!(
            result.raw_value(),
            Some(&json!({ "name": "John", "age": 30, "extraField": "ignored" }))
        );
    }

    #[test]
    fn create_json_response_handler_returns_api_call_error_for_invalid_json() {
        let response_headers =
            BTreeMap::from([("content-type".to_string(), "application/json".to_string())]);
        let options = JsonResponseHandlerOptions::new(
            "https://api.example.com/users",
            json!({ "query": "john" }),
            502,
            "{not json",
        )
        .with_response_headers(response_headers.clone());

        let error = create_json_response_handler(options, |value| {
            Ok::<JsonValue, &'static str>(value.clone())
        })
        .expect_err("invalid JSON response becomes an API call error");

        assert_eq!(error.message(), "Invalid JSON response");
        assert_eq!(error.url(), "https://api.example.com/users");
        assert_eq!(error.request_body_values(), &json!({ "query": "john" }));
        assert_eq!(error.status_code(), Some(502));
        assert_eq!(error.response_headers(), Some(&response_headers));
        assert_eq!(error.response_body(), Some("{not json"));
        assert!(error.is_retryable());
    }

    #[test]
    fn create_json_response_handler_returns_api_call_error_for_validation_failure() {
        let options = JsonResponseHandlerOptions::new(
            "https://api.example.com/users",
            json!({ "query": "john" }),
            200,
            r#"{"name":"John"}"#,
        );

        let error = create_json_response_handler(options, validate_person)
            .expect_err("schema validation failure becomes an API call error");

        assert_eq!(error.message(), "Invalid JSON response");
        assert_eq!(error.status_code(), Some(200));
        assert_eq!(error.response_body(), Some("{\"name\":\"John\"}"));
        assert!(!error.is_retryable());
    }

    #[test]
    fn status_code_error_response_handler_options_use_camel_case_json() {
        let options = StatusCodeErrorResponseHandlerOptions::new(
            "https://api.example.com/models",
            json!({ "model": "test" }),
            404,
            "Not Found",
            "missing",
        )
        .with_response_headers(BTreeMap::from([(
            "x-request-id".to_string(),
            "req_404".to_string(),
        )]));

        let serialized = serde_json::to_value(&options).expect("options serialize");

        assert_eq!(
            serialized,
            json!({
                "url": "https://api.example.com/models",
                "requestBodyValues": { "model": "test" },
                "statusCode": 404,
                "statusText": "Not Found",
                "responseHeaders": {
                    "x-request-id": "req_404"
                },
                "responseBody": "missing"
            })
        );

        let deserialized: StatusCodeErrorResponseHandlerOptions =
            serde_json::from_value(serialized).expect("options deserialize");

        assert_eq!(deserialized, options);
    }

    #[test]
    fn response_handler_upstream_status_code_handler_creates_error_with_status_text_and_body() {
        let options = StatusCodeErrorResponseHandlerOptions::new(
            "test-url",
            json!({ "some": "data" }),
            404,
            "Not Found",
            "Error message",
        );

        let result = create_status_code_error_response_handler(options);
        let error = result.value();

        assert_eq!(error.message(), "Not Found");
        assert_eq!(error.status_code(), Some(404));
        assert_eq!(error.response_body(), Some("Error message"));
        assert_eq!(error.url(), "test-url");
        assert_eq!(error.request_body_values(), &json!({ "some": "data" }));
    }

    #[test]
    fn create_status_code_error_response_handler_returns_api_call_error_result() {
        let response_headers =
            BTreeMap::from([("content-type".to_string(), "text/plain".to_string())]);
        let options = StatusCodeErrorResponseHandlerOptions::new(
            "https://api.example.com/models",
            json!({ "model": "test" }),
            429,
            "Too Many Requests",
            "retry later",
        )
        .with_response_headers(response_headers.clone());

        let result = create_status_code_error_response_handler(options);
        let error = result.value();

        assert_eq!(result.response_headers(), Some(&response_headers));
        assert_eq!(error.message(), "Too Many Requests");
        assert_eq!(error.url(), "https://api.example.com/models");
        assert_eq!(error.request_body_values(), &json!({ "model": "test" }));
        assert_eq!(error.status_code(), Some(429));
        assert_eq!(error.response_headers(), Some(&response_headers));
        assert_eq!(error.response_body(), Some("retry later"));
        assert!(error.is_retryable());
    }

    #[test]
    fn combine_headers_returns_empty_map_for_missing_groups() {
        assert_eq!(
            combine_headers::<String, String, Vec<(String, Option<String>)>, _>([None, None,]),
            BTreeMap::new()
        );
    }

    #[test]
    fn combine_headers_preserves_keys_and_combines_groups() {
        let headers = combine_headers([
            Some(vec![
                ("Authorization", Some("Bearer token")),
                ("X-Feature", Some("alpha")),
            ]),
            None,
            Some(vec![("X-Feature", Some("beta")), ("X-Disabled", None)]),
        ]);

        assert_eq!(
            headers,
            BTreeMap::from([
                (
                    "Authorization".to_string(),
                    Some("Bearer token".to_string())
                ),
                ("X-Disabled".to_string(), None),
                ("X-Feature".to_string(), Some("beta".to_string())),
            ])
        );
    }

    #[test]
    fn combine_headers_allows_missing_values_to_override_present_values() {
        let headers = combine_headers([
            Some(vec![("x-enabled", Some("true")), ("x-empty", Some(""))]),
            Some(vec![("x-enabled", None)]),
        ]);

        assert_eq!(
            headers,
            BTreeMap::from([
                ("x-empty".to_string(), Some("".to_string())),
                ("x-enabled".to_string(), None),
            ])
        );
    }

    #[test]
    fn normalize_headers_returns_empty_map_for_missing_input() {
        assert_eq!(
            normalize_headers::<String, String, Vec<(String, Option<String>)>>(None),
            BTreeMap::new()
        );
    }

    #[test]
    fn normalize_headers_lowercases_keys_and_filters_missing_values() {
        let headers = normalize_headers(Some(vec![
            ("Authorization", Some("Bearer token")),
            ("X-Feature", Some("beta")),
            ("X-Ignore", None),
        ]));

        assert_eq!(
            headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer token".to_string()),
                ("x-feature".to_string(), "beta".to_string()),
            ])
        );
    }

    #[test]
    fn normalize_headers_preserves_empty_strings_and_allows_later_overrides() {
        let headers = normalize_headers(Some(vec![
            ("CONTENT-TYPE", Some("text/plain")),
            ("content-type", Some("application/json")),
            ("x-empty", Some("")),
        ]));

        assert_eq!(
            headers,
            BTreeMap::from([
                ("content-type".to_string(), "application/json".to_string()),
                ("x-empty".to_string(), "".to_string()),
            ])
        );
    }

    #[test]
    fn normalize_headers_upstream_returns_empty_object_for_undefined() {
        assert_eq!(
            normalize_headers::<String, String, Vec<(String, Option<String>)>>(None),
            BTreeMap::new()
        );
    }

    #[test]
    fn normalize_headers_upstream_converts_headers_instance_to_record() {
        let headers = normalize_headers(Some(vec![
            ("Content-Type", Some("application/json")),
            ("X-Test", Some("value")),
        ]));

        assert_eq!(
            headers,
            BTreeMap::from([
                ("content-type".to_string(), "application/json".to_string()),
                ("x-test".to_string(), "value".to_string()),
            ])
        );
    }

    #[test]
    fn normalize_headers_upstream_converts_tuple_array() {
        let headers = normalize_headers(Some(vec![
            ("Content-Type", Some("application/json")),
            ("X-Test", Some("value")),
        ]));

        assert_eq!(
            headers,
            BTreeMap::from([
                ("content-type".to_string(), "application/json".to_string()),
                ("x-test".to_string(), "value".to_string()),
            ])
        );
    }

    #[test]
    fn normalize_headers_upstream_converts_plain_record_and_filters_nullish_values() {
        let headers = normalize_headers(Some(BTreeMap::from([
            ("Authorization", Some("Bearer token")),
            ("X-Feature", None),
            ("Content-Type", Some("application/json")),
        ])));

        assert_eq!(
            headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer token".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
            ])
        );
    }

    #[test]
    fn normalize_headers_upstream_handles_empty_headers_instance() {
        let headers = normalize_headers(Some(Vec::<(&str, Option<&str>)>::new()));

        assert_eq!(headers, BTreeMap::new());
    }

    #[test]
    fn normalize_headers_upstream_converts_uppercase_keys_to_lowercase() {
        let headers = normalize_headers(Some(vec![
            ("CONTENT-TYPE", Some("application/json")),
            ("X-API-KEY", Some("secret")),
        ]));

        assert_eq!(
            headers,
            BTreeMap::from([
                ("content-type".to_string(), "application/json".to_string()),
                ("x-api-key".to_string(), "secret".to_string()),
            ])
        );
    }

    #[test]
    fn with_user_agent_suffix_creates_user_agent_header() {
        let headers = with_user_agent_suffix(
            Some(vec![
                ("Content-Type", Some("application/json")),
                ("Authorization", Some("Bearer token")),
            ]),
            ["ai-sdk/0.0.0-test", "provider/test-openai"],
        );

        assert_eq!(
            headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer token".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
                (
                    "user-agent".to_string(),
                    "ai-sdk/0.0.0-test provider/test-openai".to_string(),
                ),
            ])
        );
    }

    #[test]
    fn with_user_agent_suffix_appends_to_existing_header_and_filters_empty_parts() {
        let headers = with_user_agent_suffix(
            Some(vec![
                ("User-Agent", Some("TestApp/0.0.0-test")),
                ("Accept", Some("application/json")),
            ]),
            ["", "ai-sdk/0.0.0-test", "provider/test-anthropic"],
        );

        assert_eq!(
            headers,
            BTreeMap::from([
                ("accept".to_string(), "application/json".to_string()),
                (
                    "user-agent".to_string(),
                    "TestApp/0.0.0-test ai-sdk/0.0.0-test provider/test-anthropic".to_string(),
                ),
            ])
        );
    }

    #[test]
    fn with_user_agent_suffix_removes_missing_headers_before_appending() {
        let headers = with_user_agent_suffix(
            Some(vec![
                ("Content-Type", Some("application/json")),
                ("Authorization", None),
                ("User-Agent", Some("TestApp/0.0.0-test")),
                ("Accept", Some("application/json")),
                ("Cache-Control", None),
            ]),
            ["ai-sdk/0.0.0-test"],
        );

        assert_eq!(
            headers,
            BTreeMap::from([
                ("accept".to_string(), "application/json".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
                (
                    "user-agent".to_string(),
                    "TestApp/0.0.0-test ai-sdk/0.0.0-test".to_string(),
                ),
            ])
        );
    }

    #[test]
    fn with_user_agent_suffix_sets_empty_user_agent_when_no_parts_exist() {
        assert_eq!(
            with_user_agent_suffix::<String, String, Vec<(String, Option<String>)>, String, _>(
                None,
                Vec::new(),
            ),
            BTreeMap::from([("user-agent".to_string(), String::new())])
        );
    }

    #[test]
    fn with_user_agent_suffix_upstream_creates_new_user_agent_header() {
        let headers = with_user_agent_suffix(
            Some(vec![
                ("content-type", Some("application/json")),
                ("authorization", Some("Bearer token123")),
            ]),
            ["ai-sdk/0.0.0-test", "provider/test-openai"],
        );

        assert_eq!(
            headers.get("user-agent"),
            Some(&"ai-sdk/0.0.0-test provider/test-openai".to_string())
        );
        assert_eq!(
            headers.get("content-type"),
            Some(&"application/json".to_string())
        );
        assert_eq!(
            headers.get("authorization"),
            Some(&"Bearer token123".to_string())
        );
    }

    #[test]
    fn with_user_agent_suffix_upstream_appends_suffix_parts_to_existing_user_agent_header() {
        let headers = with_user_agent_suffix(
            Some(vec![
                ("user-agent", Some("TestApp/0.0.0-test")),
                ("accept", Some("application/json")),
            ]),
            ["ai-sdk/0.0.0-test", "provider/test-anthropic"],
        );

        assert_eq!(
            headers.get("user-agent"),
            Some(&"TestApp/0.0.0-test ai-sdk/0.0.0-test provider/test-anthropic".to_string())
        );
        assert_eq!(headers.get("accept"), Some(&"application/json".to_string()));
    }

    #[test]
    fn with_user_agent_suffix_upstream_removes_missing_header_entries() {
        let headers = with_user_agent_suffix(
            Some(vec![
                ("content-type", Some("application/json")),
                ("authorization", None),
                ("user-agent", Some("TestApp/0.0.0-test")),
                ("accept", Some("application/json")),
                ("cache-control", None),
            ]),
            ["ai-sdk/0.0.0-test"],
        );

        assert_eq!(
            headers.get("user-agent"),
            Some(&"TestApp/0.0.0-test ai-sdk/0.0.0-test".to_string())
        );
        assert_eq!(
            headers.get("content-type"),
            Some(&"application/json".to_string())
        );
        assert_eq!(headers.get("accept"), Some(&"application/json".to_string()));
        assert!(!headers.contains_key("authorization"));
        assert!(!headers.contains_key("cache-control"));
    }

    #[test]
    fn with_user_agent_suffix_upstream_preserves_headers_instance_entries() {
        let headers = with_user_agent_suffix(
            Some(vec![
                ("Authorization", Some("Bearer token123")),
                ("X-Custom", Some("value")),
            ]),
            ["ai-sdk/0.0.0-test"],
        );

        assert_eq!(
            headers.get("authorization"),
            Some(&"Bearer token123".to_string())
        );
        assert_eq!(headers.get("x-custom"), Some(&"value".to_string()));
        assert_eq!(
            headers.get("user-agent"),
            Some(&"ai-sdk/0.0.0-test".to_string())
        );
    }

    #[test]
    fn with_user_agent_suffix_upstream_handles_array_header_entries() {
        let headers = with_user_agent_suffix(
            Some(vec![
                ("Authorization", Some("Bearer token123")),
                ("X-Feature", Some("alpha")),
            ]),
            ["ai-sdk/0.0.0-test"],
        );

        assert_eq!(
            headers.get("authorization"),
            Some(&"Bearer token123".to_string())
        );
        assert_eq!(headers.get("x-feature"), Some(&"alpha".to_string()));
        assert_eq!(
            headers.get("user-agent"),
            Some(&"ai-sdk/0.0.0-test".to_string())
        );
    }

    #[test]
    fn with_provider_utils_user_agent_adds_version_and_runtime_suffixes() {
        let headers = with_provider_utils_user_agent(
            Some(vec![
                ("Authorization", Some("Bearer token")),
                ("X-Ignore", None),
            ]),
            &RuntimeEnvironment::navigator_user_agent("Deno/2.0 TEST"),
        );

        assert_eq!(
            headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer token".to_string()),
                (
                    "user-agent".to_string(),
                    format!(
                        "ai-sdk/provider-utils/{} runtime/deno/2.0 test",
                        crate::VERSION
                    ),
                ),
            ])
        );
    }

    #[test]
    fn with_provider_utils_user_agent_appends_to_existing_user_agent() {
        let headers = with_provider_utils_user_agent(
            Some(vec![("User-Agent", Some("MyApp/1.0"))]),
            &RuntimeEnvironment::node_js("v22.0.0"),
        );

        assert_eq!(
            headers,
            BTreeMap::from([(
                "user-agent".to_string(),
                format!(
                    "MyApp/1.0 ai-sdk/provider-utils/{} runtime/node.js/v22.0.0",
                    crate::VERSION
                ),
            )])
        );
    }

    #[test]
    fn provider_api_request_serializes_upstream_prepared_request_shape() {
        let request = ProviderApiRequest::post(
            "https://api.example.com/v1/models",
            BTreeMap::from([
                ("content-type".to_string(), "application/json".to_string()),
                (
                    "user-agent".to_string(),
                    "ai-sdk/provider-utils/test".to_string(),
                ),
            ]),
            ProviderApiRequestBody::text("{\"model\":\"test\"}"),
            json!({ "model": "test" }),
        );

        let serialized = serde_json::to_value(&request).expect("request serializes");

        assert_eq!(
            serialized,
            json!({
                "method": "POST",
                "url": "https://api.example.com/v1/models",
                "headers": {
                    "content-type": "application/json",
                    "user-agent": "ai-sdk/provider-utils/test"
                },
                "body": {
                    "type": "text",
                    "content": "{\"model\":\"test\"}"
                },
                "requestBodyValues": { "model": "test" }
            })
        );

        let deserialized: ProviderApiRequest =
            serde_json::from_value(serialized).expect("request deserializes");

        assert_eq!(deserialized, request);
        assert_eq!(deserialized.method, ProviderApiRequestMethod::Post);
        assert_eq!(
            deserialized
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text),
            Some("{\"model\":\"test\"}")
        );
    }

    #[test]
    fn provider_api_request_body_supports_binary_content() {
        let body = ProviderApiRequestBody::bytes([1_u8, 2, 3]);
        let serialized = serde_json::to_value(&body).expect("body serializes");

        assert_eq!(
            serialized,
            json!({
                "type": "bytes",
                "content": [1, 2, 3]
            })
        );

        let deserialized: ProviderApiRequestBody =
            serde_json::from_value(serialized).expect("body deserializes");

        assert_eq!(deserialized.as_bytes(), Some([1_u8, 2, 3].as_slice()));
    }

    #[test]
    fn provider_api_request_body_supports_form_data_content() {
        let form_data = FormData {
            entries: vec![
                FormDataEntry::new("model", FormDataValue::text("gpt-image-1")),
                FormDataEntry::new("image", FormDataValue::bytes([1_u8, 2, 3])),
            ],
        };
        let body = ProviderApiRequestBody::form_data(form_data.clone());
        let serialized = serde_json::to_value(&body).expect("form-data body serializes");

        assert_eq!(
            serialized,
            json!({
                "type": "form-data",
                "content": {
                    "entries": [
                        {
                            "name": "model",
                            "value": {
                                "type": "text",
                                "value": "gpt-image-1"
                            }
                        },
                        {
                            "name": "image",
                            "value": {
                                "type": "bytes",
                                "value": [1, 2, 3]
                            }
                        }
                    ]
                }
            })
        );

        let deserialized: ProviderApiRequestBody =
            serde_json::from_value(serialized).expect("form-data body deserializes");

        assert_eq!(deserialized.as_form_data(), Some(&form_data));
    }

    #[test]
    fn provider_api_response_serializes_upstream_response_metadata_shape() {
        let response = ProviderApiResponse::text(201, "Created", r#"{"ok":true}"#).with_headers(
            BTreeMap::from([("content-type".to_string(), "application/json".to_string())]),
        );

        let serialized = serde_json::to_value(&response).expect("response serializes");

        assert_eq!(
            serialized,
            json!({
                "statusCode": 201,
                "statusText": "Created",
                "headers": {
                    "content-type": "application/json"
                },
                "body": {
                    "type": "text",
                    "content": "{\"ok\":true}"
                }
            })
        );

        let deserialized: ProviderApiResponse =
            serde_json::from_value(serialized).expect("response deserializes");

        assert_eq!(deserialized, response);
        assert!(deserialized.is_success_status());
        assert_eq!(deserialized.text_body(), Some(r#"{"ok":true}"#));
        assert_eq!(deserialized.bytes_body(), None);
    }

    #[test]
    fn provider_api_response_body_supports_binary_content() {
        let body = ProviderApiResponseBody::bytes([4_u8, 5, 6]);
        let serialized = serde_json::to_value(&body).expect("body serializes");

        assert_eq!(
            serialized,
            json!({
                "type": "bytes",
                "content": [4, 5, 6]
            })
        );

        let deserialized: ProviderApiResponseBody =
            serde_json::from_value(serialized).expect("body deserializes");

        assert_eq!(deserialized.as_bytes(), Some([4_u8, 5, 6].as_slice()));
        assert_eq!(deserialized.as_text(), None);
    }

    #[test]
    fn provider_api_response_success_status_matches_fetch_ok_range() {
        for status_code in [200, 204, 299] {
            assert!(
                ProviderApiResponse::new(status_code, "OK").is_success_status(),
                "{status_code} should be successful"
            );
        }

        for status_code in [199, 300, 404, 500] {
            assert!(
                !ProviderApiResponse::new(status_code, "Error").is_success_status(),
                "{status_code} should be failed"
            );
        }
    }

    #[test]
    fn provider_api_response_builds_text_response_handler_options() {
        let request = ProviderApiRequest::post(
            "https://api.example.com/v1/chat",
            BTreeMap::from([("authorization".to_string(), "Bearer test".to_string())]),
            ProviderApiRequestBody::text("{\"prompt\":\"hi\"}"),
            json!({ "prompt": "hi" }),
        );
        let response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_123".to_string())]);
        let response =
            ProviderApiResponse::text(429, "Too Many Requests", r#"{"error":"rate_limit"}"#)
                .with_headers(response_headers.clone());

        let status_options = response.status_code_error_response_handler_options(&request);
        assert_eq!(status_options.url, "https://api.example.com/v1/chat");
        assert_eq!(
            status_options.request_body_values,
            json!({ "prompt": "hi" })
        );
        assert_eq!(status_options.status_code, 429);
        assert_eq!(status_options.status_text, "Too Many Requests");
        assert_eq!(status_options.response_headers, response_headers);
        assert_eq!(status_options.response_body, r#"{"error":"rate_limit"}"#);

        let json_error_options = response.json_error_response_handler_options(&request);
        assert_eq!(json_error_options.status_text, "Too Many Requests");
        assert_eq!(
            json_error_options.response_body,
            r#"{"error":"rate_limit"}"#
        );

        let json_options = response.json_response_handler_options(&request);
        assert_eq!(json_options.status_code, 429);
        assert_eq!(json_options.response_body, r#"{"error":"rate_limit"}"#);
    }

    #[test]
    fn provider_api_response_builds_binary_and_event_source_handler_options() {
        let request = ProviderApiRequest::get(
            "https://api.example.com/v1/events",
            BTreeMap::from([("accept".to_string(), "text/event-stream".to_string())]),
        );
        let response_headers =
            BTreeMap::from([("content-type".to_string(), "text/event-stream".to_string())]);
        let response = ProviderApiResponse::bytes(200, "OK", [b'd', b'a', b't', b'a'])
            .with_headers(response_headers.clone());

        let binary_options = response.binary_response_handler_options(&request);
        assert_eq!(binary_options.url, "https://api.example.com/v1/events");
        assert_eq!(binary_options.request_body_values, json!({}));
        assert_eq!(binary_options.status_code, 200);
        assert_eq!(binary_options.response_headers, response_headers);
        assert_eq!(binary_options.response_body, Some(b"data".to_vec()));

        let event_options = response.event_source_response_handler_options();
        assert_eq!(event_options.response_body, Some(b"data".to_vec()));
        assert_eq!(
            event_options.response_headers,
            BTreeMap::from([("content-type".to_string(), "text/event-stream".to_string())])
        );

        let empty_options =
            ProviderApiResponse::new(204, "No Content").binary_response_handler_options(&request);
        assert_eq!(empty_options.response_body, None);
    }

    #[test]
    fn provider_api_response_decodes_binary_text_like_fetch_response_text() {
        let request = ProviderApiRequest::get("https://api.example.com/v1/data", BTreeMap::new());
        let response = ProviderApiResponse::bytes(200, "OK", [b'{', b'}']);

        let options = response.json_response_handler_options(&request);

        assert_eq!(options.response_body, "{}");
    }

    #[test]
    fn provider_api_response_handler_error_serializes_tagged_shape() {
        let error = ProviderApiResponseHandlerError::api_call(
            ApiCallError::new(
                "provider failed",
                "https://api.example.com/v1/data",
                json!({}),
            )
            .with_status_code(500),
        );

        assert_eq!(
            serde_json::to_value(&error).expect("handler error serializes"),
            json!({
                "type": "api-call",
                "error": {
                    "message": "provider failed",
                    "url": "https://api.example.com/v1/data",
                    "requestBodyValues": {},
                    "statusCode": 500,
                    "isRetryable": true
                }
            })
        );

        let deserialized: ProviderApiResponseHandlerError = serde_json::from_value(json!({
            "type": "other",
            "message": "invalid handler state"
        }))
        .expect("handler error deserializes");

        assert_eq!(deserialized.other_message(), Some("invalid handler state"));
        assert_eq!(deserialized.api_call_error(), None);
    }

    #[test]
    fn handle_provider_api_response_returns_successful_handler_result() {
        let request = ProviderApiRequest::get(
            "https://api.example.com/v1/data",
            BTreeMap::from([("authorization".to_string(), "Bearer test".to_string())]),
        );
        let response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_success".to_string())]);
        let response = ProviderApiResponse::text(200, "OK", r#"{"name":"Ada","age":36}"#)
            .with_headers(response_headers.clone());

        let result = handle_provider_api_response(
            &request,
            &response,
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_person,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        )
        .expect("successful response is handled");

        assert_eq!(
            result.value(),
            &Person {
                name: "Ada".to_string(),
                age: 36
            }
        );
        assert_eq!(
            result.raw_value(),
            Some(&json!({ "name": "Ada", "age": 36 }))
        );
        assert_eq!(result.response_headers(), Some(&response_headers));
    }

    #[test]
    fn handle_provider_api_response_returns_failed_handler_api_error() {
        let request = ProviderApiRequest::post(
            "https://api.example.com/v1/chat",
            BTreeMap::new(),
            ProviderApiRequestBody::text("{\"prompt\":\"hi\"}"),
            json!({ "prompt": "hi" }),
        );
        let response = ProviderApiResponse::text(429, "Too Many Requests", "rate limited");

        let error = handle_provider_api_response::<Person, _, _>(
            &request,
            &response,
            |_request, _response| {
                Ok(ResponseHandlerResult::new(Person {
                    name: "unused".to_string(),
                    age: 0,
                }))
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        )
        .expect_err("unsuccessful status returns the failed handler error");

        assert_eq!(error.message(), "Too Many Requests");
        assert_eq!(error.status_code(), Some(429));
        assert_eq!(error.response_body(), Some("rate limited"));
        assert_eq!(error.request_body_values(), &json!({ "prompt": "hi" }));
        assert!(error.is_retryable());
    }

    #[test]
    fn handle_provider_api_response_wraps_non_api_handler_failures() {
        let response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_wrapper".to_string())]);
        let request = ProviderApiRequest::get("https://api.example.com/v1/data", BTreeMap::new());
        let response =
            ProviderApiResponse::text(200, "OK", "not json").with_headers(response_headers.clone());

        let error = handle_provider_api_response::<Person, _, _>(
            &request,
            &response,
            |_request, _response| Err(ProviderApiResponseHandlerError::other("validator crashed")),
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        )
        .expect_err("non-api handler errors are wrapped");

        assert_eq!(error.message(), "Failed to process successful response");
        assert_eq!(error.url(), "https://api.example.com/v1/data");
        assert_eq!(error.request_body_values(), &json!({}));
        assert_eq!(error.status_code(), Some(200));
        assert_eq!(error.response_headers(), Some(&response_headers));
        assert_eq!(error.response_body(), None);
    }

    #[test]
    fn handle_provider_api_response_passes_through_api_handler_failures() {
        let request = ProviderApiRequest::get("https://api.example.com/v1/data", BTreeMap::new());
        let response = ProviderApiResponse::text(200, "OK", "not json");
        let api_error = ApiCallError::new("Invalid JSON response", request.url.clone(), json!({}))
            .with_status_code(200);

        let error = handle_provider_api_response::<Person, _, _>(
            &request,
            &response,
            |_request, _response| Err(ProviderApiResponseHandlerError::api_call(api_error.clone())),
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        )
        .expect_err("api-call handler errors are passed through");

        assert_eq!(*error, api_error);
    }

    #[test]
    fn execute_provider_api_request_sends_prepared_request_and_handles_success() {
        let request = prepare_get_from_api_request(
            "https://api.example.com/v1/data",
            Some(vec![("Authorization", Some("Bearer test"))]),
            &RuntimeEnvironment::unknown(),
        );
        let expected_request = request.clone();
        let response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_execute".to_string())]);
        let expected_response_headers = response_headers.clone();

        let result = poll_ready(execute_provider_api_request(
            request,
            move |request| {
                assert_eq!(request, expected_request);

                ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    r#"{"name":"Ada","age":36}"#,
                )
                .with_headers(response_headers)))
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_person,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect("successful transport response is handled");

        assert_eq!(
            result.value(),
            &Person {
                name: "Ada".to_string(),
                age: 36
            }
        );
        assert_eq!(result.response_headers(), Some(&expected_response_headers));
    }

    #[test]
    fn execute_provider_api_request_normalizes_transport_failures() {
        let request = ProviderApiRequest::post(
            "https://api.example.com/v1/chat",
            BTreeMap::new(),
            ProviderApiRequestBody::text("{\"prompt\":\"hi\"}"),
            json!({ "prompt": "hi" }),
        );

        let error = poll_ready(execute_provider_api_request(
            request,
            |_request| {
                ready(Err(FetchErrorInfo::new("fetch failed")
                    .with_name("TypeError")
                    .with_cause_message("ECONNRESET")))
            },
            |_request, _response| {
                Ok(ResponseHandlerResult::new(Person {
                    name: "unused".to_string(),
                    age: 0,
                }))
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("transport failure is normalized");

        let HandledFetchError::ApiCall { error } = error else {
            panic!("fetch TypeError with a cause should become an API call error");
        };

        assert_eq!(error.message(), "Cannot connect to API: ECONNRESET");
        assert_eq!(error.url(), "https://api.example.com/v1/chat");
        assert_eq!(error.request_body_values(), &json!({ "prompt": "hi" }));
        assert!(error.is_retryable());
    }

    #[test]
    fn execute_provider_api_request_preserves_abort_transport_failures() {
        let abort_error = FetchErrorInfo::new("Aborted").with_name("AbortError");
        let error = poll_ready(execute_provider_api_request(
            ProviderApiRequest::get("https://api.example.com/v1/data", BTreeMap::new()),
            {
                let abort_error = abort_error.clone();
                move |_request| ready(Err(abort_error))
            },
            |_request, _response| {
                Ok(ResponseHandlerResult::new(Person {
                    name: "unused".to_string(),
                    age: 0,
                }))
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("abort failure is preserved");

        assert_eq!(error, HandledFetchError::Original { error: abort_error });
    }

    #[test]
    fn get_from_api_options_serialize_camel_case_request_metadata() {
        let options = GetFromApiOptions::new("https://api.example.com/v1/data")
            .with_headers(vec![
                ("Authorization", Some("Bearer test")),
                ("X-Ignore", None),
            ])
            .with_environment(RuntimeEnvironment::node_js("v22.0.0"));

        assert_eq!(
            serde_json::to_value(&options).expect("get-from-api options serialize"),
            json!({
                "url": "https://api.example.com/v1/data",
                "headers": {
                    "Authorization": "Bearer test",
                    "X-Ignore": null
                },
                "environment": {
                    "nodeVersion": "v22.0.0"
                }
            })
        );

        let options: GetFromApiOptions = serde_json::from_value(json!({
            "url": "https://api.example.com/v1/data"
        }))
        .expect("minimal get-from-api options deserialize");

        assert_eq!(
            options,
            GetFromApiOptions::new("https://api.example.com/v1/data")
        );
    }

    #[test]
    fn get_from_api_prepares_request_and_handles_success() {
        let options = GetFromApiOptions::new("https://api.example.com/v1/data")
            .with_header("Authorization", "Bearer test")
            .with_environment(RuntimeEnvironment::navigator_user_agent("Deno/2.0 TEST"));
        let expected_response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_get".to_string())]);
        let response_headers = expected_response_headers.clone();

        let result = poll_ready(get_from_api(
            options,
            move |request| {
                assert_eq!(request.method, ProviderApiRequestMethod::Get);
                assert_eq!(request.url, "https://api.example.com/v1/data");
                assert_eq!(request.body, None);
                assert_eq!(request.request_body_values, json!({}));
                assert_eq!(
                    request.headers,
                    BTreeMap::from([
                        ("authorization".to_string(), "Bearer test".to_string()),
                        (
                            "user-agent".to_string(),
                            format!(
                                "ai-sdk/provider-utils/{} runtime/deno/2.0 test",
                                crate::VERSION
                            )
                        ),
                    ])
                );

                ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    r#"{"name":"Ada","age":36}"#,
                )
                .with_headers(response_headers)))
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_person,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect("get-from-api request succeeds");

        assert_eq!(
            result.value(),
            &Person {
                name: "Ada".to_string(),
                age: 36
            }
        );
        assert_eq!(result.response_headers(), Some(&expected_response_headers));
    }

    #[test]
    fn get_from_api_normalizes_transport_failures() {
        let error = poll_ready(get_from_api(
            GetFromApiOptions::new("https://api.example.com/v1/data"),
            |_request| {
                ready(Err(FetchErrorInfo::new("fetch failed")
                    .with_name("TypeError")
                    .with_cause_message("Failed to connect")))
            },
            |_request, _response| {
                Ok(ResponseHandlerResult::new(Person {
                    name: "unused".to_string(),
                    age: 0,
                }))
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("get-from-api transport failure is normalized");

        let HandledFetchError::ApiCall { error } = error else {
            panic!("fetch TypeError with a cause should become an API call error");
        };

        assert_eq!(error.message(), "Cannot connect to API: Failed to connect");
        assert_eq!(error.url(), "https://api.example.com/v1/data");
        assert_eq!(error.request_body_values(), &json!({}));
        assert!(error.is_retryable());
    }

    #[test]
    fn get_from_api_upstream_should_successfully_fetch_and_parse_data() {
        let captured_request = Arc::new(Mutex::new(None));
        let captured_request_for_transport = Arc::clone(&captured_request);

        let result = poll_ready(get_from_api(
            GetFromApiOptions::new("https://api.test.com/data")
                .with_header("Authorization", "Bearer test")
                .with_environment(RuntimeEnvironment::navigator_user_agent("test-env")),
            move |request| {
                *captured_request_for_transport.lock().unwrap() = Some(request.clone());
                ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    r#"{"name":"test","value":123}"#,
                )
                .with_headers(BTreeMap::from([(
                    "content-type".to_string(),
                    "application/json".to_string(),
                )]))))
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_name_value_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect("getFromApi success should parse JSON");

        assert_eq!(result.value(), &json!({ "name": "test", "value": 123 }));

        let request = captured_request
            .lock()
            .unwrap()
            .clone()
            .expect("transport captured request");
        assert_eq!(request.url, "https://api.test.com/data");
        assert_eq!(request.method, ProviderApiRequestMethod::Get);
        assert_eq!(
            request.headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer test".to_string()),
                (
                    "user-agent".to_string(),
                    format!("ai-sdk/provider-utils/{} runtime/test-env", crate::VERSION)
                ),
            ])
        );
    }

    #[test]
    fn get_from_api_upstream_should_handle_api_errors() {
        let error = poll_ready(get_from_api(
            GetFromApiOptions::new("https://api.test.com/data"),
            |_request| {
                ready(Ok(ProviderApiResponse::text(
                    404,
                    "Not Found",
                    r#"{"error":"Not Found"}"#,
                )))
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_name_value_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("API error should become APICallError");

        let HandledFetchError::ApiCall { error } = error else {
            panic!("failed status should return APICallError");
        };

        assert_eq!(error.message(), "Not Found");
        assert_eq!(error.status_code(), Some(404));
        assert_eq!(error.response_body(), Some(r#"{"error":"Not Found"}"#));
    }

    #[test]
    fn get_from_api_upstream_should_handle_network_errors() {
        let error = poll_ready(get_from_api(
            GetFromApiOptions::new("https://api.test.com/data"),
            |_request| {
                ready(Err(FetchErrorInfo::new("fetch failed")
                    .with_name("TypeError")
                    .with_cause_message("Failed to connect")))
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_name_value_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("network error should be normalized");

        let HandledFetchError::ApiCall { error } = error else {
            panic!("fetch TypeError with a cause should become APICallError");
        };

        assert_eq!(error.message(), "Cannot connect to API: Failed to connect");
        assert_eq!(error.url(), "https://api.test.com/data");
        assert!(error.is_retryable());
    }

    #[test]
    fn get_from_api_upstream_should_handle_abort_signals() {
        let abort_controller = LanguageModelAbortController::new();
        abort_controller.abort();
        let called = Arc::new(AtomicUsize::new(0));
        let called_by_transport = Arc::clone(&called);

        let error = poll_ready(get_from_api(
            GetFromApiOptions::new("https://api.test.com/data")
                .with_abort_signal(abort_controller.signal()),
            move |_request| {
                called_by_transport.fetch_add(1, Ordering::SeqCst);
                ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    r#"{"name":"test","value":123}"#,
                )))
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_name_value_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("aborted request should return abort error");

        assert_eq!(called.load(Ordering::SeqCst), 0);
        let HandledFetchError::Original { error } = error else {
            panic!("abort errors should pass through unchanged");
        };
        assert_eq!(error.name(), Some("AbortError"));
        assert_eq!(error.message(), "Aborted");
    }

    #[test]
    fn get_from_api_upstream_should_remove_undefined_header_entries() {
        let captured_request = Arc::new(Mutex::new(None));
        let captured_request_for_transport = Arc::clone(&captured_request);

        poll_ready(get_from_api(
            GetFromApiOptions::new("https://api.test.com/data")
                .with_headers([
                    ("Authorization", Some("Bearer test")),
                    ("X-Custom-Header", None),
                ])
                .with_environment(RuntimeEnvironment::navigator_user_agent("test-env")),
            move |request| {
                *captured_request_for_transport.lock().unwrap() = Some(request.clone());
                ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    r#"{"name":"test","value":123}"#,
                )))
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_name_value_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect("getFromApi succeeds");

        let request = captured_request
            .lock()
            .unwrap()
            .clone()
            .expect("transport captured request");
        assert_eq!(
            request.headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer test".to_string()),
                (
                    "user-agent".to_string(),
                    format!("ai-sdk/provider-utils/{} runtime/test-env", crate::VERSION)
                ),
            ])
        );
    }

    #[test]
    fn get_from_api_upstream_should_handle_errors_in_response_handlers() {
        let error = poll_ready(get_from_api(
            GetFromApiOptions::new("https://api.test.com/data"),
            |_request| ready(Ok(ProviderApiResponse::text(200, "OK", "invalid json"))),
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_name_value_response,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("response handler error should become APICallError");

        let HandledFetchError::ApiCall { error } = error else {
            panic!("handler errors should return APICallError");
        };

        assert_eq!(error.message(), "Invalid JSON response");
        assert_eq!(error.status_code(), Some(200));
        assert_eq!(error.response_body(), Some("invalid json"));
    }

    #[test]
    fn post_json_to_api_options_serialize_camel_case_request_metadata() {
        let options =
            PostJsonToApiOptions::new("https://api.example.com/v1/chat", json!({ "prompt": "Hi" }))
                .with_headers(vec![
                    ("Authorization", Some("Bearer test")),
                    ("X-Ignore", None),
                ])
                .with_environment(RuntimeEnvironment::vercel_edge());

        assert_eq!(
            serde_json::to_value(&options).expect("post-json-to-api options serialize"),
            json!({
                "url": "https://api.example.com/v1/chat",
                "headers": {
                    "Authorization": "Bearer test",
                    "X-Ignore": null
                },
                "body": {
                    "prompt": "Hi"
                },
                "environment": {
                    "hasEdgeRuntime": true
                }
            })
        );

        let options: PostJsonToApiOptions = serde_json::from_value(json!({
            "url": "https://api.example.com/v1/chat",
            "body": {
                "prompt": "Hi"
            }
        }))
        .expect("minimal post-json-to-api options deserialize");

        assert_eq!(
            options,
            PostJsonToApiOptions::new("https://api.example.com/v1/chat", json!({ "prompt": "Hi" }))
        );
    }

    #[test]
    fn post_json_to_api_options_carries_abort_signal_without_serializing_it() {
        let abort_controller = LanguageModelAbortController::new();
        let options =
            PostJsonToApiOptions::new("https://api.example.com/v1/chat", json!({ "prompt": "Hi" }))
                .with_abort_signal(abort_controller.signal());

        assert_eq!(
            serde_json::to_value(&options).expect("post-json options serialize"),
            json!({
                "url": "https://api.example.com/v1/chat",
                "body": {
                    "prompt": "Hi"
                }
            })
        );

        let request = options.into_request();
        assert!(
            request
                .abort_signal
                .as_ref()
                .is_some_and(|signal| !signal.is_aborted())
        );

        let request_signal = request.abort_signal.clone().expect("request signal set");
        abort_controller.abort_with_reason("client-disconnected");
        assert!(request_signal.is_aborted());
        assert_eq!(request_signal.reason(), Some(json!("client-disconnected")));
    }

    #[test]
    fn post_json_to_api_prepares_request_and_handles_success() {
        let options =
            PostJsonToApiOptions::new("https://api.example.com/v1/chat", json!({ "prompt": "Hi" }))
                .with_header("Authorization", "Bearer test")
                .with_environment(RuntimeEnvironment::node_js("v22.0.0"));
        let expected_response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_post_json".to_string())]);
        let response_headers = expected_response_headers.clone();

        let result = poll_ready(post_json_to_api(
            options,
            move |request| {
                assert_eq!(request.method, ProviderApiRequestMethod::Post);
                assert_eq!(request.url, "https://api.example.com/v1/chat");
                assert_eq!(request.request_body_values, json!({ "prompt": "Hi" }));
                assert_eq!(
                    request
                        .body
                        .as_ref()
                        .and_then(ProviderApiRequestBody::as_text),
                    Some("{\"prompt\":\"Hi\"}")
                );
                assert_eq!(
                    request.headers,
                    BTreeMap::from([
                        ("authorization".to_string(), "Bearer test".to_string()),
                        ("content-type".to_string(), "application/json".to_string()),
                        (
                            "user-agent".to_string(),
                            format!(
                                "ai-sdk/provider-utils/{} runtime/node.js/v22.0.0",
                                crate::VERSION
                            )
                        ),
                    ])
                );

                ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    r#"{"name":"Ada","age":36}"#,
                )
                .with_headers(response_headers)))
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_person,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect("post-json-to-api request succeeds");

        assert_eq!(
            result.value(),
            &Person {
                name: "Ada".to_string(),
                age: 36
            }
        );
        assert_eq!(result.response_headers(), Some(&expected_response_headers));
    }

    #[test]
    fn post_json_to_api_aborts_before_transport_call() {
        let abort_controller = LanguageModelAbortController::new();
        abort_controller.abort_with_reason("client-disconnected");
        let transport_calls = Arc::new(AtomicUsize::new(0));
        let transport_calls_for_request = Arc::clone(&transport_calls);

        let error = poll_ready(post_json_to_api(
            PostJsonToApiOptions::new("https://api.example.com/v1/chat", json!({ "prompt": "Hi" }))
                .with_abort_signal(abort_controller.signal()),
            move |_request| {
                transport_calls_for_request.fetch_add(1, Ordering::SeqCst);
                ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    r#"{"name":"Ada","age":36}"#,
                )))
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_person,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("aborted request fails before transport");

        assert_eq!(transport_calls.load(Ordering::SeqCst), 0);
        let HandledFetchError::Original { error } = error else {
            panic!("aborted request should preserve the abort error");
        };
        assert_eq!(error.name(), Some("AbortError"));
    }

    #[test]
    fn post_json_to_api_aborts_pending_transport_when_signal_fires() {
        struct AbortOnFirstPoll {
            abort_controller: LanguageModelAbortController,
            polls: Arc<AtomicUsize>,
        }

        impl Future for AbortOnFirstPoll {
            type Output = Result<ProviderApiResponse, FetchErrorInfo>;

            fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
                let polls = self.polls.fetch_add(1, Ordering::SeqCst);
                if polls == 0 {
                    self.abort_controller
                        .abort_with_reason("client-disconnected");
                }
                Poll::Pending
            }
        }

        let abort_controller = LanguageModelAbortController::new();
        let transport_polls = Arc::new(AtomicUsize::new(0));
        let transport_polls_for_request = Arc::clone(&transport_polls);
        let abort_controller_for_request = abort_controller.clone();

        let error = poll_until_ready(post_json_to_api(
            PostJsonToApiOptions::new("https://api.example.com/v1/chat", json!({ "prompt": "Hi" }))
                .with_abort_signal(abort_controller.signal()),
            move |request| {
                assert!(
                    request
                        .abort_signal
                        .as_ref()
                        .is_some_and(|signal| !signal.is_aborted())
                );
                AbortOnFirstPoll {
                    abort_controller: abort_controller_for_request,
                    polls: transport_polls_for_request,
                }
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_person,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("aborted pending transport fails");

        assert_eq!(transport_polls.load(Ordering::SeqCst), 1);
        assert_eq!(
            abort_controller.signal().reason(),
            Some(json!("client-disconnected"))
        );
        let HandledFetchError::Original { error } = error else {
            panic!("aborted request should preserve the abort error");
        };
        assert_eq!(error.name(), Some("AbortError"));
    }

    #[test]
    fn post_json_to_api_normalizes_transport_failures() {
        let error = poll_ready(post_json_to_api(
            PostJsonToApiOptions::new("https://api.example.com/v1/chat", json!({ "prompt": "Hi" })),
            |_request| {
                ready(Err(FetchErrorInfo::new("fetch failed")
                    .with_name("TypeError")
                    .with_cause_message("ECONNREFUSED")))
            },
            |_request, _response| {
                Ok(ResponseHandlerResult::new(Person {
                    name: "unused".to_string(),
                    age: 0,
                }))
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("post-json-to-api transport failure is normalized");

        let HandledFetchError::ApiCall { error } = error else {
            panic!("fetch TypeError with a cause should become an API call error");
        };

        assert_eq!(error.message(), "Cannot connect to API: ECONNREFUSED");
        assert_eq!(error.url(), "https://api.example.com/v1/chat");
        assert_eq!(error.request_body_values(), &json!({ "prompt": "Hi" }));
        assert!(error.is_retryable());
    }

    #[test]
    fn post_form_data_to_api_options_serialize_camel_case_request_metadata() {
        let form_data = FormData {
            entries: vec![
                FormDataEntry::new("model", FormDataValue::text("gpt-image-1")),
                FormDataEntry::new("image", FormDataValue::bytes([1_u8, 2, 3])),
            ],
        };
        let options =
            PostFormDataToApiOptions::new("https://api.example.com/v1/images", form_data.clone())
                .with_headers(vec![
                    ("Authorization", Some("Bearer test")),
                    ("X-Ignore", None),
                ])
                .with_environment(RuntimeEnvironment::vercel_edge());

        assert_eq!(
            serde_json::to_value(&options).expect("post-form-data-to-api options serialize"),
            json!({
                "url": "https://api.example.com/v1/images",
                "headers": {
                    "Authorization": "Bearer test",
                    "X-Ignore": null
                },
                "formData": {
                    "entries": [
                        {
                            "name": "model",
                            "value": {
                                "type": "text",
                                "value": "gpt-image-1"
                            }
                        },
                        {
                            "name": "image",
                            "value": {
                                "type": "bytes",
                                "value": [1, 2, 3]
                            }
                        }
                    ]
                },
                "environment": {
                    "hasEdgeRuntime": true
                }
            })
        );

        let options: PostFormDataToApiOptions = serde_json::from_value(json!({
            "url": "https://api.example.com/v1/images",
            "formData": {
                "entries": [
                    {
                        "name": "model",
                        "value": {
                            "type": "text",
                            "value": "gpt-image-1"
                        }
                    }
                ]
            }
        }))
        .expect("minimal post-form-data-to-api options deserialize");

        assert_eq!(
            options,
            PostFormDataToApiOptions::new(
                "https://api.example.com/v1/images",
                FormData {
                    entries: vec![FormDataEntry::new(
                        "model",
                        FormDataValue::text("gpt-image-1")
                    )]
                }
            )
        );
    }

    #[test]
    fn post_form_data_to_api_options_carries_abort_signal_without_serializing_it() {
        let abort_controller = LanguageModelAbortController::new();
        let form_data = FormData {
            entries: vec![FormDataEntry::new(
                "model",
                FormDataValue::text("gpt-image-1"),
            )],
        };
        let options = PostFormDataToApiOptions::new("https://api.example.com/v1/images", form_data)
            .with_abort_signal(abort_controller.signal());

        assert_eq!(
            serde_json::to_value(&options).expect("post-form-data options serialize"),
            json!({
                "url": "https://api.example.com/v1/images",
                "formData": {
                    "entries": [
                        {
                            "name": "model",
                            "value": {
                                "type": "text",
                                "value": "gpt-image-1"
                            }
                        }
                    ]
                }
            })
        );

        let request = options.into_request();
        let request_signal = request.abort_signal.clone().expect("request signal set");
        assert!(!request_signal.is_aborted());

        abort_controller.abort_with_reason("client-disconnected");
        assert!(request_signal.is_aborted());
        assert_eq!(request_signal.reason(), Some(json!("client-disconnected")));
    }

    #[test]
    fn post_form_data_to_api_prepares_request_and_handles_success() {
        let form_data = FormData {
            entries: vec![
                FormDataEntry::new("model", FormDataValue::text("gpt-image-1")),
                FormDataEntry::new("image", FormDataValue::bytes([1_u8])),
                FormDataEntry::new("image", FormDataValue::bytes([2_u8])),
            ],
        };
        let options =
            PostFormDataToApiOptions::new("https://api.example.com/v1/images", form_data.clone())
                .with_header("Authorization", "Bearer test")
                .with_environment(RuntimeEnvironment::node_js("v22.0.0"));
        let expected_response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_post_form".to_string())]);
        let response_headers = expected_response_headers.clone();

        let result = poll_ready(post_form_data_to_api(
            options,
            move |request| {
                assert_eq!(request.method, ProviderApiRequestMethod::Post);
                assert_eq!(request.url, "https://api.example.com/v1/images");
                assert_eq!(
                    request.request_body_values,
                    json!({
                        "model": "gpt-image-1",
                        "image": [2]
                    })
                );
                assert_eq!(
                    request
                        .body
                        .as_ref()
                        .and_then(ProviderApiRequestBody::as_form_data),
                    Some(&form_data)
                );
                assert_eq!(
                    request.headers,
                    BTreeMap::from([
                        ("authorization".to_string(), "Bearer test".to_string()),
                        (
                            "user-agent".to_string(),
                            format!(
                                "ai-sdk/provider-utils/{} runtime/node.js/v22.0.0",
                                crate::VERSION
                            )
                        ),
                    ])
                );

                ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    r#"{"name":"Ada","age":36}"#,
                )
                .with_headers(response_headers)))
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_person,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect("post-form-data-to-api request succeeds");

        assert_eq!(
            result.value(),
            &Person {
                name: "Ada".to_string(),
                age: 36
            }
        );
        assert_eq!(result.response_headers(), Some(&expected_response_headers));
    }

    #[test]
    fn post_to_api_options_serialize_camel_case_request_metadata() {
        let options = PostToApiOptions::new(
            "https://api.example.com/v1/upload",
            ProviderApiRequestBody::bytes([1_u8, 2, 3]),
            json!({ "filename": "image.png" }),
        )
        .with_headers(vec![
            ("Authorization", Some("Bearer test")),
            ("X-Ignore", None),
        ])
        .with_environment(RuntimeEnvironment::vercel_edge());

        assert_eq!(
            serde_json::to_value(&options).expect("post-to-api options serialize"),
            json!({
                "url": "https://api.example.com/v1/upload",
                "headers": {
                    "Authorization": "Bearer test",
                    "X-Ignore": null
                },
                "body": {
                    "type": "bytes",
                    "content": [1, 2, 3]
                },
                "requestBodyValues": {
                    "filename": "image.png"
                },
                "environment": {
                    "hasEdgeRuntime": true
                }
            })
        );

        let options: PostToApiOptions = serde_json::from_value(json!({
            "url": "https://api.example.com/v1/upload",
            "body": {
                "type": "text",
                "content": "plain body"
            },
            "requestBodyValues": {
                "filename": "notes.txt"
            }
        }))
        .expect("minimal post-to-api options deserialize");

        assert_eq!(
            options,
            PostToApiOptions::new(
                "https://api.example.com/v1/upload",
                ProviderApiRequestBody::text("plain body"),
                json!({ "filename": "notes.txt" })
            )
        );
    }

    #[test]
    fn post_to_api_options_carries_abort_signal_without_serializing_it() {
        let abort_controller = LanguageModelAbortController::new();
        let options = PostToApiOptions::new(
            "https://api.example.com/v1/upload",
            ProviderApiRequestBody::bytes([1_u8, 2, 3]),
            json!({ "filename": "image.png" }),
        )
        .with_abort_signal(abort_controller.signal());

        assert_eq!(
            serde_json::to_value(&options).expect("post-to-api options serialize"),
            json!({
                "url": "https://api.example.com/v1/upload",
                "body": {
                    "type": "bytes",
                    "content": [1, 2, 3]
                },
                "requestBodyValues": {
                    "filename": "image.png"
                }
            })
        );

        let request = options.into_request();
        let request_signal = request.abort_signal.clone().expect("request signal set");
        assert!(!request_signal.is_aborted());

        abort_controller.abort_with_reason("client-disconnected");
        assert!(request_signal.is_aborted());
        assert_eq!(request_signal.reason(), Some(json!("client-disconnected")));
    }

    #[test]
    fn post_to_api_prepares_request_and_handles_success() {
        let options = PostToApiOptions::new(
            "https://api.example.com/v1/upload",
            ProviderApiRequestBody::bytes([1_u8, 2, 3]),
            json!({ "filename": "image.png" }),
        )
        .with_header("Authorization", "Bearer test")
        .with_environment(RuntimeEnvironment::navigator_user_agent("Bun/1.2 TEST"));
        let expected_response_headers =
            BTreeMap::from([("x-request-id".to_string(), "req_post".to_string())]);
        let response_headers = expected_response_headers.clone();

        let result = poll_ready(post_to_api(
            options,
            move |request| {
                assert_eq!(request.method, ProviderApiRequestMethod::Post);
                assert_eq!(request.url, "https://api.example.com/v1/upload");
                assert_eq!(
                    request.request_body_values,
                    json!({ "filename": "image.png" })
                );
                assert_eq!(
                    request
                        .body
                        .as_ref()
                        .and_then(ProviderApiRequestBody::as_bytes),
                    Some([1_u8, 2, 3].as_slice())
                );
                assert_eq!(
                    request.headers,
                    BTreeMap::from([
                        ("authorization".to_string(), "Bearer test".to_string()),
                        (
                            "user-agent".to_string(),
                            format!(
                                "ai-sdk/provider-utils/{} runtime/bun/1.2 test",
                                crate::VERSION
                            )
                        ),
                    ])
                );

                ready(Ok(ProviderApiResponse::text(
                    200,
                    "OK",
                    r#"{"name":"Ada","age":36}"#,
                )
                .with_headers(response_headers)))
            },
            |request, response| {
                create_json_response_handler(
                    response.json_response_handler_options(request),
                    validate_person,
                )
                .map_err(ProviderApiResponseHandlerError::from)
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect("post-to-api request succeeds");

        assert_eq!(
            result.value(),
            &Person {
                name: "Ada".to_string(),
                age: 36
            }
        );
        assert_eq!(result.response_headers(), Some(&expected_response_headers));
    }

    #[test]
    fn post_to_api_normalizes_transport_failures() {
        let error = poll_ready(post_to_api(
            PostToApiOptions::new(
                "https://api.example.com/v1/upload",
                ProviderApiRequestBody::bytes([1_u8, 2, 3]),
                json!({ "filename": "image.png" }),
            ),
            |_request| {
                ready(Err(FetchErrorInfo::new("fetch failed")
                    .with_name("TypeError")
                    .with_cause_message("EPIPE")))
            },
            |_request, _response| {
                Ok(ResponseHandlerResult::new(Person {
                    name: "unused".to_string(),
                    age: 0,
                }))
            },
            |request, response| {
                Ok(create_status_code_error_response_handler(
                    response.status_code_error_response_handler_options(request),
                ))
            },
        ))
        .expect_err("post-to-api transport failure is normalized");

        let HandledFetchError::ApiCall { error } = error else {
            panic!("fetch TypeError with a cause should become an API call error");
        };

        assert_eq!(error.message(), "Cannot connect to API: EPIPE");
        assert_eq!(error.url(), "https://api.example.com/v1/upload");
        assert_eq!(
            error.request_body_values(),
            &json!({ "filename": "image.png" })
        );
        assert!(error.is_retryable());
    }

    #[test]
    fn prepare_get_from_api_request_matches_upstream_request_setup() {
        let request = prepare_get_from_api_request(
            "https://api.example.com/data",
            Some(vec![
                ("Authorization", Some("Bearer test")),
                ("X-Ignore", None),
            ]),
            &RuntimeEnvironment::navigator_user_agent("Deno/2.0 TEST"),
        );

        assert_eq!(request.method, ProviderApiRequestMethod::Get);
        assert_eq!(request.url, "https://api.example.com/data");
        assert_eq!(request.body, None);
        assert_eq!(request.request_body_values, json!({}));
        assert_eq!(
            request.headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer test".to_string()),
                (
                    "user-agent".to_string(),
                    format!(
                        "ai-sdk/provider-utils/{} runtime/deno/2.0 test",
                        crate::VERSION
                    ),
                ),
            ])
        );
    }

    #[test]
    fn prepare_post_to_api_request_matches_upstream_binary_request_setup() {
        let request = prepare_post_to_api_request(
            "https://api.example.com/upload",
            Some(vec![
                ("Authorization", Some("Bearer test")),
                ("X-Ignore", None),
            ]),
            ProviderApiRequestBody::bytes([1_u8, 2, 3]),
            json!({ "filename": "image.png" }),
            &RuntimeEnvironment::vercel_edge(),
        );

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.example.com/upload");
        assert_eq!(
            request.request_body_values,
            json!({ "filename": "image.png" })
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_bytes),
            Some([1_u8, 2, 3].as_slice())
        );
        assert_eq!(
            request.headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer test".to_string()),
                (
                    "user-agent".to_string(),
                    format!(
                        "ai-sdk/provider-utils/{} runtime/vercel-edge",
                        crate::VERSION
                    ),
                ),
            ])
        );
    }

    #[test]
    fn prepare_post_json_to_api_request_matches_upstream_request_setup() {
        let request = prepare_post_json_to_api_request(
            "https://api.example.com/data",
            Some(vec![
                ("Authorization", Some("Bearer test")),
                ("X-Ignore", None),
            ]),
            json!({ "model": "test", "prompt": "Hello" }),
            &RuntimeEnvironment::node_js("v22.0.0"),
        );

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.example.com/data");
        assert_eq!(
            request.request_body_values,
            json!({ "model": "test", "prompt": "Hello" })
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_text),
            Some("{\"model\":\"test\",\"prompt\":\"Hello\"}")
        );
        assert_eq!(
            request.headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer test".to_string()),
                ("content-type".to_string(), "application/json".to_string()),
                (
                    "user-agent".to_string(),
                    format!(
                        "ai-sdk/provider-utils/{} runtime/node.js/v22.0.0",
                        crate::VERSION
                    ),
                ),
            ])
        );
    }

    #[test]
    fn prepare_post_json_to_api_request_allows_header_overrides() {
        let request = prepare_post_json_to_api_request(
            "https://api.example.com/data",
            Some(vec![
                ("Content-Type", Some("application/custom+json")),
                ("User-Agent", Some("MyApp/1.0")),
            ]),
            json!({ "input": "test" }),
            &RuntimeEnvironment::unknown(),
        );

        assert_eq!(
            request.headers,
            BTreeMap::from([
                (
                    "content-type".to_string(),
                    "application/custom+json".to_string()
                ),
                (
                    "user-agent".to_string(),
                    format!(
                        "MyApp/1.0 ai-sdk/provider-utils/{} runtime/unknown",
                        crate::VERSION
                    ),
                ),
            ])
        );
    }

    #[test]
    fn prepare_post_form_data_to_api_request_matches_upstream_request_setup() {
        let form_data = FormData {
            entries: vec![
                FormDataEntry::new("model", FormDataValue::text("gpt-image-1")),
                FormDataEntry::new("image", FormDataValue::bytes([1_u8])),
                FormDataEntry::new("image", FormDataValue::bytes([2_u8])),
            ],
        };
        let request = prepare_post_form_data_to_api_request(
            "https://api.example.com/images",
            Some(vec![
                ("Authorization", Some("Bearer test")),
                ("X-Ignore", None),
            ]),
            form_data.clone(),
            &RuntimeEnvironment::navigator_user_agent("Bun/1.2 TEST"),
        );

        assert_eq!(request.method, ProviderApiRequestMethod::Post);
        assert_eq!(request.url, "https://api.example.com/images");
        assert_eq!(
            request.request_body_values,
            json!({
                "model": "gpt-image-1",
                "image": [2]
            })
        );
        assert_eq!(
            request
                .body
                .as_ref()
                .and_then(ProviderApiRequestBody::as_form_data),
            Some(&form_data)
        );
        assert_eq!(
            request.headers,
            BTreeMap::from([
                ("authorization".to_string(), "Bearer test".to_string()),
                (
                    "user-agent".to_string(),
                    format!(
                        "ai-sdk/provider-utils/{} runtime/bun/1.2 test",
                        crate::VERSION
                    ),
                ),
            ])
        );
    }

    #[test]
    fn runtime_environment_serializes_camel_case_shape() {
        let environment = RuntimeEnvironment {
            has_window: true,
            navigator_user_agent: Some("Node/Test".to_string()),
            node_version: Some("v22.0.0".to_string()),
            has_edge_runtime: true,
        };

        assert_eq!(
            serde_json::to_value(&environment).expect("runtime environment serializes"),
            json!({
                "hasWindow": true,
                "navigatorUserAgent": "Node/Test",
                "nodeVersion": "v22.0.0",
                "hasEdgeRuntime": true
            })
        );

        let environment: RuntimeEnvironment = serde_json::from_value(json!({
            "navigatorUserAgent": "Deno/2.0",
        }))
        .expect("runtime environment deserializes");

        assert_eq!(
            environment,
            RuntimeEnvironment {
                has_window: false,
                navigator_user_agent: Some("Deno/2.0".to_string()),
                node_version: None,
                has_edge_runtime: false,
            }
        );
    }

    #[test]
    fn runtime_environment_omits_unknown_indicators() {
        assert_eq!(
            serde_json::to_value(RuntimeEnvironment::unknown())
                .expect("unknown runtime environment serializes"),
            json!({})
        );
    }

    #[test]
    fn get_runtime_environment_user_agent_upstream_returns_correct_user_agent_for_browsers() {
        assert_eq!(
            get_runtime_environment_user_agent(&RuntimeEnvironment::browser()),
            "runtime/browser"
        );
    }

    #[test]
    fn get_runtime_environment_user_agent_upstream_returns_correct_user_agent_for_test() {
        assert_eq!(
            get_runtime_environment_user_agent(&RuntimeEnvironment::navigator_user_agent("test")),
            "runtime/test"
        );
    }

    #[test]
    fn get_runtime_environment_user_agent_upstream_returns_correct_user_agent_for_edge_runtime() {
        assert_eq!(
            get_runtime_environment_user_agent(&RuntimeEnvironment::vercel_edge()),
            "runtime/vercel-edge"
        );
    }

    #[test]
    fn get_runtime_environment_user_agent_upstream_returns_correct_user_agent_for_node_js() {
        assert_eq!(
            get_runtime_environment_user_agent(&RuntimeEnvironment::node_js("test")),
            "runtime/node.js/test"
        );
    }

    #[test]
    fn get_runtime_environment_user_agent_lowercases_navigator_user_agents() {
        assert_eq!(
            get_runtime_environment_user_agent(&RuntimeEnvironment::navigator_user_agent(
                "Deno/2.0 TEST"
            )),
            "runtime/deno/2.0 test"
        );
    }

    #[test]
    fn get_runtime_environment_user_agent_returns_unknown_for_missing_runtime() {
        assert_eq!(
            get_runtime_environment_user_agent(&RuntimeEnvironment::unknown()),
            "runtime/unknown"
        );
    }

    #[test]
    fn get_runtime_environment_user_agent_uses_upstream_probe_precedence() {
        let browser_environment = RuntimeEnvironment {
            has_window: true,
            navigator_user_agent: Some("Bun/1.2".to_string()),
            node_version: Some("v22.0.0".to_string()),
            has_edge_runtime: true,
        };
        assert_eq!(
            get_runtime_environment_user_agent(&browser_environment),
            "runtime/browser"
        );

        let navigator_environment = RuntimeEnvironment {
            has_window: false,
            navigator_user_agent: Some("Bun/1.2".to_string()),
            node_version: Some("v22.0.0".to_string()),
            has_edge_runtime: true,
        };
        assert_eq!(
            get_runtime_environment_user_agent(&navigator_environment),
            "runtime/bun/1.2"
        );

        let node_environment = RuntimeEnvironment {
            has_window: false,
            navigator_user_agent: Some(String::new()),
            node_version: Some("v20.11.1".to_string()),
            has_edge_runtime: true,
        };
        assert_eq!(
            get_runtime_environment_user_agent(&node_environment),
            "runtime/node.js/v20.11.1"
        );
    }

    #[test]
    fn is_abort_error_matches_upstream_error_names() {
        for error_name in ["AbortError", "ResponseAborted", "TimeoutError"] {
            assert!(
                is_abort_error(error_name),
                "{error_name} should be treated as an abort error"
            );
        }

        for error_name in ["aborterror", "Response aborted", "TypeError", ""] {
            assert!(
                !is_abort_error(error_name),
                "{error_name:?} should not be treated as an abort error"
            );
        }
    }

    #[test]
    fn fetch_error_info_serializes_camel_case_shape() {
        let error = FetchErrorInfo::new("fetch failed")
            .with_name("TypeError")
            .with_code("ECONNRESET")
            .with_cause_message("socket closed");

        assert_eq!(
            serde_json::to_value(&error).expect("fetch error info serializes"),
            json!({
                "name": "TypeError",
                "message": "fetch failed",
                "code": "ECONNRESET",
                "causeMessage": "socket closed"
            })
        );

        let minimal: FetchErrorInfo = serde_json::from_value(json!({
            "message": "unexpected"
        }))
        .expect("minimal fetch error info deserializes");

        assert_eq!(minimal.message(), "unexpected");
        assert_eq!(minimal.name(), None);
        assert_eq!(minimal.code(), None);
        assert_eq!(minimal.cause_message(), None);
    }

    #[test]
    fn handled_fetch_error_serializes_tagged_api_call_result() {
        let result = HandledFetchError::ApiCall {
            error: Box::new(
                ApiCallError::new(
                    "Cannot connect to API: ECONNREFUSED",
                    "https://api.example.com/v1/chat",
                    json!({ "prompt": "test" }),
                )
                .with_is_retryable(true),
            ),
        };

        assert_eq!(
            serde_json::to_value(&result).expect("handled fetch error serializes"),
            json!({
                "type": "api-call",
                "error": {
                    "message": "Cannot connect to API: ECONNREFUSED",
                    "url": "https://api.example.com/v1/chat",
                    "requestBodyValues": { "prompt": "test" },
                    "isRetryable": true
                }
            })
        );

        let original: HandledFetchError = serde_json::from_value(json!({
            "type": "original",
            "error": {
                "name": "AbortError",
                "message": "Aborted"
            }
        }))
        .expect("handled original fetch error deserializes");

        assert_eq!(
            original.original_error().map(FetchErrorInfo::name),
            Some(Some("AbortError"))
        );
        assert!(original.api_call_error().is_none());
    }

    #[test]
    fn handle_fetch_error_returns_abort_errors_unchanged() {
        let error = FetchErrorInfo::new("Aborted").with_name("AbortError");

        let result =
            handle_fetch_error(error.clone(), "https://api.example.com/v1/chat", json!({}));

        assert_eq!(result, HandledFetchError::Original { error });
    }

    #[test]
    fn handle_fetch_error_upstream_returns_abort_error_as_is() {
        let error = FetchErrorInfo::new("Aborted").with_name("AbortError");

        let result = handle_fetch_error(
            error.clone(),
            "https://api.example.com/v1/chat",
            json!({ "prompt": "test" }),
        );

        assert_eq!(result, HandledFetchError::Original { error });
    }

    fn assert_retryable_fetch_api_call(
        result: HandledFetchError,
        expected_message: &str,
    ) -> Box<ApiCallError> {
        let HandledFetchError::ApiCall { error } = result else {
            panic!("fetch error should become an API call error");
        };

        assert_eq!(error.message(), expected_message);
        assert_eq!(error.url(), "https://api.example.com/v1/chat");
        assert_eq!(error.request_body_values(), &json!({ "prompt": "test" }));
        assert!(error.is_retryable());
        error
    }

    #[test]
    fn handle_fetch_error_wraps_node_fetch_failed_type_errors() {
        let result = handle_fetch_error(
            FetchErrorInfo::new("fetch failed")
                .with_name("TypeError")
                .with_cause_message("ECONNREFUSED"),
            "https://api.example.com/v1/chat",
            json!({ "prompt": "test" }),
        );

        let HandledFetchError::ApiCall { error } = result else {
            panic!("fetch failed TypeError should become an API call error");
        };

        assert_eq!(error.message(), "Cannot connect to API: ECONNREFUSED");
        assert_eq!(error.url(), "https://api.example.com/v1/chat");
        assert_eq!(error.request_body_values(), &json!({ "prompt": "test" }));
        assert!(error.is_retryable());
        assert_eq!(error.status_code(), None);
    }

    #[test]
    fn handle_fetch_error_upstream_handles_type_error_with_fetch_failed_message() {
        let result = handle_fetch_error(
            FetchErrorInfo::new("fetch failed")
                .with_name("TypeError")
                .with_cause_message("ECONNREFUSED"),
            "https://api.example.com/v1/chat",
            json!({ "prompt": "test" }),
        );

        let error = assert_retryable_fetch_api_call(result, "Cannot connect to API: ECONNREFUSED");
        assert_eq!(error.status_code(), None);
    }

    #[test]
    fn handle_fetch_error_wraps_browser_failed_to_fetch_type_errors() {
        let result = handle_fetch_error(
            FetchErrorInfo::new("Failed to fetch")
                .with_name("TypeError")
                .with_cause_message("Network error"),
            "https://api.example.com/v1/chat",
            json!({ "prompt": "test" }),
        );

        let HandledFetchError::ApiCall { error } = result else {
            panic!("failed to fetch TypeError should become an API call error");
        };

        assert_eq!(error.message(), "Cannot connect to API: Network error");
        assert!(error.is_retryable());
    }

    #[test]
    fn handle_fetch_error_upstream_handles_type_error_with_failed_to_fetch_message() {
        let result = handle_fetch_error(
            FetchErrorInfo::new("Failed to fetch")
                .with_name("TypeError")
                .with_cause_message("Network error"),
            "https://api.example.com/v1/chat",
            json!({ "prompt": "test" }),
        );

        assert_retryable_fetch_api_call(result, "Cannot connect to API: Network error");
    }

    #[test]
    fn handle_fetch_error_leaves_fetch_failed_type_errors_without_cause_unchanged() {
        let error = FetchErrorInfo::new("fetch failed").with_name("TypeError");

        let result =
            handle_fetch_error(error.clone(), "https://api.example.com/v1/chat", json!({}));

        assert_eq!(result, HandledFetchError::Original { error });
    }

    #[test]
    fn handle_fetch_error_wraps_bun_network_errors() {
        for code in [
            "ConnectionRefused",
            "ConnectionClosed",
            "FailedToOpenSocket",
            "ECONNRESET",
            "ECONNREFUSED",
            "ETIMEDOUT",
            "EPIPE",
        ] {
            let result = handle_fetch_error(
                FetchErrorInfo::new("socket unavailable").with_code(code),
                "https://api.example.com/v1/chat",
                json!({ "prompt": "test" }),
            );

            let HandledFetchError::ApiCall { error } = result else {
                panic!("{code} should become an API call error");
            };

            assert_eq!(error.message(), "Cannot connect to API: socket unavailable");
            assert!(error.is_retryable());
        }
    }

    #[test]
    fn handle_fetch_error_upstream_handles_connection_refused_error() {
        let result = handle_fetch_error(
            FetchErrorInfo::new("Unable to connect. Is the computer able to access the url?")
                .with_code("ConnectionRefused"),
            "https://api.example.com/v1/chat",
            json!({ "prompt": "test" }),
        );

        assert_retryable_fetch_api_call(
            result,
            "Cannot connect to API: Unable to connect. Is the computer able to access the url?",
        );
    }

    #[test]
    fn handle_fetch_error_upstream_handles_connection_closed_error() {
        let result = handle_fetch_error(
            FetchErrorInfo::new("The socket connection was closed unexpectedly")
                .with_code("ConnectionClosed"),
            "https://api.example.com/v1/chat",
            json!({ "prompt": "test" }),
        );

        assert_retryable_fetch_api_call(
            result,
            "Cannot connect to API: The socket connection was closed unexpectedly",
        );
    }

    #[test]
    fn handle_fetch_error_upstream_handles_failed_to_open_socket_error() {
        let result = handle_fetch_error(
            FetchErrorInfo::new("Was there a typo in the url or port?")
                .with_code("FailedToOpenSocket"),
            "https://api.example.com/v1/chat",
            json!({ "prompt": "test" }),
        );

        assert_retryable_fetch_api_call(
            result,
            "Cannot connect to API: Was there a typo in the url or port?",
        );
    }

    #[test]
    fn handle_fetch_error_upstream_handles_econnreset_error() {
        let result = handle_fetch_error(
            FetchErrorInfo::new(
                "Client network socket disconnected before secure TLS connection was established",
            )
            .with_code("ECONNRESET"),
            "https://api.example.com/v1/chat",
            json!({ "prompt": "test" }),
        );

        assert_retryable_fetch_api_call(
            result,
            "Cannot connect to API: Client network socket disconnected before secure TLS connection was established",
        );
    }

    #[test]
    fn handle_fetch_error_returns_unknown_errors_unchanged() {
        let error = FetchErrorInfo::new("Something unexpected");

        let result =
            handle_fetch_error(error.clone(), "https://api.example.com/v1/chat", json!({}));

        assert_eq!(result, HandledFetchError::Original { error });
    }

    #[test]
    fn handle_fetch_error_upstream_returns_unknown_errors_as_is() {
        let error = FetchErrorInfo::new("Something unexpected");

        let result = handle_fetch_error(
            error.clone(),
            "https://api.example.com/v1/chat",
            json!({ "prompt": "test" }),
        );

        assert_eq!(result, HandledFetchError::Original { error });
    }

    #[test]
    fn create_tool_name_mapping_maps_provider_defined_tools() {
        let tools = vec![
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "anthropic.computer-use",
                "custom-computer-tool",
                JsonObject::new(),
            )),
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "openai.code-interpreter",
                "custom-code-tool",
                JsonObject::new(),
            )),
        ];
        let provider_tool_names = BTreeMap::from([
            (
                "anthropic.computer-use".to_string(),
                "computer_use".to_string(),
            ),
            (
                "openai.code-interpreter".to_string(),
                "code_interpreter".to_string(),
            ),
        ]);

        let mapping = create_tool_name_mapping(&tools, &provider_tool_names);

        assert_eq!(
            mapping.to_provider_tool_name("custom-computer-tool"),
            "computer_use"
        );
        assert_eq!(
            mapping.to_provider_tool_name("custom-code-tool"),
            "code_interpreter"
        );
        assert_eq!(
            mapping.to_custom_tool_name("computer_use"),
            "custom-computer-tool"
        );
        assert_eq!(
            mapping.to_custom_tool_name("code_interpreter"),
            "custom-code-tool"
        );
    }

    #[test]
    fn create_tool_name_mapping_ignores_function_tools() {
        let tools = vec![LanguageModelTool::Function(LanguageModelFunctionTool::new(
            "weather",
            object_schema(),
        ))];
        let mapping = create_tool_name_mapping(&tools, &BTreeMap::new());

        assert_eq!(mapping.to_provider_tool_name("weather"), "weather");
        assert_eq!(mapping.to_custom_tool_name("weather"), "weather");
    }

    #[test]
    fn create_tool_name_mapping_passes_through_unknown_provider_tool_ids() {
        let tools = vec![LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "unknown.tool",
            "custom-tool",
            JsonObject::new(),
        ))];
        let mapping = create_tool_name_mapping(&tools, &BTreeMap::new());

        assert_eq!(mapping.to_provider_tool_name("custom-tool"), "custom-tool");
        assert_eq!(mapping.to_custom_tool_name("unknown-name"), "unknown-name");
    }

    #[test]
    fn create_tool_name_mapping_handles_mixed_and_empty_tool_sets() {
        let provider_tool_names = BTreeMap::from([(
            "anthropic.computer-use".to_string(),
            "computer_use".to_string(),
        )]);
        let mixed_tools = vec![
            LanguageModelTool::Function(LanguageModelFunctionTool::new(
                "function-tool",
                object_schema(),
            )),
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "anthropic.computer-use",
                "provider-tool",
                JsonObject::new(),
            )),
        ];

        let empty_mapping =
            create_tool_name_mapping(Vec::<LanguageModelTool>::new().iter(), &BTreeMap::new());
        assert_eq!(empty_mapping.to_provider_tool_name("any-tool"), "any-tool");
        assert_eq!(empty_mapping.to_custom_tool_name("any-tool"), "any-tool");

        let mapping = create_tool_name_mapping(&mixed_tools, &provider_tool_names);
        assert_eq!(
            mapping.to_provider_tool_name("function-tool"),
            "function-tool"
        );
        assert_eq!(
            mapping.to_provider_tool_name("provider-tool"),
            "computer_use"
        );
        assert_eq!(mapping.to_custom_tool_name("computer_use"), "provider-tool");
    }

    #[test]
    fn create_tool_name_mapping_upstream_should_create_mappings_for_provider_defined_tools() {
        let tools = vec![
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "anthropic.computer-use",
                "custom-computer-tool",
                JsonObject::new(),
            )),
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "openai.code-interpreter",
                "custom-code-tool",
                JsonObject::new(),
            )),
        ];
        let provider_tool_names = BTreeMap::from([
            (
                "anthropic.computer-use".to_string(),
                "computer_use".to_string(),
            ),
            (
                "openai.code-interpreter".to_string(),
                "code_interpreter".to_string(),
            ),
        ]);

        let mapping = create_tool_name_mapping(&tools, &provider_tool_names);

        assert_eq!(
            mapping.to_provider_tool_name("custom-computer-tool"),
            "computer_use"
        );
        assert_eq!(
            mapping.to_provider_tool_name("custom-code-tool"),
            "code_interpreter"
        );
        assert_eq!(
            mapping.to_custom_tool_name("computer_use"),
            "custom-computer-tool"
        );
        assert_eq!(
            mapping.to_custom_tool_name("code_interpreter"),
            "custom-code-tool"
        );
    }

    #[test]
    fn create_tool_name_mapping_upstream_should_ignore_function_tools() {
        let tools = vec![LanguageModelTool::Function(
            LanguageModelFunctionTool::new("my-function-tool", object_schema())
                .with_description("A function tool"),
        )];

        let mapping = create_tool_name_mapping(&tools, &BTreeMap::new());

        assert_eq!(
            mapping.to_provider_tool_name("my-function-tool"),
            "my-function-tool"
        );
        assert_eq!(
            mapping.to_custom_tool_name("my-function-tool"),
            "my-function-tool"
        );
    }

    #[test]
    fn create_tool_name_mapping_upstream_should_return_input_when_tool_not_in_provider_tool_names()
    {
        let tools = vec![LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "unknown.tool",
            "custom-tool",
            JsonObject::new(),
        ))];

        let mapping = create_tool_name_mapping(&tools, &BTreeMap::new());

        assert_eq!(mapping.to_provider_tool_name("custom-tool"), "custom-tool");
        assert_eq!(mapping.to_custom_tool_name("unknown-name"), "unknown-name");
    }

    #[test]
    fn create_tool_name_mapping_upstream_should_return_input_when_mapping_does_not_exist() {
        let tools = vec![LanguageModelTool::Provider(LanguageModelProviderTool::new(
            "anthropic.computer-use",
            "custom-computer-tool",
            JsonObject::new(),
        ))];
        let provider_tool_names = BTreeMap::from([(
            "anthropic.computer-use".to_string(),
            "computer_use".to_string(),
        )]);

        let mapping = create_tool_name_mapping(&tools, &provider_tool_names);

        assert_eq!(
            mapping.to_provider_tool_name("non-existent-tool"),
            "non-existent-tool"
        );
        assert_eq!(
            mapping.to_custom_tool_name("non-existent-provider-tool"),
            "non-existent-provider-tool"
        );
    }

    #[test]
    fn create_tool_name_mapping_upstream_should_handle_empty_tools_array() {
        let mapping =
            create_tool_name_mapping(Vec::<LanguageModelTool>::new().iter(), &BTreeMap::new());

        assert_eq!(mapping.to_provider_tool_name("any-tool"), "any-tool");
        assert_eq!(mapping.to_custom_tool_name("any-tool"), "any-tool");
    }

    #[test]
    fn create_tool_name_mapping_upstream_should_handle_mixed_function_and_provider_defined_tools() {
        let tools = vec![
            LanguageModelTool::Function(
                LanguageModelFunctionTool::new("function-tool", object_schema())
                    .with_description("A function tool"),
            ),
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "anthropic.computer-use",
                "provider-tool",
                JsonObject::new(),
            )),
        ];
        let provider_tool_names = BTreeMap::from([(
            "anthropic.computer-use".to_string(),
            "computer_use".to_string(),
        )]);

        let mapping = create_tool_name_mapping(&tools, &provider_tool_names);

        assert_eq!(
            mapping.to_provider_tool_name("function-tool"),
            "function-tool"
        );
        assert_eq!(
            mapping.to_custom_tool_name("function-tool"),
            "function-tool"
        );
        assert_eq!(
            mapping.to_provider_tool_name("provider-tool"),
            "computer_use"
        );
        assert_eq!(mapping.to_custom_tool_name("computer_use"), "provider-tool");
    }

    #[test]
    fn tool_prepares_upstream_function_tool_shape() {
        let tool = Tool::new("weather", object_schema())
            .with_description("Look up weather.")
            .with_input_example(
                json!({
                    "city": "Brisbane"
                })
                .as_object()
                .expect("input example is an object")
                .clone(),
            )
            .with_strict(true);

        assert_eq!(
            tool.to_language_model_tool(),
            LanguageModelTool::Function(
                LanguageModelFunctionTool::new("weather", object_schema())
                    .with_description("Look up weather.")
                    .with_input_example(
                        json!({ "city": "Brisbane" })
                            .as_object()
                            .expect("input example is an object")
                            .clone()
                    )
                    .with_strict(true)
            )
        );
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "function",
                "name": "weather",
                "description": "Look up weather.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                },
                "inputExamples": [
                    {
                        "input": {
                            "city": "Brisbane"
                        }
                    }
                ],
                "strict": true
            })
        );
    }

    #[test]
    fn tool_constructor_prepares_upstream_function_tool_shape() {
        let tool = tool("weather", object_schema()).with_description("Look up weather.");

        assert!(!tool.is_dynamic());
        assert!(!tool.is_provider_tool());
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "function",
                "name": "weather",
                "description": "Look up weather.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }
            })
        );
    }

    #[test]
    fn tool_constructor_input_type_upstream_should_infer_input_type_from_zod_input_schema() {
        let input_schema = object_schema();
        let tool = tool("weather", input_schema.clone());

        assert!(!tool.is_executable());
        assert!(!tool.is_dynamic());
        assert!(!tool.is_provider_tool());
        assert_eq!(tool.input_schema, input_schema);
        assert_eq!(tool.output_schema(), None);
        assert!(matches!(
            tool.to_language_model_tool(),
            LanguageModelTool::Function(_)
        ));
    }

    #[test]
    fn tool_constructor_input_type_upstream_should_preserve_input_type_from_flexible_schema() {
        let schema = Schema::new(object_schema()).with_validator(|value| {
            value
                .get("city")
                .and_then(JsonValue::as_str)
                .map(|city| json!({ "city": city }))
                .map(ValidationResult::success)
                .unwrap_or_else(|| ValidationResult::failure("Expected city string"))
        });
        let flexible_schema = FlexibleSchema::from(schema.clone());

        let normalized = as_flexible_schema(Some(flexible_schema));
        let validated = normalized
            .validate(&json!({ "city": "Brisbane" }))
            .expect("schema validates");

        assert_eq!(normalized.json_schema(), schema.json_schema());
        assert_eq!(
            validated,
            ValidationResult::success(json!({ "city": "Brisbane" }))
        );
    }

    #[test]
    fn tool_constructor_input_type_upstream_should_infer_input_type_with_optional_default_examples()
    {
        let input_schema = schema_object(json!({
            "type": "object",
            "properties": {
                "location": { "type": "string" },
                "unit": {
                    "type": "string",
                    "enum": ["celsius", "fahrenheit"],
                    "default": "celsius"
                }
            },
            "required": ["location"]
        }));
        let input_example = json!({
            "location": "San Francisco",
            "unit": "celsius"
        })
        .as_object()
        .expect("input example is an object")
        .clone();
        let tool = tool("weather", input_schema.clone())
            .with_description("Get the weather for a location")
            .with_input_example(input_example.clone())
            .with_execute(|input, _options| {
                ready(Ok(json!({
                    "temperature": 20,
                    "unit": input["unit"]
                })))
            });

        let output = poll_ready(
            tool.execute(
                json!({
                    "location": "San Francisco",
                    "unit": "celsius"
                }),
                ToolExecutionOptions::new("call-optional-default", Vec::new()),
            )
            .expect("execute callback is configured"),
        )
        .expect("execute succeeds");

        assert_eq!(tool.input_schema, input_schema);
        assert_eq!(
            tool.input_examples,
            Some(vec![LanguageModelToolInputExample::new(input_example)])
        );
        assert_eq!(
            output,
            json!({
                "temperature": 20,
                "unit": "celsius"
            })
        );
    }

    #[test]
    fn tool_constructor_input_type_upstream_should_infer_input_type_with_refined_schema_examples() {
        let input_schema = schema_object(json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "minLength": 3,
                    "maxLength": 3
                }
            },
            "required": ["code"]
        }));
        let input_example = json!({ "code": "ABC" })
            .as_object()
            .expect("input example is an object")
            .clone();
        let tool = tool("codeDetails", input_schema.clone())
            .with_description("Get code details")
            .with_input_example(input_example.clone())
            .with_execute(|input, _options| {
                ready(Ok(json!({
                    "code": input["code"],
                    "valid": input["code"].as_str().is_some_and(|code| code.len() == 3)
                })))
            });

        let output = poll_ready(
            tool.execute(
                json!({
                    "code": "ABC"
                }),
                ToolExecutionOptions::new("call-refined", Vec::new()),
            )
            .expect("execute callback is configured"),
        )
        .expect("execute succeeds");

        assert_eq!(tool.input_schema, input_schema);
        assert_eq!(
            tool.input_examples,
            Some(vec![LanguageModelToolInputExample::new(input_example)])
        );
        assert_eq!(
            output,
            json!({
                "code": "ABC",
                "valid": true
            })
        );
    }

    #[test]
    fn tool_constructor_context_type_upstream_should_infer_context_type_from_context_schema_in_execute()
     {
        let context_schema = json_schema(
            json!({
                "type": "object",
                "properties": {
                    "userId": { "type": "string" },
                    "isAdmin": { "type": "boolean" }
                },
                "required": ["userId", "isAdmin"]
            })
            .as_object()
            .expect("context schema is an object")
            .clone(),
        );
        let tool = tool("weather", object_schema())
            .with_context_schema(context_schema.clone())
            .with_execute(|input, options| {
                ready(Ok(json!({
                    "number": input["number"],
                    "userId": options
                        .context
                        .as_ref()
                        .and_then(|context| context.get("userId"))
                        .cloned()
                        .unwrap_or(JsonValue::Null),
                    "isAdmin": options
                        .context
                        .as_ref()
                        .and_then(|context| context.get("isAdmin"))
                        .cloned()
                        .unwrap_or(JsonValue::Null)
                })))
            });

        let output = poll_ready(
            tool.execute(
                json!({ "number": 7 }),
                ToolExecutionOptions::new("call-context", Vec::new()).with_context(json!({
                    "userId": "user-1",
                    "isAdmin": true
                })),
            )
            .expect("execute callback is configured"),
        )
        .expect("execute succeeds");

        assert!(tool.context_schema().is_some());
        assert_eq!(
            tool.context_schema()
                .expect("context schema is present")
                .as_schema()
                .json_schema(),
            context_schema.json_schema()
        );
        assert_eq!(
            output,
            json!({
                "number": 7,
                "userId": "user-1",
                "isAdmin": true
            })
        );
    }

    #[test]
    fn tool_constructor_context_type_upstream_should_infer_context_type_in_input_lifecycle_callbacks()
     {
        let context_schema = json_schema(
            json!({
                "type": "object",
                "properties": {
                    "requestId": { "type": "string" }
                },
                "required": ["requestId"]
            })
            .as_object()
            .expect("context schema is an object")
            .clone(),
        );
        let recorded = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let start_recorded = Arc::clone(&recorded);
        let delta_recorded = Arc::clone(&recorded);
        let available_recorded = Arc::clone(&recorded);
        let tool = tool("weather", object_schema())
            .with_context_schema(context_schema)
            .with_on_input_start(move |options| {
                let recorded = Arc::clone(&start_recorded);
                async move {
                    recorded.lock().expect("recorded lock").push(json!({
                        "type": "start",
                        "context": options.context
                    }));
                }
            })
            .with_on_input_delta(move |options| {
                let recorded = Arc::clone(&delta_recorded);
                async move {
                    recorded.lock().expect("recorded lock").push(json!({
                        "type": "delta",
                        "inputTextDelta": options.input_text_delta,
                        "context": options.context
                    }));
                }
            })
            .with_on_input_available(move |options| {
                let recorded = Arc::clone(&available_recorded);
                async move {
                    recorded.lock().expect("recorded lock").push(json!({
                        "type": "available",
                        "input": options.input,
                        "context": options.context
                    }));
                }
            });

        poll_ready(
            tool.on_input_start(
                ToolExecutionOptions::new("call-lifecycle", Vec::new()).with_context(json!({
                    "requestId": "req-1"
                })),
            )
            .expect("input start callback is configured"),
        );
        poll_ready(
            tool.on_input_delta(
                ToolInputDeltaOptions::new("call-lifecycle", Vec::new(), r#"{"number":7"#)
                    .with_context(json!({
                        "requestId": "req-1"
                    })),
            )
            .expect("input delta callback is configured"),
        );
        poll_ready(
            tool.on_input_available(
                ToolInputAvailableOptions::new(
                    "call-lifecycle",
                    Vec::new(),
                    json!({ "number": 7 }),
                )
                .with_context(json!({
                    "requestId": "req-1"
                })),
            )
            .expect("input available callback is configured"),
        );

        assert_eq!(
            recorded.lock().expect("recorded lock").as_slice(),
            [
                json!({
                    "type": "start",
                    "context": { "requestId": "req-1" }
                }),
                json!({
                    "type": "delta",
                    "inputTextDelta": r#"{"number":7"#,
                    "context": { "requestId": "req-1" }
                }),
                json!({
                    "type": "available",
                    "input": { "number": 7 },
                    "context": { "requestId": "req-1" }
                })
            ]
        );
    }

    #[test]
    fn tool_constructor_output_type_upstream_should_infer_output_type_from_execute_function() {
        let tool = tool("weather", object_schema()).with_execute(|input, _options| {
            ready(Ok(json!({
                "number": input["number"],
                "status": "test"
            })))
        });

        let output = poll_ready(
            tool.execute(
                json!({ "number": 7 }),
                ToolExecutionOptions::new("call-output", Vec::new()),
            )
            .expect("execute callback is configured"),
        )
        .expect("execute succeeds");

        assert!(tool.is_executable());
        assert_eq!(tool.output_schema(), None);
        assert_eq!(
            output,
            json!({
                "number": 7,
                "status": "test"
            })
        );
    }

    #[test]
    fn tool_constructor_output_type_upstream_should_infer_output_type_from_async_generator_execute_function()
     {
        let tool = tool("weather", object_schema()).with_execute_outputs(|_input, _options| {
            ready(Ok(vec![ExecuteToolOutput::preliminary(json!("test"))]))
        });

        let outputs = poll_ready(execute_tool(
            &tool,
            json!({ "number": 7 }),
            ToolExecutionOptions::new("call-output-stream", Vec::new()),
        ))
        .expect("execute succeeds");

        assert!(tool.is_executable());
        assert_eq!(
            outputs,
            vec![
                ExecuteToolOutput::preliminary(json!("test")),
                ExecuteToolOutput::final_output(json!("test"))
            ]
        );
    }

    #[test]
    fn tool_needs_approval_options_use_upstream_shape() {
        let options = ToolNeedsApprovalOptions::new(
            "call-1",
            vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
                vec![LanguageModelUserContentPart::Text(
                    LanguageModelTextPart::new("Weather?"),
                )],
            ))],
        )
        .with_context(json!({ "risk": "high" }));

        assert_eq!(
            serde_json::to_value(&options).expect("options serialize"),
            json!({
                "toolCallId": "call-1",
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": "Weather?"
                            }
                        ]
                    }
                ],
                "context": {
                    "risk": "high"
                }
            })
        );

        let round_tripped: ToolNeedsApprovalOptions = serde_json::from_value(json!({
            "toolCallId": "call-2",
            "messages": [],
            "context": {
                "risk": "low"
            }
        }))
        .expect("options deserialize");

        assert_eq!(round_tripped.tool_call_id, "call-2");
        assert_eq!(round_tripped.context, Some(json!({ "risk": "low" })));
    }

    #[test]
    fn tool_defined_needs_approval_function_resolves_with_input_and_options() {
        let seen = Arc::new(Mutex::new(None::<(JsonValue, ToolNeedsApprovalOptions)>));
        let seen_for_callback = Arc::clone(&seen);
        let tool = Tool::new("weather", object_schema()).with_needs_approval_function(
            move |input, options| {
                let seen = Arc::clone(&seen_for_callback);
                async move {
                    let needs_approval = input["risk"] == json!("high");
                    seen.lock().expect("seen lock").replace((input, options));
                    needs_approval
                }
            },
        );

        assert_eq!(tool.needs_approval(), None);
        assert!(tool.has_needs_approval_function());

        let prompt = vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![LanguageModelUserContentPart::Text(
                LanguageModelTextPart::new("Weather?"),
            )],
        ))];
        let needs_approval = poll_ready(
            tool.resolve_needs_approval(
                json!({ "risk": "high" }),
                ToolNeedsApprovalOptions::new("call-1", prompt.clone())
                    .with_context(json!({ "tenant": "acme" })),
            )
            .expect("approval function is configured"),
        );

        assert!(needs_approval);
        let seen = seen.lock().expect("seen lock");
        let (input, options) = seen.as_ref().expect("callback captured options");
        assert_eq!(input["risk"], json!("high"));
        assert_eq!(options.tool_call_id, "call-1");
        assert_eq!(options.messages, prompt);
        assert_eq!(options.context, Some(json!({ "tenant": "acme" })));
    }

    #[test]
    fn tool_needs_approval_function_accepts_input_schema_options() {
        let tool = Tool::new("weather", object_schema()).with_needs_approval_function(
            |input, options| async move {
                input["number"] == json!(7) && options.tool_call_id == "call-input-only"
            },
        );

        let needs_approval = poll_ready(
            tool.resolve_needs_approval(
                json!({ "number": 7 }),
                ToolNeedsApprovalOptions::new("call-input-only", Vec::new()),
            )
            .expect("approval callback is configured"),
        );

        assert!(needs_approval);
    }

    #[test]
    fn tool_needs_approval_function_accepts_execute_tool_options() {
        let tool = Tool::new("weather", object_schema())
            .with_execute(|input, _options| async move {
                Ok(json!({
                    "approvedInput": input
                }))
            })
            .with_needs_approval_function(|input, options| async move {
                input["number"] == json!(7)
                    && options.tool_call_id == "call-execute"
                    && options.context == Some(json!({ "approval": "required" }))
            });

        assert!(tool.is_executable());
        let needs_approval = poll_ready(
            tool.resolve_needs_approval(
                json!({ "number": 7 }),
                ToolNeedsApprovalOptions::new("call-execute", Vec::new())
                    .with_context(json!({ "approval": "required" })),
            )
            .expect("approval callback is configured"),
        );

        assert!(needs_approval);
    }

    #[test]
    fn tool_needs_approval_function_accepts_context_schema_context() {
        let context_schema = Schema::new(
            json!({
                "type": "object",
                "properties": {
                    "sessionId": { "type": "string" },
                    "userRole": {
                        "type": "string",
                        "enum": ["user", "admin"]
                    }
                },
                "required": ["sessionId", "userRole"]
            })
            .as_object()
            .expect("context schema is an object")
            .clone(),
        );
        let tool = Tool::new("weather", object_schema())
            .with_context_schema(context_schema)
            .with_needs_approval_function(|input, options| async move {
                input["number"] == json!(7)
                    && options.context
                        == Some(json!({
                            "sessionId": "session-1",
                            "userRole": "admin"
                        }))
            });

        assert!(tool.context_schema().is_some());
        let needs_approval = poll_ready(
            tool.resolve_needs_approval(
                json!({ "number": 7 }),
                ToolNeedsApprovalOptions::new("call-context", Vec::new()).with_context(json!({
                    "sessionId": "session-1",
                    "userRole": "admin"
                })),
            )
            .expect("approval callback is configured"),
        );

        assert!(needs_approval);
    }

    #[test]
    fn tool_dynamic_description_uses_context_and_sandbox_when_prepared() {
        let sandbox: Arc<dyn ExperimentalSandbox> =
            Arc::new(StaticSandbox::new("workspace sandbox"));
        let mut tools_context = JsonObject::new();
        tools_context.insert(
            "weather".to_string(),
            json!({
                "region": "Brisbane"
            }),
        );

        let tool = Tool::new("weather", object_schema()).with_dynamic_description(|options| {
            let region = options
                .context
                .as_ref()
                .and_then(|context| context.get("region"))
                .and_then(JsonValue::as_str)
                .expect("context is provided");
            let sandbox_description = options
                .experimental_sandbox
                .as_ref()
                .expect("sandbox is provided")
                .description();

            format!("Look up {region} weather in {sandbox_description}.")
        });

        assert!(tool.has_dynamic_description());
        assert_eq!(
            ToolDescriptionOptions::new(None)
                .with_experimental_sandbox(Arc::clone(&sandbox))
                .experimental_sandbox
                .as_ref()
                .expect("sandbox is set")
                .description(),
            "workspace sandbox"
        );

        let tools = vec![tool];
        let prepared = prepare_tools_with_context(&tools, Some(&tools_context), Some(&sandbox))
            .expect("tools are prepared");

        assert_eq!(
            serde_json::to_value(prepared).expect("prepared tools serialize"),
            json!([
                {
                    "type": "function",
                    "name": "weather",
                    "description": "Look up Brisbane weather in workspace sandbox.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "city": { "type": "string" }
                        },
                        "required": ["city"]
                    }
                }
            ])
        );
    }

    #[test]
    fn tool_metadata_is_retained_but_not_sent_to_provider() {
        let metadata = json!({
            "source": "mcp",
            "server": "weather-tools"
        })
        .as_object()
        .expect("metadata is an object")
        .clone();
        let tool = Tool::new("weather", object_schema()).with_metadata(metadata.clone());

        assert_eq!(tool.metadata(), Some(&metadata));
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "function",
                "name": "weather",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }
            })
        );
    }

    #[test]
    fn tool_title_is_retained_but_not_sent_to_provider() {
        let tool = Tool::new("weather", object_schema()).with_title("Weather information");

        assert_eq!(tool.title(), Some("Weather information"));
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "function",
                "name": "weather",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }
            })
        );
    }

    #[test]
    fn tool_context_schema_is_retained_but_not_sent_to_provider() {
        let context_schema = Schema::new(
            json!({
                "type": "object",
                "properties": {
                    "apiKey": { "type": "string" }
                },
                "required": ["apiKey"]
            })
            .as_object()
            .expect("context schema is an object")
            .clone(),
        );
        let tool = Tool::new("weather", object_schema()).with_context_schema(context_schema);

        assert!(tool.context_schema().is_some());
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "function",
                "name": "weather",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }
            })
        );
    }

    #[test]
    fn dynamic_tool_upstream_should_include_dynamic_tools_in_the_tool_union() {
        let dynamic = dynamic_tool("runtimeWeather", object_schema());
        let as_tool: &Tool = &dynamic;

        assert!(as_tool.is_dynamic());
        assert!(!as_tool.is_provider_tool());
        assert!(!as_tool.is_provider_executed());
    }

    #[test]
    fn dynamic_tool_upstream_should_allow_function_style_properties() {
        let input_example = json!({ "location": "San Francisco" })
            .as_object()
            .expect("input example is an object")
            .clone();
        let output_schema = schema_object(json!({ "type": "string" }));
        let tool = dynamic_tool("weather", object_schema())
            .with_description("Get the weather for a location")
            .with_strict(true)
            .with_input_example(input_example.clone())
            .with_output_schema(output_schema.clone());

        assert_eq!(
            tool.description.as_deref(),
            Some("Get the weather for a location")
        );
        assert_eq!(tool.strict, Some(true));
        assert_eq!(
            tool.input_examples,
            Some(vec![LanguageModelToolInputExample::new(input_example)])
        );
        assert_eq!(tool.output_schema(), Some(&output_schema));
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "function",
                "name": "weather",
                "description": "Get the weather for a location",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                },
                "inputExamples": [
                    {
                        "input": {
                            "location": "San Francisco"
                        }
                    }
                ],
                "strict": true
            })
        );
    }

    #[test]
    fn dynamic_tool_upstream_should_reject_provider_only_properties() {
        let tool = dynamic_tool("weather", object_schema()).with_supports_deferred_results(true);

        assert!(tool.is_dynamic());
        assert!(!tool.is_provider_tool());
        assert!(!tool.is_provider_executed());
        assert_eq!(tool.provider_tool_id(), None);
        assert_eq!(tool.provider_tool_args(), None);
        assert_eq!(tool.supports_deferred_results(), None);
    }

    #[test]
    fn dynamic_tool_upstream_should_create_dynamic_tools_with_the_dynamic_discriminator() {
        let tool = dynamic_tool("weather", object_schema());

        assert!(tool.is_dynamic());
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "function",
                "name": "weather",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }
            })
        );
    }

    #[test]
    fn dynamic_tool_prepares_upstream_function_tool_shape() {
        let tool = dynamic_tool("mcpWeather", object_schema())
            .with_description("Runtime weather lookup.")
            .with_strict(true);

        assert!(tool.is_dynamic());
        assert!(!tool.is_provider_tool());
        assert!(!tool.is_provider_executed());
        assert_eq!(tool.provider_tool_id(), None);
        assert_eq!(tool.provider_tool_args(), None);
        assert_eq!(tool.output_schema(), None);
        assert_eq!(tool.supports_deferred_results(), None);
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "function",
                "name": "mcpWeather",
                "description": "Runtime weather lookup.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                },
                "strict": true
            })
        );
    }

    #[test]
    fn function_tool_retains_output_schema_without_provider_serialization() {
        let output_schema = json!({
            "type": "object",
            "properties": {
                "forecast": { "type": "string" }
            },
            "required": ["forecast"]
        })
        .as_object()
        .expect("output schema is an object")
        .clone();
        let tool = tool("weather", object_schema()).with_output_schema(output_schema.clone());

        assert_eq!(tool.output_schema(), Some(&output_schema));
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "function",
                "name": "weather",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }
            })
        );
    }

    #[test]
    fn provider_defined_tool_upstream_should_include_provider_defined_tools_in_the_tool_union() {
        let args = json!({ "maxResults": 3 })
            .as_object()
            .expect("args are an object")
            .clone();
        let tool =
            Tool::provider_defined("webSearch", "provider.web_search", args, object_schema());
        let as_tool: &Tool = &tool;

        assert!(as_tool.is_provider_tool());
        assert!(!as_tool.is_provider_executed());
        assert!(!as_tool.is_dynamic());
    }

    #[test]
    fn provider_defined_tool_upstream_should_require_provider_specific_properties() {
        let args = json!({ "maxResults": 3 })
            .as_object()
            .expect("args are an object")
            .clone();
        let tool = Tool::provider_defined(
            "webSearch",
            "provider.web_search",
            args.clone(),
            object_schema(),
        );

        assert_eq!(tool.provider_tool_id(), Some("provider.web_search"));
        assert_eq!(tool.provider_tool_args(), Some(&args));
        assert!(!tool.is_provider_executed());
        assert_eq!(tool.supports_deferred_results(), None);
    }

    #[test]
    fn provider_defined_tool_upstream_should_allow_user_execution_or_an_output_schema() {
        let args = json!({ "maxResults": 3 })
            .as_object()
            .expect("args are an object")
            .clone();
        let executable_tool = Tool::provider_defined(
            "webSearch",
            "provider.web_search",
            args.clone(),
            object_schema(),
        )
        .with_execute(|input, _options| ready(Ok(json!({ "echo": input }))));
        let output = poll_ready(
            executable_tool
                .execute(
                    json!({ "location": "Brisbane" }),
                    ToolExecutionOptions::new("call-1", Vec::new()),
                )
                .expect("execute callback is configured"),
        )
        .expect("execute succeeds");

        let output_schema = schema_object(json!({ "type": "string" }));
        let output_schema_tool =
            Tool::provider_defined("webSearch", "provider.web_search", args, object_schema())
                .with_output_schema(output_schema.clone());

        assert!(executable_tool.is_executable());
        assert_eq!(output, json!({ "echo": { "location": "Brisbane" } }));
        assert_eq!(output_schema_tool.output_schema(), Some(&output_schema));
    }

    #[test]
    fn provider_defined_tool_upstream_rejects_function_only_properties() {
        let args = json!({ "maxResults": 3 })
            .as_object()
            .expect("args are an object")
            .clone();
        let tool = Tool::provider_defined(
            "webSearch",
            "provider.web_search",
            args.clone(),
            object_schema(),
        )
        .with_description("Get weather")
        .with_input_example(
            json!({ "location": "San Francisco" })
                .as_object()
                .expect("input example is an object")
                .clone(),
        )
        .with_strict(true)
        .with_supports_deferred_results(true);

        assert_eq!(tool.description, None);
        assert_eq!(tool.input_examples, None);
        assert_eq!(tool.strict, None);
        assert_eq!(tool.supports_deferred_results(), None);
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "provider",
                "id": "provider.web_search",
                "name": "webSearch",
                "args": {
                    "maxResults": 3
                }
            })
        );
    }

    #[test]
    fn tool_prepares_upstream_provider_defined_tool_shape() {
        let args = json!({ "maxResults": 3 })
            .as_object()
            .expect("args are an object")
            .clone();
        let output_schema = json!({
            "type": "object",
            "properties": {
                "results": { "type": "array" }
            }
        })
        .as_object()
        .expect("output schema is an object")
        .clone();
        let tool = Tool::provider_defined(
            "webSearch",
            "provider.web_search",
            args.clone(),
            object_schema(),
        )
        .with_output_schema(output_schema.clone());

        assert!(tool.is_provider_tool());
        assert!(!tool.is_provider_executed());
        assert_eq!(tool.provider_tool_id(), Some("provider.web_search"));
        assert_eq!(tool.provider_tool_args(), Some(&args));
        assert_eq!(tool.output_schema(), Some(&output_schema));
        assert_eq!(tool.supports_deferred_results(), None);
        assert_eq!(
            tool.to_language_model_tool(),
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "provider.web_search",
                "webSearch",
                args.clone()
            ))
        );
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "provider",
                "id": "provider.web_search",
                "name": "webSearch",
                "args": {
                    "maxResults": 3
                }
            })
        );
    }

    #[test]
    fn provider_executed_tool_upstream_should_include_provider_executed_tools_in_the_tool_union() {
        let args = json!({ "region": "au" })
            .as_object()
            .expect("args are an object")
            .clone();
        let output_schema = schema_object(json!({ "type": "object" }));
        let tool = Tool::provider_executed(
            "codeInterpreter",
            "provider.code_interpreter",
            args,
            object_schema(),
            output_schema,
        );
        let as_tool: &Tool = &tool;

        assert!(as_tool.is_provider_tool());
        assert!(as_tool.is_provider_executed());
        assert!(!as_tool.is_dynamic());
    }

    #[test]
    fn provider_executed_tool_upstream_should_require_provider_specific_properties() {
        let args = json!({ "region": "au" })
            .as_object()
            .expect("args are an object")
            .clone();
        let output_schema = schema_object(json!({ "type": "object" }));
        let tool = Tool::provider_executed(
            "codeInterpreter",
            "provider.code_interpreter",
            args.clone(),
            object_schema(),
            output_schema.clone(),
        );

        assert_eq!(tool.provider_tool_id(), Some("provider.code_interpreter"));
        assert_eq!(tool.provider_tool_args(), Some(&args));
        assert!(tool.is_provider_executed());
        assert_eq!(tool.output_schema(), Some(&output_schema));
    }

    #[test]
    fn provider_executed_tool_upstream_should_allow_deferred_result_support() {
        let args = json!({ "region": "au" })
            .as_object()
            .expect("args are an object")
            .clone();
        let tool = Tool::provider_executed(
            "codeInterpreter",
            "provider.code_interpreter",
            args,
            object_schema(),
            schema_object(json!({ "type": "object" })),
        )
        .with_supports_deferred_results(true);

        assert_eq!(tool.supports_deferred_results(), Some(true));
    }

    #[test]
    fn provider_executed_tool_upstream_rejects_function_only_properties() {
        let args = json!({ "region": "au" })
            .as_object()
            .expect("args are an object")
            .clone();
        let tool = Tool::provider_executed(
            "codeInterpreter",
            "provider.code_interpreter",
            args.clone(),
            object_schema(),
            schema_object(json!({ "type": "object" })),
        )
        .with_description("Get weather")
        .with_input_example(
            json!({ "location": "San Francisco" })
                .as_object()
                .expect("input example is an object")
                .clone(),
        )
        .with_strict(true);

        assert_eq!(tool.description, None);
        assert_eq!(tool.input_examples, None);
        assert_eq!(tool.strict, None);
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "provider",
                "id": "provider.code_interpreter",
                "name": "codeInterpreter",
                "args": {
                    "region": "au"
                }
            })
        );
    }

    #[test]
    fn tool_prepares_upstream_provider_executed_tool_shape() {
        let args = json!({ "region": "au" })
            .as_object()
            .expect("args are an object")
            .clone();
        let output_schema = json!({ "type": "object" })
            .as_object()
            .expect("output schema is an object")
            .clone();
        let tool = Tool::provider_executed(
            "codeInterpreter",
            "provider.code_interpreter",
            args.clone(),
            object_schema(),
            output_schema.clone(),
        )
        .with_supports_deferred_results(true);

        assert!(tool.is_provider_tool());
        assert!(tool.is_provider_executed());
        assert!(!tool.is_executable());
        assert_eq!(tool.provider_tool_id(), Some("provider.code_interpreter"));
        assert_eq!(tool.provider_tool_args(), Some(&args));
        assert_eq!(tool.output_schema(), Some(&output_schema));
        assert_eq!(tool.supports_deferred_results(), Some(true));
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "provider",
                "id": "provider.code_interpreter",
                "name": "codeInterpreter",
                "args": {
                    "region": "au"
                }
            })
        );
    }

    #[test]
    fn function_tool_upstream_should_expose_the_function_tool_discriminator() {
        let tool = Tool::new("weather", object_schema());

        assert!(!tool.is_dynamic());
        assert!(!tool.is_provider_tool());
        assert_eq!(
            serde_json::to_value(tool.to_language_model_tool()).expect("tool serializes"),
            json!({
                "type": "function",
                "name": "weather",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }
            })
        );
    }

    #[test]
    fn function_tool_upstream_should_include_function_tools_in_the_tool_union() {
        let function_tool = Tool::new("weather", object_schema());
        let as_tool: &Tool = &function_tool;

        assert!(!as_tool.is_dynamic());
        assert!(!as_tool.is_provider_tool());
        assert!(!as_tool.is_provider_executed());
    }

    #[test]
    fn function_tool_upstream_should_allow_omitted_and_explicit_function_discriminators() {
        let direct_constructor = Tool::new("directWeather", object_schema());
        let helper_constructor = tool("helperWeather", object_schema());

        for function_tool in [direct_constructor, helper_constructor] {
            assert!(!function_tool.is_provider_tool());
            assert!(!function_tool.is_dynamic());
            assert!(matches!(
                function_tool.to_language_model_tool(),
                LanguageModelTool::Function(_)
            ));
        }
    }

    #[test]
    fn function_tool_upstream_should_reject_dynamic_and_provider_only_properties() {
        let tool = tool("weather", object_schema()).with_supports_deferred_results(true);

        assert!(!tool.is_dynamic());
        assert!(!tool.is_provider_tool());
        assert_eq!(tool.provider_tool_id(), None);
        assert_eq!(tool.provider_tool_args(), None);
        assert_eq!(tool.supports_deferred_results(), None);
    }

    #[test]
    fn tool_union_upstream_should_expose_all_tool_variants_and_type_discriminators() {
        let provider_args = json!({ "region": "au" })
            .as_object()
            .expect("args are an object")
            .clone();
        let tools = vec![
            Tool::new("weather", object_schema()),
            dynamic_tool("runtimeWeather", object_schema()),
            Tool::provider_defined(
                "webSearch",
                "provider.web_search",
                provider_args.clone(),
                object_schema(),
            ),
            Tool::provider_executed(
                "codeInterpreter",
                "provider.code_interpreter",
                provider_args,
                object_schema(),
                schema_object(json!({ "type": "object" })),
            ),
        ];

        let high_level_variants = tools
            .iter()
            .map(|tool| {
                if tool.is_provider_tool() {
                    if tool.is_provider_executed() {
                        "provider-executed"
                    } else {
                        "provider-defined"
                    }
                } else if tool.is_dynamic() {
                    "dynamic"
                } else {
                    "function"
                }
            })
            .collect::<Vec<_>>();
        let model_discriminators = tools
            .iter()
            .map(|tool| match tool.to_language_model_tool() {
                LanguageModelTool::Function(_) => "function",
                LanguageModelTool::Provider(_) => "provider",
            })
            .collect::<Vec<_>>();

        assert_eq!(
            high_level_variants,
            vec![
                "function",
                "dynamic",
                "provider-defined",
                "provider-executed"
            ]
        );
        assert_eq!(
            model_discriminators,
            vec!["function", "function", "provider", "provider"]
        );
    }

    #[test]
    fn tool_union_upstream_should_narrow_tools_by_type() {
        fn classify_tool(tool: &Tool) -> &'static str {
            if tool.is_provider_tool() {
                if tool.is_provider_executed() {
                    "provider-executed"
                } else {
                    "provider-defined"
                }
            } else if tool.is_dynamic() {
                "dynamic"
            } else {
                "function"
            }
        }

        let provider_args = json!({}).as_object().expect("args are an object").clone();
        let cases = vec![
            (Tool::new("weather", object_schema()), "function"),
            (dynamic_tool("runtimeWeather", object_schema()), "dynamic"),
            (
                Tool::provider_defined(
                    "webSearch",
                    "provider.web_search",
                    provider_args.clone(),
                    object_schema(),
                ),
                "provider-defined",
            ),
            (
                Tool::provider_executed(
                    "codeInterpreter",
                    "provider.code_interpreter",
                    provider_args,
                    object_schema(),
                    schema_object(json!({ "type": "object" })),
                ),
                "provider-executed",
            ),
        ];

        for (tool, expected) in cases {
            assert_eq!(classify_tool(&tool), expected);
        }
    }

    #[test]
    fn provider_defined_tool_factory_round_trips_upstream_config_shape() {
        let output_schema = json!({
            "type": "object",
            "properties": {
                "results": { "type": "array" }
            }
        })
        .as_object()
        .expect("output schema is an object")
        .clone();
        let factory = create_provider_defined_tool_factory_with_output_schema(
            "provider.web_search",
            object_schema(),
            output_schema.clone(),
        );

        assert_eq!(
            serde_json::to_value(&factory).expect("factory serializes"),
            json!({
                "id": "provider.web_search",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "results": { "type": "array" }
                    }
                }
            })
        );

        let deserialized: ProviderDefinedToolFactory = serde_json::from_value(json!({
            "id": "provider.web_search",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                },
                "required": ["city"]
            },
            "outputSchema": {
                "type": "object",
                "properties": {
                    "results": { "type": "array" }
                }
            }
        }))
        .expect("factory deserializes");

        assert_eq!(
            deserialized,
            ProviderDefinedToolFactory::new("provider.web_search", object_schema())
                .with_output_schema(output_schema)
        );
    }

    #[test]
    fn provider_defined_tool_factory_creates_provider_tool() {
        let args = json!({ "maxResults": 3 })
            .as_object()
            .expect("args are an object")
            .clone();
        let output_schema = json!({ "type": "string" })
            .as_object()
            .expect("output schema is an object")
            .clone();
        let tool = create_provider_defined_tool_factory_with_output_schema(
            "provider.web_search",
            object_schema(),
            output_schema.clone(),
        )
        .tool("webSearch", args.clone());

        assert!(tool.is_provider_tool());
        assert!(!tool.is_provider_executed());
        assert_eq!(tool.provider_tool_id(), Some("provider.web_search"));
        assert_eq!(tool.provider_tool_args(), Some(&args));
        assert_eq!(tool.output_schema(), Some(&output_schema));
        assert_eq!(
            tool.to_language_model_tool(),
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "provider.web_search",
                "webSearch",
                args
            ))
        );
    }

    #[test]
    fn provider_executed_tool_factory_round_trips_upstream_config_and_creates_tool() {
        let output_schema = json!({
            "type": "object",
            "properties": {
                "result": { "type": "string" }
            }
        })
        .as_object()
        .expect("output schema is an object")
        .clone();
        let args = json!({ "region": "au" })
            .as_object()
            .expect("args are an object")
            .clone();
        let factory = create_provider_executed_tool_factory(
            "provider.code_interpreter",
            object_schema(),
            output_schema.clone(),
        )
        .with_supports_deferred_results(true);

        assert_eq!(
            serde_json::to_value(&factory).expect("factory serializes"),
            json!({
                "id": "provider.code_interpreter",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                },
                "outputSchema": {
                    "type": "object",
                    "properties": {
                        "result": { "type": "string" }
                    }
                },
                "supportsDeferredResults": true
            })
        );

        let deserialized: ProviderExecutedToolFactory = serde_json::from_value(json!({
            "id": "provider.code_interpreter",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                },
                "required": ["city"]
            },
            "outputSchema": {
                "type": "object",
                "properties": {
                    "result": { "type": "string" }
                }
            },
            "supportsDeferredResults": true
        }))
        .expect("factory deserializes");

        assert_eq!(deserialized, factory);

        let tool = factory.tool("codeInterpreter", args.clone());

        assert!(tool.is_provider_tool());
        assert!(tool.is_provider_executed());
        assert_eq!(tool.provider_tool_id(), Some("provider.code_interpreter"));
        assert_eq!(tool.provider_tool_args(), Some(&args));
        assert_eq!(tool.output_schema(), Some(&output_schema));
        assert_eq!(tool.supports_deferred_results(), Some(true));
        assert_eq!(
            tool.to_language_model_tool(),
            LanguageModelTool::Provider(LanguageModelProviderTool::new(
                "provider.code_interpreter",
                "codeInterpreter",
                args
            ))
        );
    }

    #[test]
    fn provider_defined_tool_factory_omits_missing_output_schema() {
        let factory = create_provider_defined_tool_factory("provider.web_search", object_schema());

        assert_eq!(
            serde_json::to_value(&factory).expect("factory serializes"),
            json!({
                "id": "provider.web_search",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                }
            })
        );

        let deserialized: ProviderDefinedToolFactory = serde_json::from_value(json!({
            "id": "provider.web_search",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                },
                "required": ["city"]
            }
        }))
        .expect("factory deserializes");

        assert_eq!(deserialized, factory);
    }

    #[test]
    fn provider_executed_tool_factory_omits_missing_deferred_results_support() {
        let output_schema = json!({ "type": "object" })
            .as_object()
            .expect("output schema is an object")
            .clone();
        let factory = create_provider_executed_tool_factory(
            "provider.code_interpreter",
            object_schema(),
            output_schema,
        );

        assert_eq!(
            serde_json::to_value(&factory).expect("factory serializes"),
            json!({
                "id": "provider.code_interpreter",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "city": { "type": "string" }
                    },
                    "required": ["city"]
                },
                "outputSchema": {
                    "type": "object"
                }
            })
        );

        let deserialized: ProviderExecutedToolFactory = serde_json::from_value(json!({
            "id": "provider.code_interpreter",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                },
                "required": ["city"]
            },
            "outputSchema": {
                "type": "object"
            }
        }))
        .expect("factory deserializes");

        assert_eq!(deserialized, factory);
    }

    #[test]
    fn prepare_tools_returns_none_for_empty_tool_sets() {
        assert_eq!(prepare_tools(Vec::<Tool>::new().iter()), None);
    }

    #[test]
    fn prepare_tools_converts_high_level_tools() {
        let provider_tool_args = json!({ "key": "value" })
            .as_object()
            .expect("args are an object")
            .clone();
        let tools = vec![
            Tool::new("weather", object_schema()),
            dynamic_tool("runtimeWeather", object_schema()),
            Tool::provider_defined(
                "providerTool",
                "provider.tool-id",
                provider_tool_args.clone(),
                object_schema(),
            ),
        ];

        assert_eq!(
            prepare_tools(&tools),
            Some(vec![
                LanguageModelTool::Function(LanguageModelFunctionTool::new(
                    "weather",
                    object_schema()
                )),
                LanguageModelTool::Function(LanguageModelFunctionTool::new(
                    "runtimeWeather",
                    object_schema()
                )),
                LanguageModelTool::Provider(LanguageModelProviderTool::new(
                    "provider.tool-id",
                    "providerTool",
                    provider_tool_args
                ))
            ])
        );
    }

    #[test]
    fn tool_execution_options_serialize_as_camel_case() {
        let options = ToolExecutionOptions::new(
            "call-1",
            vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
                vec![LanguageModelUserContentPart::Text(
                    LanguageModelTextPart::new("Weather?"),
                )],
            ))],
        )
        .with_context(json!({
            "apiKey": "secret"
        }));

        assert_eq!(
            serde_json::to_value(options).expect("execution options serialize"),
            json!({
                "toolCallId": "call-1",
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": "Weather?"
                            }
                        ]
                    }
                ],
                "context": {
                    "apiKey": "secret"
                }
            })
        );
    }

    #[test]
    fn tool_model_output_options_round_trip_upstream_shape() {
        let options = ToolModelOutputOptions::new(
            "call-1",
            json!({ "city": "Brisbane" }),
            json!({ "forecast": "sunny" }),
        );

        assert_eq!(
            serde_json::to_value(&options).expect("model output options serialize"),
            json!({
                "toolCallId": "call-1",
                "input": {
                    "city": "Brisbane"
                },
                "output": {
                    "forecast": "sunny"
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<ToolModelOutputOptions>(json!({
                "toolCallId": "call-2",
                "input": {
                    "query": "weather"
                },
                "output": "sunny"
            }))
            .expect("model output options deserialize"),
            ToolModelOutputOptions::new("call-2", json!({ "query": "weather" }), json!("sunny"))
        );
    }

    #[test]
    fn tool_model_output_callback_receives_tool_result_context() {
        let tool =
            Tool::new("weather", object_schema()).with_to_model_output(|options| async move {
                LanguageModelToolResultOutput::json(json!({
                    "call": options.tool_call_id,
                    "city": options.input["city"],
                    "forecast": options.output["forecast"]
                }))
            });

        assert!(tool.has_to_model_output());

        let output = poll_ready(
            tool.model_output(ToolModelOutputOptions::new(
                "call-1",
                json!({ "city": "Brisbane" }),
                json!({ "forecast": "sunny" }),
            ))
            .expect("callback is configured"),
        );

        assert_eq!(
            output,
            LanguageModelToolResultOutput::json(json!({
                "call": "call-1",
                "city": "Brisbane",
                "forecast": "sunny"
            }))
        );
    }

    #[test]
    fn tool_to_model_output_accepts_untyped_output_without_execute() {
        let tool =
            Tool::new("weather", object_schema()).with_to_model_output(|options| async move {
                LanguageModelToolResultOutput::json(json!({
                    "toolCallId": options.tool_call_id,
                    "input": options.input,
                    "output": options.output
                }))
            });

        assert!(!tool.is_executable());
        assert_eq!(tool.output_schema(), None);

        let output = poll_ready(
            tool.model_output(ToolModelOutputOptions::new(
                "call-input-only",
                json!({ "number": 7 }),
                json!({ "raw": true }),
            ))
            .expect("callback is configured"),
        );

        assert_eq!(
            output,
            LanguageModelToolResultOutput::json(json!({
                "toolCallId": "call-input-only",
                "input": { "number": 7 },
                "output": { "raw": true }
            }))
        );
    }

    #[test]
    fn tool_to_model_output_accepts_execute_function_output() {
        let tool = Tool::new("weather", object_schema())
            .with_execute(|input, _options| async move {
                Ok(json!({
                    "number": input["number"],
                    "status": "executed"
                }))
            })
            .with_to_model_output(|options| async move {
                LanguageModelToolResultOutput::json(json!({
                    "toolCallId": options.tool_call_id,
                    "input": options.input,
                    "output": options.output
                }))
            });

        assert!(tool.is_executable());
        let executed = poll_ready(
            tool.execute(
                json!({ "number": 7 }),
                ToolExecutionOptions::new("call-execute", Vec::new()),
            )
            .expect("execute callback is configured"),
        )
        .expect("execute succeeds");

        let output = poll_ready(
            tool.model_output(ToolModelOutputOptions::new(
                "call-execute",
                json!({ "number": 7 }),
                executed,
            ))
            .expect("model output callback is configured"),
        );

        assert_eq!(
            output,
            LanguageModelToolResultOutput::json(json!({
                "toolCallId": "call-execute",
                "input": { "number": 7 },
                "output": {
                    "number": 7,
                    "status": "executed"
                }
            }))
        );
    }

    #[test]
    fn tool_to_model_output_accepts_output_schema_result_output() {
        let output_schema = json!({
            "type": "string",
            "enum": ["test"]
        })
        .as_object()
        .expect("output schema is an object")
        .clone();
        let tool = Tool::new("weather", object_schema())
            .with_output_schema(output_schema.clone())
            .with_to_model_output(|options| async move {
                LanguageModelToolResultOutput::text(format!(
                    "{}:{}",
                    options.tool_call_id, options.output
                ))
            });

        assert_eq!(tool.output_schema(), Some(&output_schema));

        let output = poll_ready(
            tool.model_output(ToolModelOutputOptions::new(
                "call-schema",
                json!({ "number": 7 }),
                json!("test"),
            ))
            .expect("model output callback is configured"),
        );

        assert_eq!(
            output,
            LanguageModelToolResultOutput::text(r#"call-schema:"test""#)
        );
    }

    #[test]
    fn sandbox_command_contracts_round_trip_upstream_shape() {
        let options = SandboxCommandOptions::new("pwd").with_working_directory("/workspace");

        assert_eq!(
            serde_json::to_value(&options).expect("command options serialize"),
            json!({
                "command": "pwd",
                "workingDirectory": "/workspace"
            })
        );
        assert_eq!(
            serde_json::from_value::<SandboxCommandOptions>(json!({
                "command": "pwd",
                "workingDirectory": "/workspace"
            }))
            .expect("command options deserialize"),
            options
        );

        let result = SandboxCommandResult::new(2)
            .with_stdout("out")
            .with_stderr("err");

        assert_eq!(
            serde_json::to_value(&result).expect("command result serializes"),
            json!({
                "exitCode": 2,
                "stdout": "out",
                "stderr": "err"
            })
        );
        assert_eq!(
            serde_json::from_value::<SandboxCommandResult>(json!({
                "exitCode": 2,
                "stdout": "out",
                "stderr": "err"
            }))
            .expect("command result deserializes"),
            result
        );
    }

    #[test]
    fn sandbox_command_options_include_abort_signal_without_serializing_it() {
        let abort_controller = LanguageModelAbortController::new();
        let options =
            SandboxCommandOptions::new("pwd").with_abort_signal(abort_controller.signal());

        assert!(
            !options
                .abort_signal
                .as_ref()
                .expect("abort signal is set")
                .is_aborted()
        );
        abort_controller.abort_with_reason(json!("stop"));
        assert!(
            options
                .abort_signal
                .as_ref()
                .expect("abort signal is set")
                .is_aborted()
        );
        assert_eq!(
            serde_json::to_value(&options).expect("command options serialize"),
            json!({
                "command": "pwd"
            })
        );

        let round_tripped: SandboxCommandOptions = serde_json::from_value(json!({
            "command": "pwd",
            "workingDirectory": "/workspace"
        }))
        .expect("command options deserialize");

        assert_eq!(round_tripped.command, "pwd");
        assert_eq!(
            round_tripped.working_directory,
            Some("/workspace".to_string())
        );
        assert!(round_tripped.abort_signal.is_none());
    }

    #[test]
    fn tool_execution_options_carry_runtime_sandbox_without_serializing_it() {
        let sandbox: Arc<dyn ExperimentalSandbox> =
            Arc::new(StaticSandbox::new("workspace sandbox"));
        let options = ToolExecutionOptions::new("call-1", Vec::new())
            .with_experimental_sandbox(Arc::clone(&sandbox));

        assert_eq!(
            options
                .experimental_sandbox
                .as_ref()
                .expect("sandbox is present")
                .description(),
            "workspace sandbox"
        );
        assert_eq!(
            poll_ready(
                options
                    .experimental_sandbox
                    .as_ref()
                    .expect("sandbox is present")
                    .run_command(SandboxCommandOptions::new("echo hi"))
            ),
            SandboxCommandResult::new(0).with_stdout("echo hi")
        );
        assert_eq!(
            serde_json::to_value(options).expect("execution options serialize"),
            json!({
                "toolCallId": "call-1",
                "messages": []
            })
        );
    }

    #[test]
    fn tool_execution_options_include_execution_metadata_context_abort_signal_and_sandbox() {
        let abort_controller = LanguageModelAbortController::new();
        let sandbox: Arc<dyn ExperimentalSandbox> =
            Arc::new(StaticSandbox::new("workspace sandbox"));
        let messages = vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![LanguageModelUserContentPart::Text(
                LanguageModelTextPart::new("Weather?"),
            )],
        ))];
        let options = ToolExecutionOptions::new("call-1", messages.clone())
            .with_context(json!({ "requestId": "req-1" }))
            .with_abort_signal(abort_controller.signal())
            .with_experimental_sandbox(Arc::clone(&sandbox));

        assert_eq!(options.tool_call_id, "call-1");
        assert_eq!(options.messages, messages);
        assert_eq!(options.context, Some(json!({ "requestId": "req-1" })));
        assert!(
            !options
                .abort_signal
                .as_ref()
                .expect("abort signal is set")
                .is_aborted()
        );
        abort_controller.abort();
        assert!(
            options
                .abort_signal
                .as_ref()
                .expect("abort signal is set")
                .is_aborted()
        );
        assert_eq!(
            options
                .experimental_sandbox
                .as_ref()
                .expect("sandbox is set")
                .description(),
            "workspace sandbox"
        );
        assert_eq!(
            serde_json::to_value(&options).expect("execution options serialize"),
            json!({
                "toolCallId": "call-1",
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": "Weather?"
                            }
                        ]
                    }
                ],
                "context": {
                    "requestId": "req-1"
                }
            })
        );

        let round_tripped: ToolExecutionOptions = serde_json::from_value(json!({
            "toolCallId": "call-2",
            "messages": [],
            "context": {
                "requestId": "req-2"
            }
        }))
        .expect("execution options deserialize");

        assert_eq!(round_tripped.tool_call_id, "call-2");
        assert_eq!(round_tripped.context, Some(json!({ "requestId": "req-2" })));
        assert!(round_tripped.abort_signal.is_none());
        assert!(round_tripped.experimental_sandbox.is_none());
    }

    #[test]
    fn tool_input_lifecycle_callbacks_receive_upstream_execution_options() {
        let recorded = Arc::new(Mutex::new(Vec::<JsonValue>::new()));
        let start_recorded = Arc::clone(&recorded);
        let delta_recorded = Arc::clone(&recorded);
        let available_recorded = Arc::clone(&recorded);
        let abort_controller = LanguageModelAbortController::new();
        let sandbox: Arc<dyn ExperimentalSandbox> =
            Arc::new(StaticSandbox::new("workspace sandbox"));
        let messages = vec![LanguageModelMessage::User(LanguageModelUserMessage::new(
            vec![LanguageModelUserContentPart::Text(
                LanguageModelTextPart::new("Call the tool"),
            )],
        ))];

        let tool = Tool::new("weather", object_schema())
            .with_on_input_start(move |options| {
                let recorded = Arc::clone(&start_recorded);
                async move {
                    recorded.lock().expect("recorded lock").push(json!({
                        "type": "onInputStart",
                        "toolCallId": options.tool_call_id,
                        "context": options.context,
                        "messages": options.messages,
                        "abortSignalSet": options.abort_signal.is_some(),
                        "sandbox": options
                            .experimental_sandbox
                            .as_ref()
                            .map(|sandbox| sandbox.description().to_string())
                    }));
                }
            })
            .with_on_input_delta(move |options| {
                let recorded = Arc::clone(&delta_recorded);
                async move {
                    recorded.lock().expect("recorded lock").push(json!({
                        "type": "onInputDelta",
                        "toolCallId": options.tool_call_id,
                        "inputTextDelta": options.input_text_delta,
                        "context": options.context,
                        "messages": options.messages,
                        "abortSignalSet": options.abort_signal.is_some(),
                        "sandbox": options
                            .experimental_sandbox
                            .as_ref()
                            .map(|sandbox| sandbox.description().to_string())
                    }));
                }
            })
            .with_on_input_available(move |options| {
                let recorded = Arc::clone(&available_recorded);
                async move {
                    recorded.lock().expect("recorded lock").push(json!({
                        "type": "onInputAvailable",
                        "toolCallId": options.tool_call_id,
                        "input": options.input,
                        "context": options.context,
                        "messages": options.messages,
                        "abortSignalSet": options.abort_signal.is_some(),
                        "sandbox": options
                            .experimental_sandbox
                            .as_ref()
                            .map(|sandbox| sandbox.description().to_string())
                    }));
                }
            });

        assert!(tool.has_on_input_start());
        assert!(tool.has_on_input_delta());
        assert!(tool.has_on_input_available());

        poll_ready(
            tool.on_input_start(
                ToolExecutionOptions::new("call-1", messages.clone())
                    .with_context(json!({ "requestId": "req-1" }))
                    .with_abort_signal(abort_controller.signal())
                    .with_experimental_sandbox(Arc::clone(&sandbox)),
            )
            .expect("input start callback is configured"),
        );
        poll_ready(
            tool.on_input_delta(
                ToolInputDeltaOptions::new("call-1", messages.clone(), r#"{"city":""#)
                    .with_context(json!({ "requestId": "req-1" }))
                    .with_abort_signal(abort_controller.signal())
                    .with_experimental_sandbox(Arc::clone(&sandbox)),
            )
            .expect("input delta callback is configured"),
        );
        poll_ready(
            tool.on_input_available(
                ToolInputAvailableOptions::new("call-1", messages, json!({ "city": "Brisbane" }))
                    .with_context(json!({ "requestId": "req-1" }))
                    .with_abort_signal(abort_controller.signal())
                    .with_experimental_sandbox(sandbox),
            )
            .expect("input available callback is configured"),
        );

        assert_eq!(
            recorded.lock().expect("recorded lock").as_slice(),
            [
                json!({
                    "type": "onInputStart",
                    "toolCallId": "call-1",
                    "context": { "requestId": "req-1" },
                    "messages": [
                        {
                            "role": "user",
                            "content": [
                                {
                                    "type": "text",
                                    "text": "Call the tool"
                                }
                            ]
                        }
                    ],
                    "abortSignalSet": true,
                    "sandbox": "workspace sandbox"
                }),
                json!({
                    "type": "onInputDelta",
                    "toolCallId": "call-1",
                    "inputTextDelta": r#"{"city":""#,
                    "context": { "requestId": "req-1" },
                    "messages": [
                        {
                            "role": "user",
                            "content": [
                                {
                                    "type": "text",
                                    "text": "Call the tool"
                                }
                            ]
                        }
                    ],
                    "abortSignalSet": true,
                    "sandbox": "workspace sandbox"
                }),
                json!({
                    "type": "onInputAvailable",
                    "toolCallId": "call-1",
                    "input": { "city": "Brisbane" },
                    "context": { "requestId": "req-1" },
                    "messages": [
                        {
                            "role": "user",
                            "content": [
                                {
                                    "type": "text",
                                    "text": "Call the tool"
                                }
                            ]
                        }
                    ],
                    "abortSignalSet": true,
                    "sandbox": "workspace sandbox"
                })
            ]
        );
    }

    #[test]
    fn tool_execute_function_accepts_input_output_and_execution_options() {
        let execute: Arc<ToolExecuteFunction> = Arc::new(|input, options| {
            Box::pin(ready(Ok(json!({
                "city": input["city"],
                "toolCallId": options.tool_call_id,
                "context": options.context
            }))))
        });

        let output = poll_ready(execute.as_ref()(
            json!({
                "city": "Brisbane"
            }),
            ToolExecutionOptions::new("call-1", Vec::new())
                .with_context(json!({ "requestId": "req-1" })),
        ))
        .expect("tool execution succeeds");

        assert_eq!(
            output,
            json!({
                "city": "Brisbane",
                "toolCallId": "call-1",
                "context": {
                    "requestId": "req-1"
                }
            })
        );
    }

    #[test]
    fn tool_needs_approval_function_accepts_input_options_and_returns_boolean() {
        let needs_approval: Arc<ToolNeedsApprovalFunction> = Arc::new(|input, options| {
            Box::pin(ready(
                input["city"] == json!("Brisbane")
                    && options.context == Some(json!({ "requestId": "req-1" })),
            ))
        });

        let approved = poll_ready(needs_approval.as_ref()(
            json!({
                "city": "Brisbane"
            }),
            ToolNeedsApprovalOptions::new("call-1", Vec::new())
                .with_context(json!({ "requestId": "req-1" })),
        ));

        assert!(approved);
    }

    #[test]
    fn is_executable_tool_matches_upstream_helper_behavior() {
        let executable = Tool::new("weather", object_schema()).with_execute(|_input, _options| {
            ready(Ok(json!({
                "forecast": "sunny"
            })))
        });
        let non_executable = Tool::new("lookup", object_schema());

        assert!(is_executable_tool(Some(&executable)));
        assert!(!is_executable_tool(Some(&non_executable)));
        assert!(!is_executable_tool(None));
    }

    #[test]
    fn is_executable_tool_upstream_returns_true_for_tools_with_an_execute_function() {
        let weather_tool = tool("weather", object_schema())
            .with_execute(|_input, _options| ready(Ok(json!("sunny"))));

        assert!(is_executable_tool(Some(&weather_tool)));
    }

    #[test]
    fn is_executable_tool_upstream_returns_false_for_tools_without_an_execute_function() {
        let weather_tool = tool("weather", object_schema());

        assert!(!is_executable_tool(Some(&weather_tool)));
    }

    #[test]
    fn is_executable_tool_upstream_returns_false_for_undefined() {
        assert!(!is_executable_tool(None));
    }

    #[test]
    fn is_executable_tool_upstream_allows_executable_tools_to_be_passed_to_execute_tool_after_narrowing()
     {
        let weather_tool = tool("weather", object_schema())
            .with_context_schema(json_schema(
                json!({
                    "type": "object",
                    "properties": {
                        "requestId": { "type": "string" }
                    },
                    "required": ["requestId"]
                })
                .as_object()
                .expect("context schema is an object")
                .clone(),
            ))
            .with_execute(|input, options| {
                ready(Ok(json!({
                    "city": input["city"],
                    "requestId": options
                        .context
                        .as_ref()
                        .and_then(|context| context.get("requestId"))
                        .cloned()
                        .unwrap_or(JsonValue::Null)
                })))
            });

        assert!(is_executable_tool(Some(&weather_tool)));

        let results = poll_ready(execute_tool(
            &weather_tool,
            json!({ "city": "Berlin" }),
            ToolExecutionOptions::new("tool-call-1", Vec::new()).with_context(json!({
                "requestId": "req-1"
            })),
        ))
        .expect("tool execution succeeds");

        assert_eq!(
            results,
            vec![ExecuteToolOutput::final_output(json!({
                "city": "Berlin",
                "requestId": "req-1"
            }))]
        );
    }

    #[test]
    fn execute_tool_output_round_trips_upstream_shape() {
        let final_output = ExecuteToolOutput::final_output(json!({
            "forecast": "sunny"
        }));

        assert_eq!(
            serde_json::to_value(&final_output).expect("final output serializes"),
            json!({
                "type": "final",
                "output": {
                    "forecast": "sunny"
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<ExecuteToolOutput>(json!({
                "type": "preliminary",
                "output": "partial"
            }))
            .expect("preliminary output deserializes"),
            ExecuteToolOutput::preliminary(json!("partial"))
        );
        assert_eq!(final_output.output(), &json!({ "forecast": "sunny" }));
    }

    #[test]
    fn execute_tool_runs_executor_and_wraps_final_output() {
        let tool = Tool::new("weather", object_schema()).with_execute(|input, options| {
            ready(Ok(json!({
                "input": input,
                "toolCallId": options.tool_call_id
            })))
        });

        let outputs = poll_ready(execute_tool(
            &tool,
            json!({
                "city": "Brisbane"
            }),
            ToolExecutionOptions::new("call-1", Vec::new()),
        ))
        .expect("tool execution succeeds");

        assert_eq!(
            outputs,
            vec![ExecuteToolOutput::final_output(json!({
                "input": {
                    "city": "Brisbane"
                },
                "toolCallId": "call-1"
            }))]
        );
    }

    #[test]
    fn execute_tool_upstream_yields_a_single_final_output_for_non_streaming_tools() {
        let weather_tool = tool("weather", object_schema())
            .with_context_schema(json_schema(
                json!({
                    "type": "object",
                    "properties": {
                        "requestId": { "type": "string" }
                    },
                    "required": ["requestId"]
                })
                .as_object()
                .expect("context schema is an object")
                .clone(),
            ))
            .with_execute(|input, options| {
                ready(Ok(json!({
                    "city": input["city"],
                    "requestId": options
                        .context
                        .as_ref()
                        .and_then(|context| context.get("requestId"))
                        .cloned()
                        .unwrap_or(JsonValue::Null)
                })))
            });

        let results = poll_ready(execute_tool(
            &weather_tool,
            json!({ "city": "Berlin" }),
            ToolExecutionOptions::new("tool-call-1", Vec::new()).with_context(json!({
                "requestId": "req-1"
            })),
        ))
        .expect("tool execution succeeds");

        assert_eq!(
            results,
            vec![ExecuteToolOutput::final_output(json!({
                "city": "Berlin",
                "requestId": "req-1"
            }))]
        );
    }

    #[test]
    fn execute_tool_upstream_yields_streamed_values_as_preliminary_output_and_repeats_the_last_one_as_final()
     {
        let weather_tool = tool("weather", object_schema())
            .with_context_schema(json_schema(
                json!({
                    "type": "object",
                    "properties": {
                        "requestId": { "type": "string" }
                    },
                    "required": ["requestId"]
                })
                .as_object()
                .expect("context schema is an object")
                .clone(),
            ))
            .with_execute_outputs(|input, options| {
                ready(Ok(vec![
                    ExecuteToolOutput::preliminary(json!(format!(
                        "{}:{}:1",
                        input["city"].as_str().unwrap_or_default(),
                        options
                            .context
                            .as_ref()
                            .and_then(|context| context.get("requestId"))
                            .and_then(JsonValue::as_str)
                            .unwrap_or_default()
                    ))),
                    ExecuteToolOutput::preliminary(json!(format!(
                        "{}:{}:2",
                        input["city"].as_str().unwrap_or_default(),
                        options
                            .context
                            .as_ref()
                            .and_then(|context| context.get("requestId"))
                            .and_then(JsonValue::as_str)
                            .unwrap_or_default()
                    ))),
                ]))
            });

        let results = poll_ready(execute_tool(
            &weather_tool,
            json!({ "city": "Berlin" }),
            ToolExecutionOptions::new("tool-call-2", Vec::new()).with_context(json!({
                "requestId": "req-2"
            })),
        ))
        .expect("tool execution succeeds");

        assert_eq!(
            results,
            vec![
                ExecuteToolOutput::preliminary(json!("Berlin:req-2:1")),
                ExecuteToolOutput::preliminary(json!("Berlin:req-2:2")),
                ExecuteToolOutput::final_output(json!("Berlin:req-2:2")),
            ]
        );
    }

    #[test]
    fn execute_tool_reports_non_executable_tools() {
        let tool = Tool::new("weather", object_schema());

        let error = poll_ready(execute_tool(
            &tool,
            json!({ "city": "Brisbane" }),
            ToolExecutionOptions::new("call-1", Vec::new()),
        ))
        .expect_err("non-executable tools fail");

        assert_eq!(error.message(), "Tool is not executable.");
    }

    #[test]
    fn tool_executor_returns_json_results() {
        let tool = Tool::new("weather", object_schema()).with_execute(|input, options| {
            ready(Ok(json!({
                "input": input,
                "toolCallId": options.tool_call_id
            })))
        });

        assert!(tool.is_executable());

        let result = poll_ready(
            tool.execute(
                json!({
                    "city": "Brisbane"
                }),
                ToolExecutionOptions::new("call-1", Vec::new()),
            )
            .expect("tool has an executor"),
        )
        .expect("tool execution succeeds");

        assert_eq!(
            result,
            json!({
                "input": {
                    "city": "Brisbane"
                },
                "toolCallId": "call-1"
            })
        );
    }

    #[test]
    fn tool_execution_error_retains_message() {
        let error = ToolExecutionError::new("Tool failed.");

        assert_eq!(error.message(), "Tool failed.");
        assert_eq!(error.to_string(), "Tool failed.");
        assert_eq!(
            serde_json::to_value(error).expect("tool execution error serializes"),
            json!({
                "message": "Tool failed."
            })
        );
    }

    #[test]
    fn load_api_key_returns_explicit_value_without_reading_environment() {
        let api_key = load_api_key(
            LoadApiKeyOptions::new("AI_SDK_RUST_TEST_API_KEY", "Test Provider")
                .with_api_key("explicit-key"),
        )
        .expect("explicit API key loads");

        assert_eq!(api_key, "explicit-key");
    }

    #[test]
    fn load_api_key_reads_environment_when_value_is_missing() {
        let api_key = load_api_key_with_env(
            LoadApiKeyOptions::new("AI_SDK_RUST_TEST_API_KEY", "Test Provider"),
            |name| {
                assert_eq!(name, "AI_SDK_RUST_TEST_API_KEY");
                Ok("env-key".to_string())
            },
        )
        .expect("environment API key loads");

        assert_eq!(api_key, "env-key");
    }

    #[test]
    fn load_api_key_reports_upstream_missing_message() {
        let error = load_api_key_with_env(
            LoadApiKeyOptions::new("AI_SDK_RUST_TEST_API_KEY", "Test Provider")
                .with_api_key_parameter_name("token"),
            |_| Err(VarError::NotPresent),
        )
        .expect_err("missing API key is rejected");

        assert_eq!(
            error.to_string(),
            "Test Provider API key is missing. Pass it using the 'token' parameter or the AI_SDK_RUST_TEST_API_KEY environment variable."
        );
    }

    #[test]
    fn load_api_key_reports_non_unicode_environment_values_as_non_strings() {
        let error = load_api_key_with_env(
            LoadApiKeyOptions::new("AI_SDK_RUST_TEST_API_KEY", "Test Provider"),
            |_| Err(VarError::NotUnicode(OsString::from("not-unicode"))),
        )
        .expect_err("non-Unicode API key is rejected");

        assert_eq!(
            error.to_string(),
            "Test Provider API key must be a string. The value of the AI_SDK_RUST_TEST_API_KEY environment variable is not a string."
        );
    }

    #[test]
    fn load_setting_returns_explicit_value_without_reading_environment() {
        let setting = load_setting(
            LoadSettingOptions::new("AI_SDK_RUST_TEST_BASE_URL", "baseURL", "Test Provider")
                .with_setting_value("https://example.com"),
        )
        .expect("explicit setting loads");

        assert_eq!(setting, "https://example.com");
    }

    #[test]
    fn load_setting_reads_environment_when_value_is_missing() {
        let setting = load_setting_with_env(
            LoadSettingOptions::new("AI_SDK_RUST_TEST_BASE_URL", "baseURL", "Test Provider"),
            |name| {
                assert_eq!(name, "AI_SDK_RUST_TEST_BASE_URL");
                Ok("https://env.example.com".to_string())
            },
        )
        .expect("environment setting loads");

        assert_eq!(setting, "https://env.example.com");
    }

    #[test]
    fn load_setting_reports_upstream_missing_message() {
        let error = load_setting_with_env(
            LoadSettingOptions::new("AI_SDK_RUST_TEST_BASE_URL", "baseURL", "Test Provider"),
            |_| Err(VarError::NotPresent),
        )
        .expect_err("missing setting is rejected");

        assert_eq!(
            error.to_string(),
            "Test Provider setting is missing. Pass it using the 'baseURL' parameter or the AI_SDK_RUST_TEST_BASE_URL environment variable."
        );
    }

    #[test]
    fn load_setting_reports_non_unicode_environment_values_as_non_strings() {
        let error = load_setting_with_env(
            LoadSettingOptions::new("AI_SDK_RUST_TEST_BASE_URL", "baseURL", "Test Provider"),
            |_| Err(VarError::NotUnicode(OsString::from("not-unicode"))),
        )
        .expect_err("non-Unicode setting is rejected");

        assert_eq!(
            error.to_string(),
            "Test Provider setting must be a string. The value of the AI_SDK_RUST_TEST_BASE_URL environment variable is not a string."
        );
    }

    #[test]
    fn load_optional_setting_prefers_explicit_value() {
        let setting = load_optional_setting_with_env(
            LoadOptionalSettingOptions::new("AI_SDK_RUST_TEST_OPTIONAL")
                .with_setting_value("explicit"),
            |_| panic!("environment should not be read when explicit setting is present"),
        );

        assert_eq!(setting.as_deref(), Some("explicit"));
    }

    #[test]
    fn load_optional_setting_reads_environment_when_value_is_missing() {
        let setting = load_optional_setting_with_env(
            LoadOptionalSettingOptions::new("AI_SDK_RUST_TEST_OPTIONAL"),
            |_| Ok("env-setting".to_string()),
        );

        assert_eq!(setting.as_deref(), Some("env-setting"));
    }

    #[test]
    fn load_optional_setting_returns_none_for_missing_or_non_unicode_environment_values() {
        assert_eq!(
            load_optional_setting_with_env(
                LoadOptionalSettingOptions::new("AI_SDK_RUST_TEST_OPTIONAL"),
                |_| Err(VarError::NotPresent),
            ),
            None
        );

        assert_eq!(
            load_optional_setting_with_env(
                LoadOptionalSettingOptions::new("AI_SDK_RUST_TEST_OPTIONAL"),
                |_| Err(VarError::NotUnicode(OsString::from("not-unicode"))),
            ),
            None
        );
    }

    #[test]
    fn media_type_to_extension_maps_common_audio_media_types() {
        for (media_type, expected_extension) in [
            ("audio/mpeg", "mp3"),
            ("audio/mp3", "mp3"),
            ("audio/wav", "wav"),
            ("audio/x-wav", "wav"),
            ("audio/webm", "webm"),
            ("audio/ogg", "ogg"),
            ("audio/opus", "ogg"),
            ("audio/mp4", "m4a"),
            ("audio/x-m4a", "m4a"),
            ("audio/flac", "flac"),
            ("audio/aac", "aac"),
        ] {
            assert_eq!(
                media_type_to_extension(media_type),
                expected_extension,
                "{media_type} maps to {expected_extension}"
            );
        }
    }

    #[test]
    fn media_type_to_extension_lowercases_subtypes_and_handles_invalid_values() {
        assert_eq!(media_type_to_extension("AUDIO/MPEG"), "mp3");
        assert_eq!(media_type_to_extension("AUDIO/MP3"), "mp3");
        assert_eq!(media_type_to_extension("nope"), "");
    }

    #[test]
    fn media_type_to_extension_maps_audio_mpeg_to_mp3() {
        assert_eq!(media_type_to_extension("audio/mpeg"), "mp3");
    }

    #[test]
    fn media_type_to_extension_maps_audio_mp3_to_mp3() {
        assert_eq!(media_type_to_extension("audio/mp3"), "mp3");
    }

    #[test]
    fn media_type_to_extension_maps_audio_wav_to_wav() {
        assert_eq!(media_type_to_extension("audio/wav"), "wav");
    }

    #[test]
    fn media_type_to_extension_maps_audio_x_wav_to_wav() {
        assert_eq!(media_type_to_extension("audio/x-wav"), "wav");
    }

    #[test]
    fn media_type_to_extension_maps_audio_webm_to_webm() {
        assert_eq!(media_type_to_extension("audio/webm"), "webm");
    }

    #[test]
    fn media_type_to_extension_maps_audio_ogg_to_ogg() {
        assert_eq!(media_type_to_extension("audio/ogg"), "ogg");
    }

    #[test]
    fn media_type_to_extension_maps_audio_opus_to_ogg() {
        assert_eq!(media_type_to_extension("audio/opus"), "ogg");
    }

    #[test]
    fn media_type_to_extension_maps_audio_mp4_to_m4a() {
        assert_eq!(media_type_to_extension("audio/mp4"), "m4a");
    }

    #[test]
    fn media_type_to_extension_maps_audio_x_m4a_to_m4a() {
        assert_eq!(media_type_to_extension("audio/x-m4a"), "m4a");
    }

    #[test]
    fn media_type_to_extension_maps_audio_flac_to_flac() {
        assert_eq!(media_type_to_extension("audio/flac"), "flac");
    }

    #[test]
    fn media_type_to_extension_maps_audio_aac_to_aac() {
        assert_eq!(media_type_to_extension("audio/aac"), "aac");
    }

    #[test]
    fn media_type_to_extension_maps_uppercase_audio_mpeg_to_mp3() {
        assert_eq!(media_type_to_extension("AUDIO/MPEG"), "mp3");
    }

    #[test]
    fn media_type_to_extension_maps_uppercase_audio_mp3_to_mp3() {
        assert_eq!(media_type_to_extension("AUDIO/MP3"), "mp3");
    }

    #[test]
    fn media_type_to_extension_maps_invalid_media_type_to_empty_string() {
        assert_eq!(media_type_to_extension("nope"), "");
    }

    #[test]
    fn strip_file_extension_upstream_strips_extension_from_filename() {
        assert_eq!(strip_file_extension("report.pdf"), "report");
    }

    #[test]
    fn strip_file_extension_upstream_returns_input_when_there_is_no_extension() {
        assert_eq!(strip_file_extension("report"), "report");
    }

    #[test]
    fn strip_file_extension_upstream_strips_all_extension_segments_for_multi_dot_filenames() {
        assert_eq!(strip_file_extension("archive.tar.gz"), "archive");
    }

    #[test]
    fn strip_file_extension_upstream_strips_a_trailing_dot() {
        assert_eq!(strip_file_extension("report."), "report");
    }

    #[test]
    fn without_trailing_slash_removes_one_trailing_slash() {
        assert_eq!(
            without_trailing_slash(Some("https://api.example.com/")),
            Some("https://api.example.com")
        );
    }

    #[test]
    fn without_trailing_slash_preserves_values_without_trailing_slash() {
        assert_eq!(
            without_trailing_slash(Some("https://api.example.com/v1")),
            Some("https://api.example.com/v1")
        );
    }

    #[test]
    fn without_trailing_slash_preserves_missing_url() {
        assert_eq!(without_trailing_slash(None), None);
    }

    #[test]
    fn without_trailing_slash_only_removes_the_final_slash() {
        assert_eq!(
            without_trailing_slash(Some("https://api.example.com//")),
            Some("https://api.example.com/")
        );
    }

    #[test]
    fn resolve_provider_reference_returns_provider_specific_identifier() {
        let reference = ProviderReference::try_from(BTreeMap::from([
            ("anthropic".to_string(), "file-xyz".to_string()),
            ("openai".to_string(), "file-abc".to_string()),
        ]))
        .expect("provider reference is valid");

        assert_eq!(
            resolve_provider_reference(&reference, "openai").expect("openai reference is present"),
            "file-abc"
        );
        assert_eq!(
            resolve_provider_reference(&reference, "anthropic")
                .expect("anthropic reference is present"),
            "file-xyz"
        );
    }

    #[test]
    fn resolve_provider_reference_reports_missing_provider_context() {
        let reference = ProviderReference::try_from(BTreeMap::from([(
            "anthropic".to_string(),
            "file-xyz".to_string(),
        )]))
        .expect("provider reference is valid");

        let error = resolve_provider_reference(&reference, "openai")
            .expect_err("missing provider reference is rejected");

        assert_eq!(error.provider(), "openai");
        assert_eq!(error.reference(), &reference);
    }

    #[test]
    fn resolve_provider_reference_rejects_empty_references() {
        let reference =
            ProviderReference::try_from(BTreeMap::new()).expect("empty reference is valid");

        let error = resolve_provider_reference(&reference, "openai")
            .expect_err("empty reference cannot satisfy provider lookup");

        assert_eq!(error.provider(), "openai");
        assert_eq!(error.reference(), &reference);
    }

    #[test]
    fn resolve_provider_reference_upstream_returns_identifier_when_provider_key_exists() {
        let reference = ProviderReference::try_from(BTreeMap::from([
            ("anthropic".to_string(), "file-xyz".to_string()),
            ("openai".to_string(), "file-abc".to_string()),
        ]))
        .expect("provider reference is valid");

        assert_eq!(
            resolve_provider_reference(&reference, "openai").expect("openai reference is present"),
            "file-abc"
        );
    }

    #[test]
    fn resolve_provider_reference_upstream_returns_correct_identifier_for_different_provider() {
        let reference = ProviderReference::try_from(BTreeMap::from([
            ("anthropic".to_string(), "file-xyz".to_string()),
            ("openai".to_string(), "file-abc".to_string()),
        ]))
        .expect("provider reference is valid");

        assert_eq!(
            resolve_provider_reference(&reference, "anthropic")
                .expect("anthropic reference is present"),
            "file-xyz"
        );
    }

    #[test]
    fn resolve_provider_reference_upstream_throws_when_no_entry_exists_for_provider() {
        let reference = ProviderReference::try_from(BTreeMap::from([
            ("anthropic".to_string(), "file-xyz".to_string()),
            ("google".to_string(), "file-123".to_string()),
        ]))
        .expect("provider reference is valid");

        let error = resolve_provider_reference(&reference, "openai")
            .expect_err("missing provider reference is rejected");

        assert_eq!(error.provider(), "openai");
        assert_eq!(error.reference(), &reference);
    }

    #[test]
    fn resolve_provider_reference_upstream_throws_when_reference_is_empty() {
        let reference =
            ProviderReference::try_from(BTreeMap::new()).expect("empty reference is valid");

        let error = resolve_provider_reference(&reference, "openai")
            .expect_err("empty reference cannot satisfy provider lookup");

        assert_eq!(error.provider(), "openai");
        assert_eq!(error.reference(), &reference);
    }

    #[test]
    fn resolve_provider_reference_upstream_works_with_single_provider_reference() {
        let reference = ProviderReference::try_from(BTreeMap::from([(
            "openai".to_string(),
            "file-only".to_string(),
        )]))
        .expect("provider reference is valid");

        assert_eq!(
            resolve_provider_reference(&reference, "openai").expect("openai reference is present"),
            "file-only"
        );
    }
}
