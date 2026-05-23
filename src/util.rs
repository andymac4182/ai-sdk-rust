use std::fmt;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::task::{Context, Poll};
use std::thread;
use std::time::Duration;

use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{LanguageModelAbortController, LanguageModelAbortSignal};
use crate::provider_utils::{
    ParseJsonResult, convert_base64_to_bytes, normalize_headers, safe_parse_json,
};
use crate::retry::{DEFAULT_MAX_RETRIES, RetryWithExponentialBackoffOptions};

/// Error returned when text cannot be extracted from a data URL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataUrlTextError {
    /// The URL did not include the media-type header and comma-separated data payload.
    InvalidFormat,

    /// The data payload was not valid base64.
    Decode,
}

impl std::fmt::Display for DataUrlTextError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFormat => formatter.write_str("Invalid data URL format"),
            Self::Decode => formatter.write_str("Error decoding data URL"),
        }
    }
}

impl std::error::Error for DataUrlTextError {}

/// Error returned when an array chunk size is invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitArrayError;

impl std::fmt::Display for SplitArrayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("chunkSize must be greater than 0")
    }
}

impl std::error::Error for SplitArrayError {}

/// Error returned when a high-level AI SDK utility receives an invalid argument.
#[derive(Clone, Debug, PartialEq)]
pub struct InvalidArgumentError {
    parameter: String,
    value: JsonValue,
    message: String,
}

impl InvalidArgumentError {
    /// Creates an invalid argument error with the upstream high-level message prefix.
    pub fn new(
        parameter: impl Into<String>,
        value: impl Into<JsonValue>,
        message: impl Into<String>,
    ) -> Self {
        let parameter = parameter.into();
        let message = format!(
            "Invalid argument for parameter {}: {}",
            parameter,
            message.into()
        );

        Self {
            parameter,
            value: value.into(),
            message,
        }
    }

    /// Returns the invalid parameter name.
    pub fn parameter(&self) -> &str {
        &self.parameter
    }

    /// Returns the invalid value supplied for the parameter.
    pub fn value(&self) -> &JsonValue {
        &self.value
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into the invalid parameter, value, and message.
    pub fn into_parts(self) -> (String, JsonValue, String) {
        (self.parameter, self.value, self.message)
    }
}

impl std::fmt::Display for InvalidArgumentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InvalidArgumentError {}

/// Options accepted by [`prepare_retries`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PrepareRetriesOptions {
    max_retries: Option<usize>,
}

impl PrepareRetriesOptions {
    /// Creates retry preparation options with no explicit retry count.
    pub const fn new() -> Self {
        Self { max_retries: None }
    }

    /// Sets the maximum number of retries after the first attempt.
    pub const fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = Some(max_retries);
        self
    }

    /// Returns the explicit maximum retry count, when present.
    pub const fn max_retries(&self) -> Option<usize> {
        self.max_retries
    }
}

/// Prepared retry settings returned by [`prepare_retries`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedRetries {
    max_retries: usize,
    retry_options: RetryWithExponentialBackoffOptions,
}

impl PreparedRetries {
    /// Returns the resolved maximum retry count.
    pub const fn max_retries(&self) -> usize {
        self.max_retries
    }

    /// Returns options for the high-level retry executor.
    pub const fn retry_options(&self) -> RetryWithExponentialBackoffOptions {
        self.retry_options
    }
}

/// Validates and prepares retry settings for high-level AI SDK calls.
///
/// Upstream validates JavaScript numbers at runtime. Rust accepts a typed
/// `usize`, so negative and fractional values are rejected at the type boundary.
pub fn prepare_retries(options: PrepareRetriesOptions) -> PreparedRetries {
    let max_retries = options.max_retries.unwrap_or(DEFAULT_MAX_RETRIES);

    PreparedRetries {
        max_retries,
        retry_options: RetryWithExponentialBackoffOptions::new().with_max_retries(max_retries),
    }
}

/// Result returned by a high-level callback utility.
pub type CallbackResult = Result<(), String>;

/// Future returned by a high-level callback utility.
pub type CallbackFuture<'a> = Pin<Box<dyn Future<Output = CallbackResult> + 'a>>;

/// Function signature accepted by [`Callback`].
pub type CallbackFunction<'a, Event> = dyn Fn(Event) -> CallbackFuture<'a> + 'a;

/// Upstream-style callback wrapper used by [`merge_callbacks`] and [`notify`].
#[derive(Clone)]
pub struct Callback<'a, Event> {
    callback: Rc<CallbackFunction<'a, Event>>,
}

impl<'a, Event> Callback<'a, Event> {
    /// Creates a callback whose future can resolve successfully or with an ignored error.
    pub fn new<F, Fut>(callback: F) -> Self
    where
        F: Fn(Event) -> Fut + 'a,
        Fut: Future<Output = CallbackResult> + 'a,
    {
        Self {
            callback: Rc::new(move |event| Box::pin(callback(event))),
        }
    }

    /// Creates an infallible callback.
    pub fn infallible<F, Fut>(callback: F) -> Self
    where
        F: Fn(Event) -> Fut + 'a,
        Fut: Future<Output = ()> + 'a,
    {
        Self::new(move |event| {
            let future = callback(event);
            async move {
                future.await;
                Ok(())
            }
        })
    }

    /// Runs the callback and returns its original result.
    pub fn run(&self, event: Event) -> CallbackFuture<'a> {
        (self.callback)(event)
    }

    fn settle(&self, event: Event) -> CallbackFuture<'a> {
        match catch_unwind(AssertUnwindSafe(|| self.run(event))) {
            Ok(future) => Box::pin(SettledCallbackFuture::new(future)),
            Err(_) => Box::pin(async { Ok(()) }),
        }
    }
}

impl<Event> fmt::Debug for Callback<'_, Event> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Callback").finish_non_exhaustive()
    }
}

struct SettledCallbackFuture<'a> {
    future: Option<CallbackFuture<'a>>,
}

impl<'a> SettledCallbackFuture<'a> {
    fn new(future: CallbackFuture<'a>) -> Self {
        Self {
            future: Some(future),
        }
    }
}

impl Unpin for SettledCallbackFuture<'_> {}

impl Future for SettledCallbackFuture<'_> {
    type Output = CallbackResult;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let Some(future) = this.future.as_mut() else {
            return Poll::Ready(Ok(()));
        };

        match catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(context))) {
            Ok(Poll::Ready(_)) | Err(_) => {
                this.future = None;
                Poll::Ready(Ok(()))
            }
            Ok(Poll::Pending) => Poll::Pending,
        }
    }
}

/// Future that waits for callbacks to settle while ignoring failures.
pub struct CallbackSettleFuture<'a> {
    futures: Vec<Option<CallbackFuture<'a>>>,
}

impl<'a> CallbackSettleFuture<'a> {
    fn new(futures: Vec<CallbackFuture<'a>>) -> Self {
        Self {
            futures: futures.into_iter().map(Some).collect(),
        }
    }
}

impl Unpin for CallbackSettleFuture<'_> {}

impl Future for CallbackSettleFuture<'_> {
    type Output = CallbackResult;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut pending = false;

        for future in &mut this.futures {
            let Some(callback_future) = future.as_mut() else {
                continue;
            };

            match catch_unwind(AssertUnwindSafe(|| callback_future.as_mut().poll(context))) {
                Ok(Poll::Ready(_)) | Err(_) => {
                    *future = None;
                }
                Ok(Poll::Pending) => {
                    pending = true;
                }
            }
        }

        if pending {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

/// Callback list accepted by [`notify`].
#[derive(Clone)]
pub struct NotifyCallbacks<'a, Event> {
    callbacks: Vec<Option<Callback<'a, Event>>>,
}

impl<'a, Event> NotifyCallbacks<'a, Event> {
    /// Creates an empty callback list.
    pub fn none() -> Self {
        Self {
            callbacks: Vec::new(),
        }
    }

    /// Creates a callback list with one optional callback.
    pub fn one(callback: Option<Callback<'a, Event>>) -> Self {
        Self {
            callbacks: vec![callback],
        }
    }

    /// Creates a callback list from callbacks that are all present.
    pub fn many(callbacks: impl IntoIterator<Item = Callback<'a, Event>>) -> Self {
        Self {
            callbacks: callbacks.into_iter().map(Some).collect(),
        }
    }

    /// Creates a callback list from optional callbacks.
    pub fn many_optional(callbacks: impl IntoIterator<Item = Option<Callback<'a, Event>>>) -> Self {
        Self {
            callbacks: callbacks.into_iter().collect(),
        }
    }
}

impl<Event> fmt::Debug for NotifyCallbacks<'_, Event> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NotifyCallbacks")
            .field("len", &self.callbacks.len())
            .finish()
    }
}

impl<'a, Event> From<Callback<'a, Event>> for NotifyCallbacks<'a, Event> {
    fn from(callback: Callback<'a, Event>) -> Self {
        Self::one(Some(callback))
    }
}

impl<'a, Event> From<Option<Callback<'a, Event>>> for NotifyCallbacks<'a, Event> {
    fn from(callback: Option<Callback<'a, Event>>) -> Self {
        Self::one(callback)
    }
}

impl<'a, Event> From<Vec<Callback<'a, Event>>> for NotifyCallbacks<'a, Event> {
    fn from(callbacks: Vec<Callback<'a, Event>>) -> Self {
        Self::many(callbacks)
    }
}

impl<'a, Event> From<Vec<Option<Callback<'a, Event>>>> for NotifyCallbacks<'a, Event> {
    fn from(callbacks: Vec<Option<Callback<'a, Event>>>) -> Self {
        Self::many_optional(callbacks)
    }
}

/// Future returned by [`notify`].
pub struct NotifyFuture<'a> {
    inner: CallbackSettleFuture<'a>,
}

impl Unpin for NotifyFuture<'_> {}

impl Future for NotifyFuture<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        match Pin::new(&mut self.get_mut().inner).poll(context) {
            Poll::Ready(_) => Poll::Ready(()),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Creates a callback that invokes all provided callbacks and waits for settlement.
///
/// Missing callbacks are skipped, and callback errors or panics are ignored.
pub fn merge_callbacks<'a, Event, I>(callbacks: I) -> Callback<'a, Event>
where
    Event: Clone + 'a,
    I: IntoIterator<Item = Option<Callback<'a, Event>>>,
{
    let callbacks = Rc::new(callbacks.into_iter().flatten().collect::<Vec<_>>());

    Callback::new(move |event: Event| {
        let futures = callbacks
            .iter()
            .map(|callback| callback.settle(event.clone()))
            .collect::<Vec<_>>();
        CallbackSettleFuture::new(futures)
    })
}

/// Notifies all callbacks with an event and waits for them to settle.
///
/// This mirrors upstream `notify`: callback arrays are supported, missing
/// callbacks are skipped, and callback errors do not break the caller.
pub fn notify<'a, Event>(
    event: Event,
    callbacks: impl Into<NotifyCallbacks<'a, Event>>,
) -> NotifyFuture<'a>
where
    Event: Clone + 'a,
{
    let futures = callbacks
        .into()
        .callbacks
        .into_iter()
        .flatten()
        .map(|callback| callback.settle(event.clone()))
        .collect::<Vec<_>>();

    NotifyFuture {
        inner: CallbackSettleFuture::new(futures),
    }
}

/// Error returned by a serial job.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerialJobError {
    message: String,
}

impl SerialJobError {
    /// Creates a serial job error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for SerialJobError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SerialJobError {}

/// Result returned by [`SerialJobExecutor`] jobs.
pub type SerialJobResult = Result<(), SerialJobError>;

struct SerialJob {
    job: Box<dyn FnOnce() -> SerialJobResult + Send + 'static>,
    completion: mpsc::Sender<SerialJobResult>,
}

/// Handle returned by [`SerialJobExecutor::run`].
pub struct SerialJobHandle {
    completion: mpsc::Receiver<SerialJobResult>,
}

impl SerialJobHandle {
    /// Waits until the queued job has completed.
    pub fn wait(self) -> SerialJobResult {
        self.completion
            .recv()
            .unwrap_or_else(|_| Err(SerialJobError::new("serial job executor stopped")))
    }
}

impl fmt::Debug for SerialJobHandle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SerialJobHandle")
            .finish_non_exhaustive()
    }
}

/// Executes submitted jobs one at a time in submission order.
///
/// Upstream uses promises and a queue. Rust uses one worker thread per
/// executor, preserving the same serialized ordering and per-job error result.
pub struct SerialJobExecutor {
    sender: Option<mpsc::Sender<SerialJob>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl SerialJobExecutor {
    /// Creates a serial job executor.
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel::<SerialJob>();
        let worker = thread::spawn(move || {
            for queued_job in receiver {
                let result = (queued_job.job)();
                let _ = queued_job.completion.send(result);
            }
        });

        Self {
            sender: Some(sender),
            worker: Some(worker),
        }
    }

    /// Queues a job for serialized execution.
    pub fn run<F>(&self, job: F) -> SerialJobHandle
    where
        F: FnOnce() -> SerialJobResult + Send + 'static,
    {
        let (completion, receiver) = mpsc::channel();
        let queued_job = SerialJob {
            job: Box::new(job),
            completion: completion.clone(),
        };

        let send_result = self.sender.as_ref().map(|sender| sender.send(queued_job));

        if !matches!(send_result, Some(Ok(()))) {
            let _ = completion.send(Err(SerialJobError::new("serial job executor stopped")));
        }

        SerialJobHandle {
            completion: receiver,
        }
    }
}

impl Default for SerialJobExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SerialJobExecutor {
    fn drop(&mut self) {
        drop(self.sender.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl fmt::Debug for SerialJobExecutor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SerialJobExecutor")
            .finish_non_exhaustive()
    }
}

/// Source accepted by [`merge_abort_signals`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AbortSignalSource {
    /// A caller-controlled abort signal.
    Signal(LanguageModelAbortSignal),

    /// A timeout in milliseconds.
    TimeoutMs(u64),
}

impl AbortSignalSource {
    /// Creates an abort source from a signal.
    pub fn signal(signal: LanguageModelAbortSignal) -> Self {
        Self::Signal(signal)
    }

    /// Creates an abort source from a timeout in milliseconds.
    pub const fn timeout_ms(timeout_ms: u64) -> Self {
        Self::TimeoutMs(timeout_ms)
    }
}

impl From<LanguageModelAbortSignal> for AbortSignalSource {
    fn from(signal: LanguageModelAbortSignal) -> Self {
        Self::signal(signal)
    }
}

impl From<u64> for AbortSignalSource {
    fn from(timeout_ms: u64) -> Self {
        Self::timeout_ms(timeout_ms)
    }
}

/// Options for [`set_abort_timeout`].
#[derive(Clone, Debug)]
pub struct AbortTimeoutOptions {
    abort_controller: Option<LanguageModelAbortController>,
    label: String,
    timeout_ms: Option<u64>,
}

impl AbortTimeoutOptions {
    /// Creates timeout options with a human-readable label.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            abort_controller: None,
            label: label.into(),
            timeout_ms: None,
        }
    }

    /// Sets the abort controller that will be aborted when the timeout elapses.
    pub fn with_abort_controller(mut self, abort_controller: LanguageModelAbortController) -> Self {
        self.abort_controller = Some(abort_controller);
        self
    }

    /// Sets the timeout in milliseconds.
    pub const fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }
}

/// Handle returned by [`set_abort_timeout`].
#[derive(Clone, Debug)]
pub struct AbortTimeoutHandle {
    cancelled: Arc<AtomicBool>,
}

impl AbortTimeoutHandle {
    /// Cancels the scheduled abort.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Returns whether this timeout has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// Schedules a timeout that aborts a controller with an upstream-shaped timeout reason.
pub fn set_abort_timeout(options: AbortTimeoutOptions) -> Option<AbortTimeoutHandle> {
    let abort_controller = options.abort_controller?;
    let timeout_ms = options.timeout_ms?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let thread_cancelled = Arc::clone(&cancelled);
    let reason = timeout_abort_reason(&options.label, timeout_ms);

    let _ = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(timeout_ms));
        if !thread_cancelled.load(Ordering::SeqCst) {
            abort_controller.abort_with_reason(reason);
        }
    });

    Some(AbortTimeoutHandle { cancelled })
}

/// Merges abort signals and timeout sources into one signal.
///
/// This mirrors upstream `mergeAbortSignals`: absent sources are ignored, an
/// empty input returns `None`, a single valid signal is returned unchanged, and
/// multiple valid sources abort the merged signal with the first source reason.
pub fn merge_abort_signals<I>(sources: I) -> Option<LanguageModelAbortSignal>
where
    I: IntoIterator<Item = Option<AbortSignalSource>>,
{
    let mut signals = Vec::new();

    for source in sources.into_iter().flatten() {
        match source {
            AbortSignalSource::Signal(signal) => signals.push(signal),
            AbortSignalSource::TimeoutMs(timeout_ms) => {
                let controller = LanguageModelAbortController::new();
                let signal = controller.signal();
                set_abort_timeout(
                    AbortTimeoutOptions::new("Abort signal")
                        .with_abort_controller(controller)
                        .with_timeout_ms(timeout_ms),
                );
                signals.push(signal);
            }
        }
    }

    match signals.len() {
        0 => None,
        1 => signals.pop(),
        _ => {
            let controller = LanguageModelAbortController::new();
            let merged = controller.signal();
            for signal in &signals {
                signal.aborts_signal(&merged);
            }
            Some(merged)
        }
    }
}

fn timeout_abort_reason(label: &str, timeout_ms: u64) -> JsonValue {
    serde_json::json!({
        "name": "TimeoutError",
        "message": format!("{label} timeout of {timeout_ms}ms exceeded"),
    })
}

/// State returned by [`parse_partial_json`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParsePartialJsonState {
    /// No JSON text was supplied.
    UndefinedInput,

    /// The supplied JSON text parsed without repair.
    SuccessfulParse,

    /// The supplied JSON text parsed after partial JSON repair.
    RepairedParse,

    /// The supplied JSON text could not be parsed, even after repair.
    FailedParse,
}

/// Result returned by [`parse_partial_json`].
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsePartialJsonResult {
    /// Parsed JSON value when parsing succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<JsonValue>,

    /// Parse state for the attempted partial JSON parse.
    state: ParsePartialJsonState,
}

impl ParsePartialJsonResult {
    /// Creates a partial JSON result with the supplied state and value.
    pub fn new(state: ParsePartialJsonState, value: Option<JsonValue>) -> Self {
        Self { value, state }
    }

    /// Creates an undefined-input result.
    pub fn undefined_input() -> Self {
        Self::new(ParsePartialJsonState::UndefinedInput, None)
    }

    /// Creates a successful parse result.
    pub fn successful_parse(value: impl Into<JsonValue>) -> Self {
        Self::new(ParsePartialJsonState::SuccessfulParse, Some(value.into()))
    }

    /// Creates a repaired parse result.
    pub fn repaired_parse(value: impl Into<JsonValue>) -> Self {
        Self::new(ParsePartialJsonState::RepairedParse, Some(value.into()))
    }

    /// Creates a failed parse result.
    pub fn failed_parse() -> Self {
        Self::new(ParsePartialJsonState::FailedParse, None)
    }

    /// Returns the parsed JSON value when parsing succeeded.
    pub fn value(&self) -> Option<&JsonValue> {
        self.value.as_ref()
    }

    /// Returns the parse state.
    pub fn state(&self) -> ParsePartialJsonState {
        self.state
    }

    /// Converts this result into its value and state.
    pub fn into_parts(self) -> (Option<JsonValue>, ParsePartialJsonState) {
        (self.value, self.state)
    }
}

/// Performs a deep-equal comparison of two JSON data values.
///
/// This mirrors upstream `packages/ai` `isDeepEqualData` for Rust's JSON
/// boundary. JavaScript-only cases such as dates, functions, and prototypes do
/// not apply to [`JsonValue`].
pub fn is_deep_equal_data(left: &JsonValue, right: &JsonValue) -> bool {
    left == right
}

/// Applies default headers without overwriting existing values.
///
/// This mirrors upstream `packages/ai` `prepareHeaders` for Rust header maps:
/// input header names are normalized case-insensitively, and default headers
/// only fill keys that are not already present.
pub fn prepare_headers(headers: Option<Headers>, default_headers: Headers) -> Headers {
    let mut response_headers = normalize_headers(
        headers.map(|headers| headers.into_iter().map(|(key, value)| (key, Some(value)))),
    );

    for (key, value) in default_headers {
        response_headers
            .entry(key.to_ascii_lowercase())
            .or_insert(value);
    }

    response_headers
}

/// Deeply merges two JSON object values.
///
/// This mirrors upstream `packages/ai` `mergeObjects` for Rust's JSON boundary:
/// override fields replace base fields, nested objects are merged recursively,
/// arrays and scalar values are replaced, and prototype-pollution keys from
/// override objects are ignored. JavaScript-only `undefined`, `Date`, and
/// `RegExp` values do not apply to [`JsonValue`].
pub fn merge_objects(base: Option<&JsonValue>, overrides: Option<&JsonValue>) -> Option<JsonValue> {
    match (base, overrides) {
        (None, None) => None,
        (Some(base), None) => Some(base.clone()),
        (None, Some(overrides)) => Some(overrides.clone()),
        (Some(JsonValue::Object(base)), Some(JsonValue::Object(overrides))) => {
            Some(JsonValue::Object(merge_json_object_maps(base, overrides)))
        }
        (_, Some(overrides)) => Some(overrides.clone()),
    }
}

fn merge_json_object_maps(base: &JsonObject, overrides: &JsonObject) -> JsonObject {
    let mut result = base.clone();

    for (key, override_value) in overrides {
        if is_prototype_pollution_key(key) {
            continue;
        }

        let merged_value = match (result.get(key), override_value) {
            (Some(JsonValue::Object(base_object)), JsonValue::Object(override_object)) => {
                JsonValue::Object(merge_json_object_maps(base_object, override_object))
            }
            _ => override_value.clone(),
        };

        result.insert(key.clone(), merged_value);
    }

    result
}

fn is_prototype_pollution_key(key: &str) -> bool {
    matches!(key, "__proto__" | "constructor" | "prototype")
}

/// Splits a slice into cloned chunks of the supplied size.
///
/// This mirrors upstream `packages/ai` `splitArray`: chunk sizes must be
/// greater than zero, an empty input returns an empty chunk list, and the final
/// chunk may be shorter than the requested chunk size.
pub fn split_array<T: Clone>(
    array: &[T],
    chunk_size: isize,
) -> Result<Vec<Vec<T>>, SplitArrayError> {
    if chunk_size <= 0 {
        return Err(SplitArrayError);
    }

    Ok(array
        .chunks(chunk_size as usize)
        .map(<[T]>::to_vec)
        .collect())
}

/// Calculates the cosine similarity between two vectors.
///
/// This mirrors upstream `packages/ai` `cosineSimilarity`: vectors must have
/// equal lengths, empty vectors return 0, and any zero-vector input returns 0.
pub fn cosine_similarity(vector1: &[f64], vector2: &[f64]) -> Result<f64, InvalidArgumentError> {
    if vector1.len() != vector2.len() {
        return Err(InvalidArgumentError::new(
            "vector1,vector2",
            serde_json::json!({
                "vector1Length": vector1.len(),
                "vector2Length": vector2.len(),
            }),
            "Vectors must have the same length",
        ));
    }

    if vector1.is_empty() {
        return Ok(0.0);
    }

    let mut magnitude_squared1 = 0.0;
    let mut magnitude_squared2 = 0.0;
    let mut dot_product = 0.0;

    for (value1, value2) in vector1.iter().zip(vector2) {
        magnitude_squared1 += value1 * value1;
        magnitude_squared2 += value2 * value2;
        dot_product += value1 * value2;
    }

    if magnitude_squared1.classify() == std::num::FpCategory::Zero
        || magnitude_squared2.classify() == std::num::FpCategory::Zero
    {
        return Ok(0.0);
    }

    Ok(dot_product / (magnitude_squared1.sqrt() * magnitude_squared2.sqrt()))
}

/// Parses complete or repairable partial JSON text.
///
/// This mirrors upstream `packages/ai` `parsePartialJson`: missing input returns
/// `undefined-input`, valid JSON returns `successful-parse`, and incomplete JSON
/// is repaired once before returning `repaired-parse` or `failed-parse`.
pub fn parse_partial_json(json_text: Option<&str>) -> ParsePartialJsonResult {
    let Some(json_text) = json_text else {
        return ParsePartialJsonResult::undefined_input();
    };

    if let ParseJsonResult::Success { value, .. } = safe_parse_json(json_text) {
        return ParsePartialJsonResult::successful_parse(value);
    }

    let repaired_json = fix_json(json_text);

    if let ParseJsonResult::Success { value, .. } = safe_parse_json(&repaired_json) {
        return ParsePartialJsonResult::repaired_parse(value);
    }

    ParsePartialJsonResult::failed_parse()
}

/// Finds where `searched_text` is already present or could begin at the end of `text`.
///
/// This mirrors upstream `packages/ai` `getPotentialStartIndex`: a complete
/// match returns its first byte offset, otherwise the function returns the byte
/// offset for the largest suffix of `text` that matches a prefix of
/// `searched_text`. Empty search text and no match return `None`.
pub fn get_potential_start_index(text: &str, searched_text: &str) -> Option<usize> {
    if searched_text.is_empty() {
        return None;
    }

    if let Some(index) = text.find(searched_text) {
        return Some(index);
    }

    text.char_indices()
        .rev()
        .find_map(|(index, _)| searched_text.starts_with(&text[index..]).then_some(index))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PartialJsonState {
    Root,
    Finish,
    InsideString,
    InsideStringEscape,
    InsideLiteral,
    InsideNumber,
    InsideObjectStart,
    InsideObjectKey,
    InsideObjectAfterKey,
    InsideObjectBeforeValue,
    InsideObjectAfterValue,
    InsideObjectAfterComma,
    InsideArrayStart,
    InsideArrayAfterValue,
    InsideArrayAfterComma,
}

/// Repairs a partial JSON prefix into the closest complete JSON text.
///
/// This mirrors upstream `packages/ai` `fixJson`. It is intended for incomplete
/// JSON prefixes produced during streaming; invalid complete JSON is still left
/// to the normal JSON parser.
pub fn fix_json(input: &str) -> String {
    use PartialJsonState::{
        Finish, InsideArrayAfterComma, InsideArrayAfterValue, InsideArrayStart, InsideLiteral,
        InsideNumber, InsideObjectAfterComma, InsideObjectAfterKey, InsideObjectAfterValue,
        InsideObjectBeforeValue, InsideObjectKey, InsideObjectStart, InsideString,
        InsideStringEscape, Root,
    };

    let mut stack = vec![Root];
    let mut last_valid_end = 0usize;
    let mut has_valid_char = false;
    let mut literal_start = None::<usize>;

    fn mark_valid(last_valid_end: &mut usize, has_valid_char: &mut bool, index: usize, char: char) {
        *last_valid_end = index + char.len_utf8();
        *has_valid_char = true;
    }

    fn process_value_start(
        char: char,
        index: usize,
        swap_state: PartialJsonState,
        stack: &mut Vec<PartialJsonState>,
        last_valid_end: &mut usize,
        has_valid_char: &mut bool,
        literal_start: &mut Option<usize>,
    ) {
        use PartialJsonState::{
            InsideArrayStart, InsideLiteral, InsideNumber, InsideObjectStart, InsideString,
        };

        match char {
            '"' => {
                mark_valid(last_valid_end, has_valid_char, index, char);
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideString);
            }
            'f' | 't' | 'n' => {
                mark_valid(last_valid_end, has_valid_char, index, char);
                *literal_start = Some(index);
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideLiteral);
            }
            '-' => {
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideNumber);
            }
            '0'..='9' => {
                mark_valid(last_valid_end, has_valid_char, index, char);
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideNumber);
            }
            '{' => {
                mark_valid(last_valid_end, has_valid_char, index, char);
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideObjectStart);
            }
            '[' => {
                mark_valid(last_valid_end, has_valid_char, index, char);
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideArrayStart);
            }
            _ => {}
        }
    }

    fn process_after_object_value(
        char: char,
        index: usize,
        stack: &mut Vec<PartialJsonState>,
        last_valid_end: &mut usize,
        has_valid_char: &mut bool,
    ) {
        match char {
            ',' => {
                stack.pop();
                stack.push(PartialJsonState::InsideObjectAfterComma);
            }
            '}' => {
                mark_valid(last_valid_end, has_valid_char, index, char);
                stack.pop();
            }
            _ => {}
        }
    }

    fn process_after_array_value(
        char: char,
        index: usize,
        stack: &mut Vec<PartialJsonState>,
        last_valid_end: &mut usize,
        has_valid_char: &mut bool,
    ) {
        match char {
            ',' => {
                stack.pop();
                stack.push(PartialJsonState::InsideArrayAfterComma);
            }
            ']' => {
                mark_valid(last_valid_end, has_valid_char, index, char);
                stack.pop();
            }
            _ => {}
        }
    }

    for (index, char) in input.char_indices() {
        let current_state = *stack.last().unwrap_or(&Finish);

        match current_state {
            Root => process_value_start(
                char,
                index,
                Finish,
                &mut stack,
                &mut last_valid_end,
                &mut has_valid_char,
                &mut literal_start,
            ),
            InsideObjectStart => match char {
                '"' => {
                    stack.pop();
                    stack.push(InsideObjectKey);
                }
                '}' => {
                    mark_valid(&mut last_valid_end, &mut has_valid_char, index, char);
                    stack.pop();
                }
                _ => {}
            },
            InsideObjectAfterComma => {
                if char == '"' {
                    stack.pop();
                    stack.push(InsideObjectKey);
                }
            }
            InsideObjectKey => {
                if char == '"' {
                    stack.pop();
                    stack.push(InsideObjectAfterKey);
                }
            }
            InsideObjectAfterKey => {
                if char == ':' {
                    stack.pop();
                    stack.push(InsideObjectBeforeValue);
                }
            }
            InsideObjectBeforeValue => process_value_start(
                char,
                index,
                InsideObjectAfterValue,
                &mut stack,
                &mut last_valid_end,
                &mut has_valid_char,
                &mut literal_start,
            ),
            InsideObjectAfterValue => process_after_object_value(
                char,
                index,
                &mut stack,
                &mut last_valid_end,
                &mut has_valid_char,
            ),
            InsideString => match char {
                '"' => {
                    stack.pop();
                    mark_valid(&mut last_valid_end, &mut has_valid_char, index, char);
                }
                '\\' => stack.push(InsideStringEscape),
                _ => mark_valid(&mut last_valid_end, &mut has_valid_char, index, char),
            },
            InsideArrayStart => match char {
                ']' => {
                    mark_valid(&mut last_valid_end, &mut has_valid_char, index, char);
                    stack.pop();
                }
                _ => {
                    mark_valid(&mut last_valid_end, &mut has_valid_char, index, char);
                    process_value_start(
                        char,
                        index,
                        InsideArrayAfterValue,
                        &mut stack,
                        &mut last_valid_end,
                        &mut has_valid_char,
                        &mut literal_start,
                    );
                }
            },
            InsideArrayAfterValue => match char {
                ',' => {
                    stack.pop();
                    stack.push(InsideArrayAfterComma);
                }
                ']' => {
                    mark_valid(&mut last_valid_end, &mut has_valid_char, index, char);
                    stack.pop();
                }
                _ => mark_valid(&mut last_valid_end, &mut has_valid_char, index, char),
            },
            InsideArrayAfterComma => process_value_start(
                char,
                index,
                InsideArrayAfterValue,
                &mut stack,
                &mut last_valid_end,
                &mut has_valid_char,
                &mut literal_start,
            ),
            InsideStringEscape => {
                stack.pop();
                mark_valid(&mut last_valid_end, &mut has_valid_char, index, char);
            }
            InsideNumber => match char {
                '0'..='9' => mark_valid(&mut last_valid_end, &mut has_valid_char, index, char),
                'e' | 'E' | '-' | '.' => {}
                ',' => {
                    stack.pop();

                    if stack.last() == Some(&InsideArrayAfterValue) {
                        process_after_array_value(
                            char,
                            index,
                            &mut stack,
                            &mut last_valid_end,
                            &mut has_valid_char,
                        );
                    }

                    if stack.last() == Some(&InsideObjectAfterValue) {
                        process_after_object_value(
                            char,
                            index,
                            &mut stack,
                            &mut last_valid_end,
                            &mut has_valid_char,
                        );
                    }
                }
                '}' => {
                    stack.pop();

                    if stack.last() == Some(&InsideObjectAfterValue) {
                        process_after_object_value(
                            char,
                            index,
                            &mut stack,
                            &mut last_valid_end,
                            &mut has_valid_char,
                        );
                    }
                }
                ']' => {
                    stack.pop();

                    if stack.last() == Some(&InsideArrayAfterValue) {
                        process_after_array_value(
                            char,
                            index,
                            &mut stack,
                            &mut last_valid_end,
                            &mut has_valid_char,
                        );
                    }
                }
                _ => {
                    stack.pop();
                }
            },
            InsideLiteral => {
                let Some(start) = literal_start else {
                    continue;
                };
                let partial_literal = &input[start..index + char.len_utf8()];

                if !"false".starts_with(partial_literal)
                    && !"true".starts_with(partial_literal)
                    && !"null".starts_with(partial_literal)
                {
                    stack.pop();

                    if stack.last() == Some(&InsideObjectAfterValue) {
                        process_after_object_value(
                            char,
                            index,
                            &mut stack,
                            &mut last_valid_end,
                            &mut has_valid_char,
                        );
                    } else if stack.last() == Some(&InsideArrayAfterValue) {
                        process_after_array_value(
                            char,
                            index,
                            &mut stack,
                            &mut last_valid_end,
                            &mut has_valid_char,
                        );
                    }
                } else {
                    mark_valid(&mut last_valid_end, &mut has_valid_char, index, char);
                }
            }
            Finish => {}
        }
    }

    let mut result = if has_valid_char {
        input[..last_valid_end].to_owned()
    } else {
        String::new()
    };

    for state in stack.iter().rev() {
        match state {
            InsideString => result.push('"'),
            InsideObjectKey
            | InsideObjectAfterKey
            | InsideObjectAfterComma
            | InsideObjectStart
            | InsideObjectBeforeValue
            | InsideObjectAfterValue => result.push('}'),
            InsideArrayStart | InsideArrayAfterComma | InsideArrayAfterValue => result.push(']'),
            InsideLiteral => {
                let Some(start) = literal_start else {
                    continue;
                };
                let partial_literal = &input[start..];

                if "true".starts_with(partial_literal) {
                    result.push_str(&"true"[partial_literal.len()..]);
                } else if "false".starts_with(partial_literal) {
                    result.push_str(&"false"[partial_literal.len()..]);
                } else if "null".starts_with(partial_literal) {
                    result.push_str(&"null"[partial_literal.len()..]);
                }
            }
            _ => {}
        }
    }

    result
}

/// Converts a base64 data URL into its text payload.
///
/// This mirrors upstream `packages/ai` `getTextFromDataUrl`: the URL is split
/// on the first comma-delimited header and payload segments, the media type must
/// be present in the header, and the payload is decoded with `atob`-style byte
/// to Unicode scalar mapping.
pub fn get_text_from_data_url(data_url: &str) -> Result<String, DataUrlTextError> {
    let mut parts = data_url.split(',');
    let header = parts.next().unwrap_or_default();
    let base64_content = parts.next().ok_or(DataUrlTextError::InvalidFormat)?;

    let header_prefix = header.split(';').next().unwrap_or_default();
    let _media_type = header_prefix
        .split(':')
        .nth(1)
        .ok_or(DataUrlTextError::InvalidFormat)?;

    let bytes = convert_base64_to_bytes(base64_content).map_err(|_| DataUrlTextError::Decode)?;

    Ok(bytes.into_iter().map(char::from).collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::cell::RefCell;
    use std::future::Future;
    use std::pin::Pin;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex, mpsc};
    use std::task::{Context, Poll, Wake, Waker};
    use std::time::{Duration, Instant};

    use super::{
        AbortSignalSource, AbortTimeoutOptions, Callback, CallbackResult, DataUrlTextError,
        InvalidArgumentError, NotifyCallbacks, PrepareRetriesOptions, SerialJobError,
        SerialJobExecutor, cosine_similarity, fix_json, get_potential_start_index,
        get_text_from_data_url, is_deep_equal_data, merge_abort_signals, merge_callbacks,
        merge_objects, notify, parse_partial_json, prepare_headers, prepare_retries,
        set_abort_timeout, split_array,
    };
    use crate::headers::Headers;
    use crate::json::JsonValue;
    use crate::language_model::{LanguageModelAbortController, LanguageModelAbortSignal};

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-12,
            "expected {actual} to be close to {expected}"
        );
    }

    fn source(source: impl Into<AbortSignalSource>) -> Option<AbortSignalSource> {
        Some(source.into())
    }

    fn wait_for_abort(signal: &LanguageModelAbortSignal) {
        let deadline = Instant::now() + Duration::from_millis(500);
        while !signal.is_aborted() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }

        assert!(signal.is_aborted(), "expected abort signal to be aborted");
    }

    #[derive(Clone)]
    struct TestEvent {
        value: String,
    }

    impl TestEvent {
        fn new(value: &str) -> Self {
            Self {
                value: value.to_string(),
            }
        }
    }

    #[derive(Default)]
    struct NoopWake;

    impl Wake for NoopWake {
        fn wake(self: Arc<Self>) {}
    }

    fn test_waker() -> Waker {
        Waker::from(Arc::new(NoopWake))
    }

    fn assert_pending<F: Future>(future: Pin<&mut F>) {
        let waker = test_waker();
        let mut context = Context::from_waker(&waker);
        assert!(
            matches!(future.poll(&mut context), Poll::Pending),
            "future should be pending"
        );
    }

    fn poll_callback_ready<F>(future: Pin<&mut F>) -> CallbackResult
    where
        F: Future<Output = CallbackResult>,
    {
        let waker = test_waker();
        let mut context = Context::from_waker(&waker);
        match future.poll(&mut context) {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("future should be ready"),
        }
    }

    fn poll_unit_ready<F>(future: Pin<&mut F>)
    where
        F: Future<Output = ()>,
    {
        let waker = test_waker();
        let mut context = Context::from_waker(&waker);
        match future.poll(&mut context) {
            Poll::Ready(()) => {}
            Poll::Pending => panic!("future should be ready"),
        }
    }

    #[derive(Clone)]
    struct ManualSignal {
        state: Rc<RefCell<ManualSignalState>>,
    }

    #[derive(Default)]
    struct ManualSignalState {
        resolved: bool,
        waker: Option<Waker>,
    }

    impl ManualSignal {
        fn new() -> Self {
            Self {
                state: Rc::new(RefCell::new(ManualSignalState::default())),
            }
        }

        fn resolve(&self) {
            let mut state = self.state.borrow_mut();
            state.resolved = true;
            if let Some(waker) = state.waker.take() {
                waker.wake();
            }
        }

        fn wait(&self) -> ManualWaitFuture {
            ManualWaitFuture {
                signal: self.clone(),
            }
        }
    }

    struct ManualWaitFuture {
        signal: ManualSignal,
    }

    impl Future for ManualWaitFuture {
        type Output = ();

        fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
            let mut state = self.signal.state.borrow_mut();
            if state.resolved {
                Poll::Ready(())
            } else {
                state.waker = Some(context.waker().clone());
                Poll::Pending
            }
        }
    }

    #[test]
    fn merge_callbacks_should_invoke_callbacks_in_parallel_wait_for_them_to_settle_and_continue_after_errors()
     {
        let calls = Rc::new(RefCell::new(Vec::<String>::new()));
        let first_callback_completed = ManualSignal::new();

        let first_calls = Rc::clone(&calls);
        let first_signal = first_callback_completed.clone();
        let first = Callback::infallible(move |event: TestEvent| {
            let calls = Rc::clone(&first_calls);
            let signal = first_signal.clone();
            async move {
                calls
                    .borrow_mut()
                    .push(format!("first start: {}", event.value));
                signal.wait().await;
                calls.borrow_mut().push("first end".to_string());
            }
        });

        let second_calls = Rc::clone(&calls);
        let second = Callback::new(move |_event: TestEvent| {
            let calls = Rc::clone(&second_calls);
            async move {
                calls.borrow_mut().push("second before throw".to_string());
                Err("callback error".to_string())
            }
        });

        let third_calls = Rc::clone(&calls);
        let third = Callback::infallible(move |event: TestEvent| {
            let calls = Rc::clone(&third_calls);
            async move {
                calls.borrow_mut().push(format!("third: {}", event.value));
            }
        });

        let merged = merge_callbacks([Some(first), None, Some(second), Some(third)]);
        let mut merged_future = Box::pin(merged.run(TestEvent::new("hello")));

        assert_pending(merged_future.as_mut());
        calls.borrow_mut().push("after call".to_string());

        assert_eq!(
            calls.borrow().as_slice(),
            [
                "first start: hello",
                "second before throw",
                "third: hello",
                "after call",
            ]
        );

        first_callback_completed.resolve();
        poll_callback_ready(merged_future.as_mut()).expect("callbacks settle");

        assert_eq!(
            calls.borrow().as_slice(),
            [
                "first start: hello",
                "second before throw",
                "third: hello",
                "after call",
                "first end",
            ]
        );
    }

    #[test]
    fn merge_callbacks_should_ignore_rejected_callbacks() {
        let calls = Rc::new(RefCell::new(Vec::<String>::new()));

        let first_calls = Rc::clone(&calls);
        let first = Callback::new(move |event: TestEvent| {
            let calls = Rc::clone(&first_calls);
            async move {
                calls
                    .borrow_mut()
                    .push(format!("first before reject: {}", event.value));
                Err("callback error".to_string())
            }
        });

        let second_calls = Rc::clone(&calls);
        let second = Callback::infallible(move |event: TestEvent| {
            let calls = Rc::clone(&second_calls);
            async move {
                calls.borrow_mut().push(format!("second: {}", event.value));
            }
        });

        let merged = merge_callbacks([Some(first), Some(second)]);
        let mut merged_future = Box::pin(merged.run(TestEvent::new("hello")));

        poll_callback_ready(merged_future.as_mut()).expect("callbacks settle");

        assert_eq!(
            calls.borrow().as_slice(),
            ["first before reject: hello", "second: hello"]
        );
    }

    #[test]
    fn merge_callbacks_should_ignore_undefined_callbacks() {
        let calls = Rc::new(RefCell::new(Vec::<String>::new()));
        let callback_calls = Rc::clone(&calls);
        let callback = Callback::infallible(move |event: TestEvent| {
            let calls = Rc::clone(&callback_calls);
            async move {
                calls.borrow_mut().push(event.value);
            }
        });

        let merged = merge_callbacks([None, Some(callback), None]);
        let mut merged_future = Box::pin(merged.run(TestEvent::new("hello")));

        poll_callback_ready(merged_future.as_mut()).expect("callbacks settle");

        assert_eq!(calls.borrow().as_slice(), ["hello"]);
    }

    #[test]
    fn notify_should_call_a_single_callback_with_the_event() {
        let calls = Rc::new(RefCell::new(Vec::<String>::new()));
        let callback_calls = Rc::clone(&calls);
        let callback = Callback::infallible(move |event: TestEvent| {
            let calls = Rc::clone(&callback_calls);
            async move {
                calls.borrow_mut().push(event.value);
            }
        });

        let mut future = Box::pin(notify(TestEvent::new("hello"), callback));
        poll_unit_ready(future.as_mut());

        assert_eq!(calls.borrow().as_slice(), ["hello"]);
    }

    #[test]
    fn notify_should_call_all_callbacks_when_given_an_array() {
        let calls = Rc::new(RefCell::new(Vec::<String>::new()));
        let first_calls = Rc::clone(&calls);
        let first = Callback::infallible(move |event: TestEvent| {
            let calls = Rc::clone(&first_calls);
            async move {
                calls.borrow_mut().push(format!("first: {}", event.value));
            }
        });
        let second_calls = Rc::clone(&calls);
        let second = Callback::infallible(move |event: TestEvent| {
            let calls = Rc::clone(&second_calls);
            async move {
                calls.borrow_mut().push(format!("second: {}", event.value));
            }
        });

        let mut future = Box::pin(notify(TestEvent::new("hello"), vec![first, second]));
        poll_unit_ready(future.as_mut());

        assert_eq!(calls.borrow().as_slice(), ["first: hello", "second: hello"]);
    }

    #[test]
    fn notify_should_handle_undefined_callbacks() {
        let mut future = Box::pin(notify(
            TestEvent::new("hello"),
            Option::<Callback<'_, TestEvent>>::None,
        ));

        poll_unit_ready(future.as_mut());
    }

    #[test]
    fn notify_should_handle_omitted_callbacks() {
        let mut future = Box::pin(notify(TestEvent::new("hello"), NotifyCallbacks::none()));

        poll_unit_ready(future.as_mut());
    }

    #[test]
    fn notify_should_await_async_callbacks_before_continuing() {
        let calls = Rc::new(RefCell::new(Vec::<String>::new()));
        let signal = ManualSignal::new();
        let callback_calls = Rc::clone(&calls);
        let callback_signal = signal.clone();
        let callback = Callback::infallible(move |_event: String| {
            let calls = Rc::clone(&callback_calls);
            let signal = callback_signal.clone();
            async move {
                signal.wait().await;
                calls.borrow_mut().push("async done".to_string());
            }
        });

        let mut future = Box::pin(notify("test".to_string(), callback));
        assert_pending(future.as_mut());

        signal.resolve();
        poll_unit_ready(future.as_mut());
        calls.borrow_mut().push("after notify".to_string());

        assert_eq!(calls.borrow().as_slice(), ["async done", "after notify"]);
    }

    #[test]
    fn notify_should_run_async_callbacks_in_parallel_and_await_all_of_them() {
        let calls = Rc::new(RefCell::new(Vec::<String>::new()));
        let slow_signal = ManualSignal::new();
        let slow_calls = Rc::clone(&calls);
        let slow_signal_for_callback = slow_signal.clone();
        let slow = Callback::infallible(move |_event: String| {
            let calls = Rc::clone(&slow_calls);
            let signal = slow_signal_for_callback.clone();
            async move {
                calls.borrow_mut().push("slow start".to_string());
                signal.wait().await;
                calls.borrow_mut().push("slow end".to_string());
            }
        });
        let fast_calls = Rc::clone(&calls);
        let fast = Callback::infallible(move |_event: String| {
            let calls = Rc::clone(&fast_calls);
            async move {
                calls.borrow_mut().push("fast start".to_string());
                calls.borrow_mut().push("fast end".to_string());
            }
        });

        let mut future = Box::pin(notify("test".to_string(), vec![slow, fast]));

        assert_pending(future.as_mut());
        assert_eq!(
            calls.borrow().as_slice(),
            ["slow start", "fast start", "fast end"]
        );

        slow_signal.resolve();
        poll_unit_ready(future.as_mut());

        assert_eq!(
            calls.borrow().as_slice(),
            ["slow start", "fast start", "fast end", "slow end"]
        );
    }

    #[test]
    fn notify_should_catch_errors_in_a_single_callback_without_breaking() {
        let calls = Rc::new(RefCell::new(Vec::<String>::new()));
        let callback_calls = Rc::clone(&calls);
        let callback = Callback::new(move |_event: String| {
            let calls = Rc::clone(&callback_calls);
            async move {
                calls.borrow_mut().push("before throw".to_string());
                Err("callback error".to_string())
            }
        });

        let mut future = Box::pin(notify("test".to_string(), callback));
        poll_unit_ready(future.as_mut());
        calls.borrow_mut().push("after notify".to_string());

        assert_eq!(calls.borrow().as_slice(), ["before throw", "after notify"]);
    }

    #[test]
    fn notify_should_catch_errors_in_array_callbacks_and_continue_to_next() {
        let calls = Rc::new(RefCell::new(Vec::<String>::new()));
        let first_calls = Rc::clone(&calls);
        let first = Callback::new(move |_event: String| {
            let calls = Rc::clone(&first_calls);
            async move {
                calls.borrow_mut().push("first before throw".to_string());
                Err("first error".to_string())
            }
        });
        let second_calls = Rc::clone(&calls);
        let second = Callback::infallible(move |_event: String| {
            let calls = Rc::clone(&second_calls);
            async move {
                calls.borrow_mut().push("second runs".to_string());
            }
        });

        let mut future = Box::pin(notify("test".to_string(), vec![first, second]));
        poll_unit_ready(future.as_mut());

        assert_eq!(
            calls.borrow().as_slice(),
            ["first before throw", "second runs"]
        );
    }

    #[test]
    fn notify_should_catch_async_rejection_without_breaking() {
        let calls = Rc::new(RefCell::new(Vec::<String>::new()));
        let callback_calls = Rc::clone(&calls);
        let callback = Callback::new(move |_event: String| {
            let calls = Rc::clone(&callback_calls);
            async move {
                calls.borrow_mut().push("async before reject".to_string());
                Err("async error".to_string())
            }
        });

        let mut future = Box::pin(notify("test".to_string(), callback));
        poll_unit_ready(future.as_mut());
        calls.borrow_mut().push("after notify".to_string());

        assert_eq!(
            calls.borrow().as_slice(),
            ["async before reject", "after notify"]
        );
    }

    #[test]
    fn notify_should_preserve_event_type_through_to_callback() {
        #[derive(Clone, Debug, Eq, PartialEq)]
        struct MyEvent {
            tool_name: String,
            input_location: String,
            step_number: usize,
        }

        let received = Rc::new(RefCell::new(Vec::<MyEvent>::new()));
        let callback_received = Rc::clone(&received);
        let callback = Callback::infallible(move |event: MyEvent| {
            let received = Rc::clone(&callback_received);
            async move {
                received.borrow_mut().push(event);
            }
        });

        let event = MyEvent {
            tool_name: "getWeather".to_string(),
            input_location: "San Francisco".to_string(),
            step_number: 2,
        };
        let mut future = Box::pin(notify(event.clone(), callback));
        poll_unit_ready(future.as_mut());

        assert_eq!(received.borrow().as_slice(), [event]);
    }

    #[test]
    fn notify_should_work_with_complex_nested_event_types() {
        #[derive(Clone)]
        struct Model {
            provider: String,
        }

        #[derive(Clone)]
        struct Step {
            step_number: usize,
        }

        #[derive(Clone)]
        struct ComplexEvent {
            model: Model,
            steps: Vec<Step>,
        }

        let received = Rc::new(RefCell::new(Vec::<JsonValue>::new()));
        let callback_received = Rc::clone(&received);
        let callback = Callback::infallible(move |event: ComplexEvent| {
            let received = Rc::clone(&callback_received);
            async move {
                let step_numbers = event
                    .steps
                    .iter()
                    .map(|step| step.step_number)
                    .collect::<Vec<_>>();
                received.borrow_mut().push(json!({
                    "provider": event.model.provider,
                    "stepNumbers": step_numbers,
                    "totalSteps": event.steps.len(),
                }));
            }
        });

        let event = ComplexEvent {
            model: Model {
                provider: "openai".to_string(),
            },
            steps: vec![Step { step_number: 0 }, Step { step_number: 1 }],
        };
        let mut future = Box::pin(notify(event, callback));
        poll_unit_ready(future.as_mut());

        assert_eq!(
            received.borrow().as_slice(),
            [json!({
                "provider": "openai",
                "stepNumbers": [0, 1],
                "totalSteps": 2,
            })]
        );
    }

    #[test]
    fn notify_should_handle_repeated_calls_with_the_same_callback() {
        let events = Rc::new(RefCell::new(Vec::<String>::new()));
        let callback_events = Rc::clone(&events);
        let callback = Callback::infallible(move |event: String| {
            let events = Rc::clone(&callback_events);
            async move {
                events.borrow_mut().push(event);
            }
        });

        let mut first = Box::pin(notify("first".to_string(), callback.clone()));
        poll_unit_ready(first.as_mut());
        let mut second = Box::pin(notify("second".to_string(), callback.clone()));
        poll_unit_ready(second.as_mut());
        let mut third = Box::pin(notify("third".to_string(), callback));
        poll_unit_ready(third.as_mut());

        assert_eq!(events.borrow().as_slice(), ["first", "second", "third"]);
    }

    #[test]
    fn prepare_retries_should_set_default_values_correctly_when_no_input_is_provided() {
        let prepared = prepare_retries(PrepareRetriesOptions::new());

        assert_eq!(prepared.max_retries(), 2);
        assert_eq!(prepared.retry_options().max_retries, 2);
    }

    #[test]
    fn serial_job_executor_should_execute_a_single_job_successfully() {
        let executor = SerialJobExecutor::new();
        let result = Arc::new(Mutex::new(None::<String>));
        let job_result = Arc::clone(&result);

        let handle = executor.run(move || {
            *job_result.lock().expect("result lock") = Some("done".to_string());
            Ok(())
        });

        handle.wait().expect("job succeeds");
        assert_eq!(result.lock().expect("result lock").as_deref(), Some("done"));
    }

    #[test]
    fn serial_job_executor_should_execute_multiple_jobs_in_serial_order() {
        let executor = SerialJobExecutor::new();
        let execution_order = Arc::new(Mutex::new(Vec::<usize>::new()));

        let handles = (1..=3)
            .map(|job_number| {
                let execution_order = Arc::clone(&execution_order);
                executor.run(move || {
                    execution_order
                        .lock()
                        .expect("execution order lock")
                        .push(job_number);
                    Ok(())
                })
            })
            .collect::<Vec<_>>();

        for handle in handles {
            handle.wait().expect("job succeeds");
        }

        assert_eq!(
            execution_order
                .lock()
                .expect("execution order lock")
                .as_slice(),
            [1, 2, 3]
        );
    }

    #[test]
    fn serial_job_executor_should_handle_job_errors_correctly() {
        let executor = SerialJobExecutor::new();

        let handle = executor.run(|| Err(SerialJobError::new("test error")));

        let error = handle.wait().expect_err("job fails");
        assert_eq!(error.message(), "test error");
    }

    #[test]
    fn serial_job_executor_should_execute_jobs_one_at_a_time() {
        let executor = SerialJobExecutor::new();
        let concurrent_jobs = Arc::new(Mutex::new(0usize));
        let max_concurrent_jobs = Arc::new(Mutex::new(0usize));
        let (job1_started_tx, job1_started_rx) = mpsc::channel::<()>();
        let (job2_started_tx, job2_started_rx) = mpsc::channel::<()>();
        let (job1_release_tx, job1_release_rx) = mpsc::channel::<()>();
        let (job2_release_tx, job2_release_rx) = mpsc::channel::<()>();

        let handle1 = {
            let concurrent_jobs = Arc::clone(&concurrent_jobs);
            let max_concurrent_jobs = Arc::clone(&max_concurrent_jobs);
            executor.run(move || {
                {
                    let mut concurrent = concurrent_jobs.lock().expect("concurrent jobs lock");
                    *concurrent += 1;
                    let mut max = max_concurrent_jobs
                        .lock()
                        .expect("max concurrent jobs lock");
                    *max = (*max).max(*concurrent);
                }
                job1_started_tx.send(()).expect("job1 started");
                job1_release_rx.recv().expect("job1 release");
                *concurrent_jobs.lock().expect("concurrent jobs lock") -= 1;
                Ok(())
            })
        };

        let handle2 = {
            let concurrent_jobs = Arc::clone(&concurrent_jobs);
            let max_concurrent_jobs = Arc::clone(&max_concurrent_jobs);
            executor.run(move || {
                {
                    let mut concurrent = concurrent_jobs.lock().expect("concurrent jobs lock");
                    *concurrent += 1;
                    let mut max = max_concurrent_jobs
                        .lock()
                        .expect("max concurrent jobs lock");
                    *max = (*max).max(*concurrent);
                }
                job2_started_tx.send(()).expect("job2 started");
                job2_release_rx.recv().expect("job2 release");
                *concurrent_jobs.lock().expect("concurrent jobs lock") -= 1;
                Ok(())
            })
        };

        job1_started_rx
            .recv_timeout(Duration::from_millis(500))
            .expect("job1 starts");
        assert!(
            job2_started_rx
                .recv_timeout(Duration::from_millis(20))
                .is_err(),
            "job2 should not start while job1 is still running"
        );

        job2_release_tx.send(()).expect("job2 release sent");
        job1_release_tx.send(()).expect("job1 release sent");
        handle1.wait().expect("job1 succeeds");
        job2_started_rx
            .recv_timeout(Duration::from_millis(500))
            .expect("job2 starts after job1");
        handle2.wait().expect("job2 succeeds");

        assert_eq!(*max_concurrent_jobs.lock().expect("max lock"), 1);
    }

    #[test]
    fn serial_job_executor_should_handle_mixed_success_and_failure_jobs() {
        let executor = SerialJobExecutor::new();
        let results = Arc::new(Mutex::new(Vec::<String>::new()));
        let (fail_job_release_tx, fail_job_release_rx) = mpsc::channel::<()>();

        let handle1 = {
            let results = Arc::clone(&results);
            executor.run(move || {
                results
                    .lock()
                    .expect("results lock")
                    .push("job1".to_string());
                Ok(())
            })
        };
        let handle2 = executor.run(move || {
            fail_job_release_rx.recv().expect("fail job release");
            Err(SerialJobError::new("test error"))
        });
        let handle3 = {
            let results = Arc::clone(&results);
            executor.run(move || {
                results
                    .lock()
                    .expect("results lock")
                    .push("job3".to_string());
                Ok(())
            })
        };

        handle1.wait().expect("job1 succeeds");
        assert_eq!(results.lock().expect("results lock").as_slice(), ["job1"]);

        fail_job_release_tx.send(()).expect("fail job release sent");
        let error = handle2.wait().expect_err("job2 fails");
        assert_eq!(error.message(), "test error");

        handle3.wait().expect("job3 succeeds");
        assert_eq!(
            results.lock().expect("results lock").as_slice(),
            ["job1", "job3"]
        );
    }

    #[test]
    fn serial_job_executor_should_handle_concurrent_calls_to_run() {
        let executor = SerialJobExecutor::new();
        let start_order = Arc::new(Mutex::new(Vec::<usize>::new()));
        let execution_order = Arc::new(Mutex::new(Vec::<usize>::new()));
        let (job1_release_tx, job1_release_rx) = mpsc::channel::<()>();
        let (job2_release_tx, job2_release_rx) = mpsc::channel::<()>();
        let (job3_release_tx, job3_release_rx) = mpsc::channel::<()>();

        let handle1 = {
            let start_order = Arc::clone(&start_order);
            let execution_order = Arc::clone(&execution_order);
            executor.run(move || {
                start_order.lock().expect("start order lock").push(1);
                job1_release_rx.recv().expect("job1 release");
                execution_order
                    .lock()
                    .expect("execution order lock")
                    .push(1);
                Ok(())
            })
        };
        let handle2 = {
            let start_order = Arc::clone(&start_order);
            let execution_order = Arc::clone(&execution_order);
            executor.run(move || {
                start_order.lock().expect("start order lock").push(2);
                job2_release_rx.recv().expect("job2 release");
                execution_order
                    .lock()
                    .expect("execution order lock")
                    .push(2);
                Ok(())
            })
        };
        let handle3 = {
            let start_order = Arc::clone(&start_order);
            let execution_order = Arc::clone(&execution_order);
            executor.run(move || {
                start_order.lock().expect("start order lock").push(3);
                job3_release_rx.recv().expect("job3 release");
                execution_order
                    .lock()
                    .expect("execution order lock")
                    .push(3);
                Ok(())
            })
        };

        job3_release_tx.send(()).expect("job3 release sent");
        job2_release_tx.send(()).expect("job2 release sent");
        job1_release_tx.send(()).expect("job1 release sent");

        handle1.wait().expect("job1 succeeds");
        handle2.wait().expect("job2 succeeds");
        handle3.wait().expect("job3 succeeds");

        assert_eq!(
            start_order.lock().expect("start order lock").as_slice(),
            [1, 2, 3]
        );
        assert_eq!(
            execution_order
                .lock()
                .expect("execution order lock")
                .as_slice(),
            [1, 2, 3]
        );
    }

    #[test]
    fn merge_abort_signals_should_return_a_signal_that_is_initially_not_aborted() {
        let controller1 = LanguageModelAbortController::new();
        let controller2 = LanguageModelAbortController::new();

        let merged =
            merge_abort_signals([source(controller1.signal()), source(controller2.signal())])
                .expect("merged signal exists");

        assert!(!merged.is_aborted());
    }

    #[test]
    fn merge_abort_signals_should_abort_when_the_first_signal_aborts() {
        let controller1 = LanguageModelAbortController::new();
        let controller2 = LanguageModelAbortController::new();
        let merged =
            merge_abort_signals([source(controller1.signal()), source(controller2.signal())])
                .expect("merged signal exists");

        controller1.abort();

        assert!(merged.is_aborted());
    }

    #[test]
    fn merge_abort_signals_should_abort_when_the_second_signal_aborts() {
        let controller1 = LanguageModelAbortController::new();
        let controller2 = LanguageModelAbortController::new();
        let merged =
            merge_abort_signals([source(controller1.signal()), source(controller2.signal())])
                .expect("merged signal exists");

        controller2.abort();

        assert!(merged.is_aborted());
    }

    #[test]
    fn merge_abort_signals_should_preserve_the_abort_reason_from_the_triggering_signal() {
        let controller1 = LanguageModelAbortController::new();
        let controller2 = LanguageModelAbortController::new();
        let reason = json!({
            "message": "custom abort reason"
        });
        let merged =
            merge_abort_signals([source(controller1.signal()), source(controller2.signal())])
                .expect("merged signal exists");

        controller1.abort_with_reason(reason.clone());

        assert_eq!(merged.reason(), Some(reason));
    }

    #[test]
    fn merge_abort_signals_should_preserve_string_abort_reason() {
        let controller = LanguageModelAbortController::new();
        let merged = merge_abort_signals([source(controller.signal())])
            .expect("single signal should be returned");

        controller.abort_with_reason("string reason");

        assert_eq!(merged.reason(), Some(json!("string reason")));
    }

    #[test]
    fn merge_abort_signals_should_handle_already_aborted_signals() {
        let controller = LanguageModelAbortController::new();
        let reason = json!({
            "message": "already aborted"
        });
        controller.abort_with_reason(reason.clone());

        let merged = merge_abort_signals([source(controller.signal())])
            .expect("single signal should be returned");

        assert!(merged.is_aborted());
        assert_eq!(merged.reason(), Some(reason));
    }

    #[test]
    fn merge_abort_signals_should_use_the_first_already_aborted_signal_reason_when_multiple_are_aborted()
     {
        let controller1 = LanguageModelAbortController::new();
        let controller2 = LanguageModelAbortController::new();
        let reason1 = json!({
            "message": "first reason"
        });
        let reason2 = json!({
            "message": "second reason"
        });
        controller1.abort_with_reason(reason1.clone());
        controller2.abort_with_reason(reason2);

        let merged =
            merge_abort_signals([source(controller1.signal()), source(controller2.signal())])
                .expect("merged signal exists");

        assert!(merged.is_aborted());
        assert_eq!(merged.reason(), Some(reason1));
    }

    #[test]
    fn merge_abort_signals_should_return_none_when_no_signals_are_provided() {
        assert!(merge_abort_signals(Vec::<Option<AbortSignalSource>>::new()).is_none());
    }

    #[test]
    fn merge_abort_signals_should_return_none_when_only_absent_signals_are_provided() {
        assert!(merge_abort_signals([None, None]).is_none());
    }

    #[test]
    fn merge_abort_signals_should_create_a_timeout_signal_from_numeric_input() {
        let merged = merge_abort_signals([source(AbortSignalSource::timeout_ms(10))])
            .expect("timeout signal exists");

        assert!(!merged.is_aborted());

        wait_for_abort(&merged);

        let reason = merged.reason().expect("timeout reason is present");
        assert_eq!(reason["name"], "TimeoutError");
    }

    #[test]
    fn merge_abort_signals_should_preserve_the_first_abort_reason_when_mixing_signals_and_timeouts()
    {
        let controller = LanguageModelAbortController::new();
        let reason = json!({
            "message": "manual abort reason"
        });
        let merged = merge_abort_signals([
            source(controller.signal()),
            source(AbortSignalSource::timeout_ms(100)),
        ])
        .expect("merged signal exists");

        controller.abort_with_reason(reason.clone());

        assert!(merged.is_aborted());
        assert_eq!(merged.reason(), Some(reason));
    }

    #[test]
    fn merge_abort_signals_should_filter_out_absent_signals() {
        let controller = LanguageModelAbortController::new();
        let reason = json!({
            "message": "abort reason"
        });
        let merged = merge_abort_signals([None, source(controller.signal()), None])
            .expect("merged signal exists");

        assert!(!merged.is_aborted());

        controller.abort_with_reason(reason.clone());

        assert!(merged.is_aborted());
        assert_eq!(merged.reason(), Some(reason));
    }

    #[test]
    fn merge_abort_signals_should_return_the_signal_directly_when_only_one_valid_signal_is_provided()
     {
        let controller = LanguageModelAbortController::new();
        let signal = controller.signal();
        let merged =
            merge_abort_signals([None, source(signal.clone()), None]).expect("signal exists");

        assert!(merged.is_same_signal(&signal));
    }

    #[test]
    fn merge_abort_signals_should_use_the_first_aborting_signal_reason_when_multiple_abort_simultaneously()
     {
        let controller1 = LanguageModelAbortController::new();
        let controller2 = LanguageModelAbortController::new();
        let reason1 = json!({
            "message": "first reason"
        });
        let reason2 = json!({
            "message": "second reason"
        });
        let merged =
            merge_abort_signals([source(controller1.signal()), source(controller2.signal())])
                .expect("merged signal exists");

        controller1.abort_with_reason(reason1.clone());
        controller2.abort_with_reason(reason2);

        assert_eq!(merged.reason(), Some(reason1));
    }

    #[test]
    fn merge_abort_signals_should_return_the_original_signal_when_only_one_signal_is_provided() {
        let controller = LanguageModelAbortController::new();
        let signal = controller.signal();
        let merged = merge_abort_signals([source(signal.clone())]).expect("signal exists");

        assert!(merged.is_same_signal(&signal));
    }

    #[test]
    fn merge_abort_signals_should_work_with_many_signals() {
        let controllers = (0..10)
            .map(|_| LanguageModelAbortController::new())
            .collect::<Vec<_>>();
        let reason = json!({
            "message": "signal 5 reason"
        });
        let merged = merge_abort_signals(
            controllers
                .iter()
                .map(|controller| source(controller.signal())),
        )
        .expect("merged signal exists");

        assert!(!merged.is_aborted());

        controllers[5].abort_with_reason(reason.clone());

        assert!(merged.is_aborted());
        assert_eq!(merged.reason(), Some(reason));
    }

    #[test]
    fn set_abort_timeout_should_not_abort_the_controller_before_the_timeout_elapses() {
        let abort_controller = LanguageModelAbortController::new();
        let handle = set_abort_timeout(
            AbortTimeoutOptions::new("Step")
                .with_abort_controller(abort_controller.clone())
                .with_timeout_ms(100),
        )
        .expect("timeout is scheduled");

        std::thread::sleep(Duration::from_millis(20));
        handle.cancel();

        assert!(!abort_controller.signal().is_aborted());
    }

    #[test]
    fn set_abort_timeout_should_abort_the_controller_when_the_timeout_elapses() {
        let abort_controller = LanguageModelAbortController::new();
        let signal = abort_controller.signal();
        set_abort_timeout(
            AbortTimeoutOptions::new("Step")
                .with_abort_controller(abort_controller)
                .with_timeout_ms(10),
        );

        wait_for_abort(&signal);
    }

    #[test]
    fn set_abort_timeout_should_abort_with_a_timeout_error_reason() {
        let abort_controller = LanguageModelAbortController::new();
        let signal = abort_controller.signal();
        set_abort_timeout(
            AbortTimeoutOptions::new("Step")
                .with_abort_controller(abort_controller)
                .with_timeout_ms(10),
        );

        wait_for_abort(&signal);

        let reason = signal.reason().expect("timeout reason is present");
        assert_eq!(reason["name"], "TimeoutError");
    }

    #[test]
    fn set_abort_timeout_should_include_the_label_and_duration_in_the_abort_reason_message() {
        let abort_controller = LanguageModelAbortController::new();
        let signal = abort_controller.signal();
        set_abort_timeout(
            AbortTimeoutOptions::new("Chunk")
                .with_abort_controller(abort_controller)
                .with_timeout_ms(10),
        );

        wait_for_abort(&signal);

        let reason = signal.reason().expect("timeout reason is present");
        assert_eq!(reason["message"], "Chunk timeout of 10ms exceeded");
    }

    #[test]
    fn set_abort_timeout_should_return_a_handle_that_can_be_cancelled_to_cancel_the_abort() {
        let abort_controller = LanguageModelAbortController::new();
        let handle = set_abort_timeout(
            AbortTimeoutOptions::new("Step")
                .with_abort_controller(abort_controller.clone())
                .with_timeout_ms(10),
        )
        .expect("timeout is scheduled");

        handle.cancel();
        std::thread::sleep(Duration::from_millis(30));

        assert!(handle.is_cancelled());
        assert!(!abort_controller.signal().is_aborted());
    }

    #[test]
    fn set_abort_timeout_should_return_none_when_abort_controller_is_absent() {
        let handle = set_abort_timeout(AbortTimeoutOptions::new("Step").with_timeout_ms(10));

        assert!(handle.is_none());
    }

    #[test]
    fn set_abort_timeout_should_return_none_when_timeout_ms_is_absent() {
        let abort_controller = LanguageModelAbortController::new();

        let handle = set_abort_timeout(
            AbortTimeoutOptions::new("Step").with_abort_controller(abort_controller.clone()),
        );
        std::thread::sleep(Duration::from_millis(20));

        assert!(handle.is_none());
        assert!(!abort_controller.signal().is_aborted());
    }

    #[test]
    fn is_deep_equal_data_compares_primitives() {
        assert!(is_deep_equal_data(&json!(1), &json!(1)));
        assert!(!is_deep_equal_data(&json!(1), &json!(2)));
        assert!(!is_deep_equal_data(&json!(null), &json!({ "a": 1 })));
    }

    #[test]
    fn is_deep_equal_data_compares_nested_objects() {
        assert!(is_deep_equal_data(
            &json!({ "a": { "c": 1 }, "b": [true, null] }),
            &json!({ "b": [true, null], "a": { "c": 1 } })
        ));

        assert!(!is_deep_equal_data(
            &json!({ "a": { "c": 1 }, "b": 2 }),
            &json!({ "a": { "c": 2 }, "b": 2 })
        ));
    }

    #[test]
    fn is_deep_equal_data_compares_arrays_by_order_and_length() {
        assert!(is_deep_equal_data(
            &json!([1, { "a": "b" }, 3]),
            &json!([1, { "a": "b" }, 3])
        ));
        assert!(!is_deep_equal_data(&json!([1, 2, 3]), &json!([1, 3, 2])));
        assert!(!is_deep_equal_data(&json!([1, 2]), &json!([1, 2, 3])));
    }

    #[test]
    fn is_deep_equal_data_distinguishes_objects_from_arrays() {
        assert!(!is_deep_equal_data(
            &json!({ "0": "one", "1": "two", "length": 2 }),
            &json!(["one", "two"])
        ));
    }

    #[test]
    fn is_deep_equal_data_should_check_if_two_primitives_are_equal() {
        assert!(is_deep_equal_data(&json!(1), &json!(1)));
        assert!(!is_deep_equal_data(&json!(1), &json!(2)));
    }

    #[test]
    fn is_deep_equal_data_should_return_false_for_different_types() {
        assert!(!is_deep_equal_data(&json!({ "a": 1 }), &json!(1)));
    }

    #[test]
    fn is_deep_equal_data_should_return_false_for_null_values_compared_with_objects() {
        assert!(!is_deep_equal_data(&json!({ "a": 1 }), &json!(null)));
    }

    #[test]
    fn is_deep_equal_data_should_identify_two_equal_objects() {
        assert!(is_deep_equal_data(
            &json!({ "a": 1, "b": 2 }),
            &json!({ "a": 1, "b": 2 })
        ));
    }

    #[test]
    fn is_deep_equal_data_should_identify_two_objects_with_different_values() {
        assert!(!is_deep_equal_data(
            &json!({ "a": 1, "b": 2 }),
            &json!({ "a": 1, "b": 3 })
        ));
    }

    #[test]
    fn is_deep_equal_data_should_identify_two_objects_with_different_number_of_keys() {
        assert!(!is_deep_equal_data(
            &json!({ "a": 1, "b": 2 }),
            &json!({ "a": 1, "b": 2, "c": 3 })
        ));
    }

    #[test]
    fn is_deep_equal_data_should_handle_nested_objects() {
        assert!(is_deep_equal_data(
            &json!({ "a": { "c": 1 }, "b": 2 }),
            &json!({ "a": { "c": 1 }, "b": 2 })
        ));
    }

    #[test]
    fn is_deep_equal_data_should_detect_inequality_in_nested_objects() {
        assert!(!is_deep_equal_data(
            &json!({ "a": { "c": 1 }, "b": 2 }),
            &json!({ "a": { "c": 2 }, "b": 2 })
        ));
    }

    #[test]
    fn is_deep_equal_data_should_compare_arrays_correctly() {
        assert!(is_deep_equal_data(&json!([1, 2, 3]), &json!([1, 2, 3])));
        assert!(!is_deep_equal_data(&json!([1, 2, 3]), &json!([1, 2, 4])));
    }

    #[test]
    fn is_deep_equal_data_should_return_false_for_null_comparison_with_object() {
        assert!(!is_deep_equal_data(&json!({ "a": 1 }), &json!(null)));
    }

    #[test]
    fn is_deep_equal_data_should_distinguish_between_array_and_object_with_same_enumerable_properties()
     {
        assert!(!is_deep_equal_data(
            &json!({ "0": "one", "1": "two", "length": 2 }),
            &json!(["one", "two"])
        ));
    }

    #[test]
    fn prepare_headers_should_set_content_type_header_if_not_present() {
        let headers = prepare_headers(Some(Headers::new()), content_type_default_headers());

        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    #[test]
    fn prepare_headers_should_not_overwrite_existing_content_type_header() {
        let headers = prepare_headers(
            Some(Headers::from([(
                "Content-Type".to_string(),
                "text/html".to_string(),
            )])),
            content_type_default_headers(),
        );

        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("text/html")
        );
    }

    #[test]
    fn prepare_headers_should_handle_undefined_init() {
        let headers = prepare_headers(None, content_type_default_headers());

        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    #[test]
    fn prepare_headers_should_handle_init_headers_as_headers_object() {
        let headers = prepare_headers(
            Some(Headers::from([("init".to_string(), "foo".to_string())])),
            content_type_default_headers(),
        );

        assert_eq!(headers.get("init").map(String::as_str), Some("foo"));
        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    #[test]
    fn prepare_headers_should_handle_response_object_headers() {
        let headers = prepare_headers(
            Some(Headers::from([
                ("init".to_string(), "foo".to_string()),
                ("extra".to_string(), "bar".to_string()),
            ])),
            content_type_default_headers(),
        );

        assert_eq!(headers.get("init").map(String::as_str), Some("foo"));
        assert_eq!(headers.get("extra").map(String::as_str), Some("bar"));
        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    fn content_type_default_headers() -> Headers {
        Headers::from([("content-type".to_string(), "application/json".to_string())])
    }

    #[test]
    fn merge_objects_should_merge_two_flat_objects() {
        let target = json!({ "a": 1, "b": 2 });
        let source = json!({ "b": 3, "c": 4 });

        assert_eq!(
            merge_objects(Some(&target), Some(&source)),
            Some(json!({ "a": 1, "b": 3, "c": 4 }))
        );
        assert_eq!(target, json!({ "a": 1, "b": 2 }));
        assert_eq!(source, json!({ "b": 3, "c": 4 }));
    }

    #[test]
    fn merge_objects_should_deeply_merge_nested_objects() {
        let target = json!({ "a": 1, "b": { "c": 2, "d": 3 } });
        let source = json!({ "b": { "c": 4, "e": 5 } });

        assert_eq!(
            merge_objects(Some(&target), Some(&source)),
            Some(json!({ "a": 1, "b": { "c": 4, "d": 3, "e": 5 } }))
        );
    }

    #[test]
    fn merge_objects_should_replace_arrays_instead_of_merging_them() {
        let target = json!({ "a": [1, 2, 3], "b": 2 });
        let source = json!({ "a": [4, 5] });

        assert_eq!(
            merge_objects(Some(&target), Some(&source)),
            Some(json!({ "a": [4, 5], "b": 2 }))
        );
    }

    #[test]
    fn merge_objects_should_handle_null_values() {
        let target = json!({ "a": 1, "b": null });
        let source = json!({ "a": null, "b": 2 });

        assert_eq!(
            merge_objects(Some(&target), Some(&source)),
            Some(json!({ "a": null, "b": 2 }))
        );
    }

    #[test]
    fn merge_objects_should_handle_complex_nested_structures() {
        let target = json!({
            "a": 1,
            "b": {
                "c": [1, 2, 3],
                "d": {
                    "e": 4,
                    "f": 5
                }
            }
        });
        let source = json!({
            "b": {
                "c": [4, 5],
                "d": {
                    "f": 6,
                    "g": 7
                }
            },
            "h": 8
        });

        assert_eq!(
            merge_objects(Some(&target), Some(&source)),
            Some(json!({
                "a": 1,
                "b": {
                    "c": [4, 5],
                    "d": {
                        "e": 4,
                        "f": 6,
                        "g": 7
                    }
                },
                "h": 8
            }))
        );
    }

    #[test]
    fn merge_objects_should_handle_empty_objects() {
        let empty = json!({});
        let object = json!({ "a": 1 });

        assert_eq!(
            merge_objects(Some(&empty), Some(&object)),
            Some(json!({ "a": 1 }))
        );
        assert_eq!(
            merge_objects(Some(&object), Some(&empty)),
            Some(json!({ "a": 1 }))
        );
    }

    #[test]
    fn merge_objects_should_handle_undefined_inputs() {
        let target = json!({ "a": 1 });
        let source = json!({ "b": 2 });

        assert_eq!(merge_objects(None, None), None);
        assert_eq!(merge_objects(Some(&target), None), Some(json!({ "a": 1 })));
        assert_eq!(merge_objects(None, Some(&source)), Some(json!({ "b": 2 })));
    }

    #[test]
    fn merge_objects_should_not_pollute_object_prototype_via_proto() {
        let malicious = json!({ "__proto__": { "polluted": true } });

        assert_eq!(
            merge_objects(Some(&json!({})), Some(&malicious)),
            Some(json!({}))
        );
    }

    #[test]
    fn merge_objects_should_ignore_proto_constructor_and_prototype_keys() {
        let malicious = json!({
            "__proto__": { "a": 1 },
            "constructor": { "prototype": { "b": 2 } },
            "prototype": { "c": 3 },
            "safe": "value"
        });

        assert_eq!(
            merge_objects(Some(&json!({ "existing": "ok" })), Some(&malicious)),
            Some(json!({ "existing": "ok", "safe": "value" }))
        );
    }

    #[test]
    fn merge_objects_should_ignore_dangerous_keys_nested_in_mergeable_objects() {
        let base = json!({ "metadata": { "user": "alice" } });
        let malicious = json!({
            "metadata": {
                "__proto__": { "polluted": true },
                "role": "admin"
            }
        });

        assert_eq!(
            merge_objects(Some(&base), Some(&malicious)),
            Some(json!({ "metadata": { "user": "alice", "role": "admin" } }))
        );
    }

    #[test]
    fn merge_objects_deeply_merges_json_objects() {
        let base = json!({
            "a": 1,
            "b": {
                "c": [1, 2, 3],
                "d": {
                    "e": 4,
                    "f": 5,
                },
            },
        });
        let overrides = json!({
            "b": {
                "c": [4, 5],
                "d": {
                    "f": 6,
                    "g": 7,
                },
            },
            "h": 8,
        });

        assert_eq!(
            merge_objects(Some(&base), Some(&overrides)),
            Some(json!({
                "a": 1,
                "b": {
                    "c": [4, 5],
                    "d": {
                        "e": 4,
                        "f": 6,
                        "g": 7,
                    },
                },
                "h": 8,
            }))
        );
        assert_eq!(base["b"]["c"], json!([1, 2, 3]));
        assert_eq!(overrides["b"]["c"], json!([4, 5]));
    }

    #[test]
    fn merge_objects_handles_missing_inputs_and_replacements() {
        let base = json!({ "a": 1, "b": null, "c": [1, 2, 3] });
        let overrides = json!({ "a": null, "b": 2, "c": [4, 5] });

        assert_eq!(merge_objects(None, None), None);
        assert_eq!(merge_objects(Some(&base), None), Some(base.clone()));
        assert_eq!(
            merge_objects(None, Some(&overrides)),
            Some(overrides.clone())
        );
        assert_eq!(
            merge_objects(Some(&base), Some(&overrides)),
            Some(json!({ "a": null, "b": 2, "c": [4, 5] }))
        );
    }

    #[test]
    fn merge_objects_ignores_dangerous_override_keys() {
        let base = json!({
            "existing": "ok",
            "metadata": { "user": "alice" },
        });
        let malicious = serde_json::from_str::<serde_json::Value>(
            r#"{
                "__proto__": { "a": 1 },
                "constructor": { "prototype": { "b": 2 } },
                "prototype": { "c": 3 },
                "safe": "value",
                "metadata": {
                    "__proto__": { "polluted": true },
                    "role": "admin"
                }
            }"#,
        )
        .unwrap();

        assert_eq!(
            merge_objects(Some(&base), Some(&malicious)),
            Some(json!({
                "existing": "ok",
                "safe": "value",
                "metadata": {
                    "user": "alice",
                    "role": "admin",
                },
            }))
        );
    }

    #[test]
    fn split_array_should_split_an_array_into_chunks_of_the_specified_size() {
        assert_eq!(
            split_array(&[1, 2, 3, 4, 5], 2).unwrap(),
            vec![vec![1, 2], vec![3, 4], vec![5]]
        );
    }

    #[test]
    fn split_array_should_return_empty_array_when_input_array_is_empty() {
        let empty: Vec<Vec<i32>> = split_array(&[], 2).unwrap();

        assert!(empty.is_empty());
    }

    #[test]
    fn split_array_should_return_original_array_when_chunk_size_is_greater_than_array_length() {
        assert_eq!(split_array(&[1, 2, 3], 5).unwrap(), vec![vec![1, 2, 3]]);
    }

    #[test]
    fn split_array_should_return_original_array_when_chunk_size_is_equal_to_array_length() {
        assert_eq!(split_array(&[1, 2, 3], 3).unwrap(), vec![vec![1, 2, 3]]);
    }

    #[test]
    fn split_array_should_handle_chunk_size_of_one_correctly() {
        assert_eq!(
            split_array(&[1, 2, 3], 1).unwrap(),
            vec![vec![1], vec![2], vec![3]]
        );
    }

    #[test]
    fn split_array_should_throw_error_for_chunk_size_of_zero() {
        assert_eq!(
            split_array(&[1, 2, 3], 0).unwrap_err(),
            super::SplitArrayError
        );
    }

    #[test]
    fn split_array_should_throw_error_for_negative_chunk_size() {
        assert_eq!(
            split_array(&[1, 2, 3], -1).unwrap_err().to_string(),
            "chunkSize must be greater than 0"
        );
    }

    #[test]
    fn split_array_should_handle_non_integer_chunk_size_by_flooring_the_size() {
        let size = 2.5_f64.floor() as isize;

        assert_eq!(
            split_array(&[1, 2, 3, 4, 5], size).unwrap(),
            vec![vec![1, 2], vec![3, 4], vec![5]]
        );
    }

    #[test]
    fn get_potential_start_index_should_return_null_when_searched_text_is_empty() {
        assert_eq!(get_potential_start_index("1234567890", ""), None);
    }

    #[test]
    fn get_potential_start_index_should_return_null_when_searched_text_is_not_in_text() {
        assert_eq!(get_potential_start_index("1234567890", "a"), None);
    }

    #[test]
    fn get_potential_start_index_should_return_index_when_searched_text_is_in_text() {
        assert_eq!(
            get_potential_start_index("1234567890", "1234567890"),
            Some(0)
        );
    }

    #[test]
    fn get_potential_start_index_should_return_index_when_searched_text_might_start_in_text() {
        assert_eq!(get_potential_start_index("1234567890", "0123"), Some(9));
    }

    #[test]
    fn get_potential_start_index_should_return_index_for_longer_possible_overlap() {
        assert_eq!(get_potential_start_index("1234567890", "90123"), Some(8));
    }

    #[test]
    fn get_potential_start_index_should_return_index_for_longest_possible_overlap() {
        assert_eq!(get_potential_start_index("1234567890", "890123"), Some(7));
    }

    #[test]
    fn fix_json_repairs_incomplete_literals_numbers_and_strings() {
        assert_eq!(fix_json(""), "");
        assert_eq!(fix_json("nul"), "null");
        assert_eq!(fix_json("t"), "true");
        assert_eq!(fix_json("fals"), "false");
        assert_eq!(fix_json("12."), "12");
        assert_eq!(fix_json("2.5e-"), "2.5");
        assert_eq!(fix_json("2.5E3"), "2.5E3");
        assert_eq!(fix_json("-"), "");
        assert_eq!(fix_json(r#""abc"#), r#""abc""#);
        assert_eq!(fix_json(r#""value with \"#), r#""value with ""#);
    }

    #[test]
    fn fix_json_repairs_incomplete_arrays_and_objects() {
        assert_eq!(fix_json("["), "[]");
        assert_eq!(fix_json("[[1], [2"), "[[1], [2]]");
        assert_eq!(fix_json("[[false], [nu"), "[[false], [null]]");
        assert_eq!(fix_json("[1, "), "[1]");
        assert_eq!(fix_json(r#"{"key":"#), "{}");
        assert_eq!(fix_json(r#"{"k1": 1, "k2"#), r#"{"k1": 1}"#);
        assert_eq!(fix_json(r#"{"key": "value"  "#), r#"{"key": "value"}"#);
        assert_eq!(
            fix_json(r#"{"a": {"b": ["c", {"d": "e","#),
            r#"{"a": {"b": ["c", {"d": "e"}]}}"#
        );
        assert_eq!(
            fix_json(r#"{"type":"div","children":[{"type":"Card","props":{}"#),
            r#"{"type":"div","children":[{"type":"Card","props":{}}]}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_empty_input() {
        assert_eq!(fix_json(""), "");
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_null() {
        assert_eq!(fix_json("nul"), "null");
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_true() {
        assert_eq!(fix_json("t"), "true");
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_false() {
        assert_eq!(fix_json("fals"), "false");
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_numbers() {
        assert_eq!(fix_json("12."), "12");
    }

    #[test]
    fn fix_json_upstream_handles_numbers_with_dot() {
        assert_eq!(fix_json("12.2"), "12.2");
    }

    #[test]
    fn fix_json_upstream_handles_negative_numbers() {
        assert_eq!(fix_json("-12"), "-12");
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_negative_numbers() {
        assert_eq!(fix_json("-"), "");
    }

    #[test]
    fn fix_json_upstream_handles_e_notation_numbers() {
        assert_eq!(fix_json("2.5e"), "2.5");
        assert_eq!(fix_json("2.5e-"), "2.5");
        assert_eq!(fix_json("2.5e3"), "2.5e3");
        assert_eq!(fix_json("-2.5e3"), "-2.5e3");
    }

    #[test]
    fn fix_json_upstream_handles_uppercase_e_notation_numbers() {
        assert_eq!(fix_json("2.5E"), "2.5");
        assert_eq!(fix_json("2.5E-"), "2.5");
        assert_eq!(fix_json("2.5E3"), "2.5E3");
        assert_eq!(fix_json("-2.5E3"), "-2.5E3");
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_exponent_numbers() {
        assert_eq!(fix_json("12.e"), "12");
        assert_eq!(fix_json("12.34e"), "12.34");
        assert_eq!(fix_json("5e"), "5");
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_strings() {
        assert_eq!(fix_json(r#""abc"#), r#""abc""#);
    }

    #[test]
    fn fix_json_upstream_handles_escape_sequences() {
        assert_eq!(
            fix_json(r#""value with \"quoted\" text and \\ escape"#),
            r#""value with \"quoted\" text and \\ escape""#
        );
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_escape_sequences() {
        assert_eq!(fix_json(r#""value with \"#), r#""value with ""#);
    }

    #[test]
    fn fix_json_upstream_handles_unicode_characters() {
        assert_eq!(
            fix_json(r#""value with unicode <""#),
            r#""value with unicode <""#
        );
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_array() {
        assert_eq!(fix_json("["), "[]");
    }

    #[test]
    fn fix_json_upstream_handles_closing_bracket_after_number_in_array() {
        assert_eq!(fix_json("[[1], [2"), "[[1], [2]]");
    }

    #[test]
    fn fix_json_upstream_handles_closing_bracket_after_string_in_array() {
        assert_eq!(fix_json(r#"[["1"], ["2"#), r#"[["1"], ["2"]]"#);
    }

    #[test]
    fn fix_json_upstream_handles_closing_bracket_after_literal_in_array() {
        assert_eq!(fix_json("[[false], [nu"), "[[false], [null]]");
    }

    #[test]
    fn fix_json_upstream_handles_closing_bracket_after_array_in_array() {
        assert_eq!(fix_json("[[[]], [[]"), "[[[]], [[]]]");
    }

    #[test]
    fn fix_json_upstream_handles_closing_bracket_after_object_in_array() {
        assert_eq!(fix_json("[[{}], [{"), "[[{}], [{}]]");
    }

    #[test]
    fn fix_json_upstream_handles_trailing_comma() {
        assert_eq!(fix_json("[1, "), "[1]");
    }

    #[test]
    fn fix_json_upstream_handles_closing_array() {
        assert_eq!(fix_json("[[], 123"), "[[], 123]");
    }

    #[test]
    fn fix_json_upstream_handles_keys_without_values() {
        assert_eq!(fix_json(r#"{"key":"#), "{}");
    }

    #[test]
    fn fix_json_upstream_handles_closing_brace_after_number_in_object() {
        assert_eq!(
            fix_json(r#"{"a": {"b": 1}, "c": {"d": 2"#),
            r#"{"a": {"b": 1}, "c": {"d": 2}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_closing_brace_after_string_in_object() {
        assert_eq!(
            fix_json(r#"{"a": {"b": "1"}, "c": {"d": 2"#),
            r#"{"a": {"b": "1"}, "c": {"d": 2}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_closing_brace_after_literal_in_object() {
        assert_eq!(
            fix_json(r#"{"a": {"b": false}, "c": {"d": 2"#),
            r#"{"a": {"b": false}, "c": {"d": 2}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_closing_brace_after_array_in_object() {
        assert_eq!(
            fix_json(r#"{"a": {"b": []}, "c": {"d": 2"#),
            r#"{"a": {"b": []}, "c": {"d": 2}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_closing_brace_after_object_in_object() {
        assert_eq!(
            fix_json(r#"{"a": {"b": {}}, "c": {"d": 2"#),
            r#"{"a": {"b": {}}, "c": {"d": 2}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_partial_keys_first_key() {
        assert_eq!(fix_json(r#"{"ke"#), "{}");
    }

    #[test]
    fn fix_json_upstream_handles_partial_keys_second_key() {
        assert_eq!(fix_json(r#"{"k1": 1, "k2"#), r#"{"k1": 1}"#);
    }

    #[test]
    fn fix_json_upstream_handles_partial_keys_with_colon_second_key() {
        assert_eq!(fix_json(r#"{"k1": 1, "k2":"#), r#"{"k1": 1}"#);
    }

    #[test]
    fn fix_json_upstream_handles_trailing_whitespace() {
        assert_eq!(fix_json(r#"{"key": "value"  "#), r#"{"key": "value"}"#);
    }

    #[test]
    fn fix_json_upstream_handles_closing_after_empty_object() {
        assert_eq!(fix_json(r#"{"a": {"b": {}"#), r#"{"a": {"b": {}}}"#);
    }

    #[test]
    fn fix_json_upstream_handles_nested_arrays_with_numbers() {
        assert_eq!(fix_json("[1, [2, 3, ["), "[1, [2, 3, []]]");
    }

    #[test]
    fn fix_json_upstream_handles_nested_arrays_with_literals() {
        assert_eq!(fix_json("[false, [true, ["), "[false, [true, []]]");
    }

    #[test]
    fn fix_json_upstream_handles_nested_objects() {
        assert_eq!(fix_json(r#"{"key": {"subKey":"#), r#"{"key": {}}"#);
    }

    #[test]
    fn fix_json_upstream_handles_nested_objects_with_numbers() {
        assert_eq!(
            fix_json(r#"{"key": 123, "key2": {"subKey":"#),
            r#"{"key": 123, "key2": {}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_nested_objects_with_literals() {
        assert_eq!(
            fix_json(r#"{"key": null, "key2": {"subKey":"#),
            r#"{"key": null, "key2": {}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_arrays_within_objects() {
        assert_eq!(fix_json(r#"{"key": [1, 2, {"#), r#"{"key": [1, 2, {}]}"#);
    }

    #[test]
    fn fix_json_upstream_handles_objects_within_arrays() {
        assert_eq!(
            fix_json(r#"[1, 2, {"key": "value","#),
            r#"[1, 2, {"key": "value"}]"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_nested_arrays_and_objects() {
        assert_eq!(
            fix_json(r#"{"a": {"b": ["c", {"d": "e","#),
            r#"{"a": {"b": ["c", {"d": "e"}]}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_deeply_nested_objects() {
        assert_eq!(
            fix_json(r#"{"a": {"b": {"c": {"d":"#),
            r#"{"a": {"b": {"c": {}}}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_potential_nested_arrays_or_objects() {
        assert_eq!(fix_json(r#"{"a": 1, "b": ["#), r#"{"a": 1, "b": []}"#);
        assert_eq!(fix_json(r#"{"a": 1, "b": {"#), r#"{"a": 1, "b": {}}"#);
        assert_eq!(fix_json(r#"{"a": 1, "b": ""#), r#"{"a": 1, "b": ""}"#);
    }

    #[test]
    fn fix_json_upstream_handles_complex_nesting_1() {
        assert_eq!(
            fix_json(
                [
                    "{",
                    r#"  "a": ["#,
                    "    {",
                    r#"      "a1": "v1","#,
                    r#"      "a2": "v2","#,
                    r#"      "a3": "v3""#,
                    "    }",
                    "  ],",
                    r#"  "b": ["#,
                    "    {",
                    r#"      "b1": "n"#,
                ]
                .join("\n")
                .as_str()
            ),
            [
                "{",
                r#"  "a": ["#,
                "    {",
                r#"      "a1": "v1","#,
                r#"      "a2": "v2","#,
                r#"      "a3": "v3""#,
                "    }",
                "  ],",
                r#"  "b": ["#,
                "    {",
                r#"      "b1": "n"}]}"#,
            ]
            .join("\n")
        );
    }

    #[test]
    fn fix_json_upstream_handles_empty_objects_inside_nested_objects_and_arrays() {
        assert_eq!(
            fix_json(r#"{"type":"div","children":[{"type":"Card","props":{}"#),
            r#"{"type":"div","children":[{"type":"Card","props":{}}]}"#
        );
    }

    #[test]
    fn invalid_argument_error_retains_parameter_value_and_message() {
        let error = InvalidArgumentError::new("messages", json!(null), "messages are required");

        assert_eq!(error.parameter(), "messages");
        assert_eq!(error.value(), &json!(null));
        assert_eq!(
            error.message(),
            "Invalid argument for parameter messages: messages are required"
        );
        assert_eq!(error.to_string(), error.message());

        let (parameter, value, message) = error.into_parts();
        assert_eq!(parameter, "messages");
        assert_eq!(value, json!(null));
        assert_eq!(
            message,
            "Invalid argument for parameter messages: messages are required"
        );
    }

    #[test]
    fn cosine_similarity_should_calculate_cosine_similarity_correctly() {
        let result = cosine_similarity(&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]).unwrap();

        assert_close(result, 0.974_631_846_197_076_2);
    }

    #[test]
    fn cosine_similarity_should_calculate_negative_cosine_similarity_correctly() {
        let result = cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]).unwrap();

        assert_close(result, -1.0);
    }

    #[test]
    fn cosine_similarity_should_throw_error_when_vectors_have_different_lengths() {
        let error = cosine_similarity(&[1.0, 2.0, 3.0], &[4.0, 5.0]).unwrap_err();

        assert_eq!(error.parameter(), "vector1,vector2");
        assert_eq!(
            error.value(),
            &json!({ "vector1Length": 3, "vector2Length": 2 })
        );
        assert_eq!(
            error.to_string(),
            "Invalid argument for parameter vector1,vector2: Vectors must have the same length"
        );
    }

    #[test]
    fn cosine_similarity_returns_zero_for_empty_vectors() {
        assert_eq!(cosine_similarity(&[], &[]).unwrap(), 0.0);
    }

    #[test]
    fn cosine_similarity_should_give_zero_when_one_of_the_vectors_is_a_zero_vector() {
        assert_eq!(
            cosine_similarity(&[0.0, 1.0, 2.0], &[0.0, 0.0, 0.0]).unwrap(),
            0.0
        );
        assert_eq!(
            cosine_similarity(&[0.0, 0.0, 0.0], &[0.0, 1.0, 2.0]).unwrap(),
            0.0
        );
    }

    #[test]
    fn cosine_similarity_should_handle_vectors_with_very_small_magnitudes() {
        let result = cosine_similarity(&[1e-10, 0.0, 0.0], &[2e-10, 0.0, 0.0]).unwrap();
        let negative = cosine_similarity(&[1e-10, 0.0, 0.0], &[-1e-10, 0.0, 0.0]).unwrap();

        assert_close(result, 1.0);
        assert_close(negative, -1.0);
    }

    #[test]
    fn parse_partial_json_returns_undefined_input_for_missing_text() {
        let result = parse_partial_json(None);

        assert_eq!(result.value(), None);
        assert_eq!(result.state(), super::ParsePartialJsonState::UndefinedInput);
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({ "state": "undefined-input" })
        );
    }

    #[test]
    fn parse_partial_json_returns_successful_parse_for_valid_json() {
        let result = parse_partial_json(Some(r#"{"foo":"bar","items":[1,true,null]}"#));

        assert_eq!(
            result.value(),
            Some(&json!({
                "foo": "bar",
                "items": [1, true, null],
            }))
        );
        assert_eq!(
            result.state(),
            super::ParsePartialJsonState::SuccessfulParse
        );
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({
                "value": {
                    "foo": "bar",
                    "items": [1, true, null],
                },
                "state": "successful-parse",
            })
        );
    }

    #[test]
    fn parse_partial_json_repairs_incomplete_literals_strings_arrays_and_objects() {
        assert_eq!(
            parse_partial_json(Some("nul")),
            super::ParsePartialJsonResult::repaired_parse(json!(null))
        );
        assert_eq!(
            parse_partial_json(Some(r#""abc"#)),
            super::ParsePartialJsonResult::repaired_parse(json!("abc"))
        );
        assert_eq!(
            parse_partial_json(Some("[[false], [nu")),
            super::ParsePartialJsonResult::repaired_parse(json!([[false], [null]]))
        );
        assert_eq!(
            parse_partial_json(Some(r#"{"k1": 1, "k2":"#)),
            super::ParsePartialJsonResult::repaired_parse(json!({ "k1": 1 }))
        );
    }

    #[test]
    fn parse_partial_json_trims_incomplete_numbers_before_repair_parse() {
        assert_eq!(
            parse_partial_json(Some("12.")),
            super::ParsePartialJsonResult::repaired_parse(json!(12))
        );
        assert_eq!(
            parse_partial_json(Some("2.5e-")),
            super::ParsePartialJsonResult::repaired_parse(json!(2.5))
        );
        assert_eq!(
            parse_partial_json(Some("-")),
            super::ParsePartialJsonResult::failed_parse()
        );
    }

    #[test]
    fn parse_partial_json_returns_failed_parse_when_repair_cannot_make_valid_json() {
        let result = parse_partial_json(Some("not json"));

        assert_eq!(result.value(), None);
        assert_eq!(result.state(), super::ParsePartialJsonState::FailedParse);
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({ "state": "failed-parse" })
        );
    }

    #[test]
    fn parse_partial_json_deserializes_state_shape() {
        let result: super::ParsePartialJsonResult = serde_json::from_value(json!({
            "value": { "partial": true },
            "state": "repaired-parse",
        }))
        .unwrap();

        assert_eq!(result.value(), Some(&json!({ "partial": true })));
        assert_eq!(result.state(), super::ParsePartialJsonState::RepairedParse);
    }

    #[test]
    fn get_text_from_data_url_decodes_base64_text() {
        let text = get_text_from_data_url("data:text/plain;base64,SGVsbG8h").unwrap();

        assert_eq!(text, "Hello!");
    }

    #[test]
    fn get_text_from_data_url_accepts_empty_payloads() {
        let text = get_text_from_data_url("data:text/plain;base64,").unwrap();

        assert_eq!(text, "");
    }

    #[test]
    fn get_text_from_data_url_uses_atob_style_byte_mapping() {
        let text = get_text_from_data_url("data:text/plain;base64,/w==").unwrap();

        assert_eq!(text, "ÿ");
    }

    #[test]
    fn get_text_from_data_url_rejects_missing_payload_or_media_type() {
        assert_eq!(
            get_text_from_data_url("data:text/plain;base64").unwrap_err(),
            DataUrlTextError::InvalidFormat
        );
        assert_eq!(
            get_text_from_data_url("text/plain;base64,SGVsbG8=").unwrap_err(),
            DataUrlTextError::InvalidFormat
        );
        assert_eq!(
            DataUrlTextError::InvalidFormat.to_string(),
            "Invalid data URL format"
        );
    }

    #[test]
    fn get_text_from_data_url_rejects_invalid_base64_payloads() {
        let error = get_text_from_data_url("data:text/plain;base64,%").unwrap_err();

        assert_eq!(error, DataUrlTextError::Decode);
        assert_eq!(error.to_string(), "Error decoding data URL");
    }
}
