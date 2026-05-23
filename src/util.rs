use std::collections::VecDeque;
use std::fmt;
use std::future::Future;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::task::{Context, Poll};
use std::thread;
use std::time::Duration;

use url::Url;

use crate::VERSION;
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::language_model::{LanguageModelAbortController, LanguageModelAbortSignal};
use crate::provider_utils::{
    DownloadBlobOptions, DownloadBlobResponse, DownloadError, DownloadedBlob, ParseJsonResult,
    convert_base64_to_bytes, download_blob, normalize_headers, read_response_with_size_limit,
    safe_parse_json, validate_download_url, with_user_agent_suffix,
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

/// Options accepted by high-level AI download helpers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CreateDownloadOptions {
    max_bytes: Option<usize>,
}

impl CreateDownloadOptions {
    /// Creates download factory options with upstream defaults.
    pub const fn new() -> Self {
        Self { max_bytes: None }
    }

    /// Sets the maximum accepted download size in bytes.
    pub const fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = Some(max_bytes);
        self
    }

    /// Returns the configured byte limit.
    pub const fn max_bytes(&self) -> Option<usize> {
        self.max_bytes
    }
}

/// Options accepted by [`download`] and [`download_with_transport`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadUrlOptions {
    url: Url,
    max_bytes: Option<usize>,
    abort_signal: Option<LanguageModelAbortSignal>,
}

impl DownloadUrlOptions {
    /// Creates download options for a URL.
    pub fn new(url: Url) -> Self {
        Self {
            url,
            max_bytes: None,
            abort_signal: None,
        }
    }

    /// Sets the maximum accepted download size in bytes.
    pub const fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = Some(max_bytes);
        self
    }

    /// Sets the abort signal supplied by the caller.
    pub fn with_abort_signal(mut self, abort_signal: LanguageModelAbortSignal) -> Self {
        self.abort_signal = Some(abort_signal);
        self
    }

    /// Sets the optional abort signal supplied by the caller.
    pub fn with_optional_abort_signal(
        mut self,
        abort_signal: Option<LanguageModelAbortSignal>,
    ) -> Self {
        self.abort_signal = abort_signal;
        self
    }

    /// Returns the URL to download.
    pub const fn url(&self) -> &Url {
        &self.url
    }

    /// Returns the configured byte limit.
    pub const fn max_bytes(&self) -> Option<usize> {
        self.max_bytes
    }

    /// Returns the abort signal.
    pub const fn abort_signal(&self) -> Option<&LanguageModelAbortSignal> {
        self.abort_signal.as_ref()
    }
}

/// Request passed to an injected high-level download transport.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DownloadTransportRequest {
    /// URL to fetch.
    pub url: String,

    /// Headers prepared by the high-level AI SDK download helper.
    pub headers: Headers,

    /// Optional abort signal propagated to the transport boundary.
    pub abort_signal: Option<LanguageModelAbortSignal>,
}

/// Reusable high-level download function created by [`create_download`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DownloadFunction {
    max_bytes: Option<usize>,
}

impl DownloadFunction {
    /// Creates a download function with upstream defaults.
    pub const fn new() -> Self {
        Self { max_bytes: None }
    }

    /// Creates a download function from factory options.
    pub const fn with_options(options: CreateDownloadOptions) -> Self {
        Self {
            max_bytes: options.max_bytes,
        }
    }

    /// Returns the configured byte limit.
    pub const fn max_bytes(&self) -> Option<usize> {
        self.max_bytes
    }

    /// Downloads the supplied URL using the default transport.
    pub async fn download(
        &self,
        url: Url,
        abort_signal: Option<LanguageModelAbortSignal>,
    ) -> Result<DownloadedBlob, DownloadError> {
        let mut options = DownloadUrlOptions::new(url).with_optional_abort_signal(abort_signal);
        options.max_bytes = self.max_bytes;
        download(options).await
    }
}

/// Creates a high-level AI SDK download function.
pub const fn create_download(options: CreateDownloadOptions) -> DownloadFunction {
    DownloadFunction::with_options(options)
}

/// Downloads a URL with the default blocking HTTP transport.
pub async fn download(options: DownloadUrlOptions) -> Result<DownloadedBlob, DownloadError> {
    download_with_transport(options, |request| {
        std::future::ready(execute_download_request(request))
    })
    .await
}

/// Downloads a URL using an injected transport.
pub async fn download_with_transport<Transport, TransportFuture>(
    options: DownloadUrlOptions,
    transport: Transport,
) -> Result<DownloadedBlob, DownloadError>
where
    Transport: FnOnce(DownloadTransportRequest) -> TransportFuture,
    TransportFuture: Future<Output = Result<DownloadBlobResponse, DownloadError>>,
{
    let DownloadUrlOptions {
        url,
        max_bytes,
        abort_signal,
    } = options;
    let url_text = url.to_string();

    if url.scheme() == "data" {
        return download_data_url(&url_text, max_bytes);
    }

    let request_headers = download_request_headers();
    download_blob(
        DownloadBlobOptions {
            url: url_text,
            max_bytes,
        },
        move |validated_url| {
            transport(DownloadTransportRequest {
                url: validated_url.to_string(),
                headers: request_headers,
                abort_signal,
            })
        },
    )
    .await
}

fn download_request_headers() -> Headers {
    with_user_agent_suffix(
        Some(Vec::<(String, Option<String>)>::new()),
        [format!("ai-sdk/{VERSION}")],
    )
}

fn download_data_url(
    url_text: &str,
    max_bytes: Option<usize>,
) -> Result<DownloadedBlob, DownloadError> {
    validate_download_url(url_text)?;

    let (header, payload) = url_text
        .split_once(',')
        .ok_or_else(|| DownloadError::new(url_text, "Invalid data URL format"))?;
    let header = header
        .strip_prefix("data:")
        .ok_or_else(|| DownloadError::new(url_text, "Invalid data URL format"))?;
    let mut header_parts = header.split(';');
    let media_type = header_parts
        .next()
        .filter(|media_type| !media_type.is_empty())
        .map(str::to_string);
    let is_base64 = header_parts.any(|part| part.eq_ignore_ascii_case("base64"));
    let data = if is_base64 {
        convert_base64_to_bytes(payload)
            .map_err(|_| DownloadError::new(url_text, "Invalid data URL base64 payload"))?
    } else {
        percent_decode_data_url_payload(payload)
            .ok_or_else(|| DownloadError::new(url_text, "Invalid data URL percent encoding"))?
    };
    let data =
        read_response_with_size_limit(url_text, std::iter::once(data.as_slice()), None, max_bytes)?;
    let mut blob = DownloadedBlob::new(data);

    if let Some(media_type) = media_type {
        blob = blob.with_media_type(media_type);
    }

    Ok(blob)
}

fn percent_decode_data_url_payload(payload: &str) -> Option<Vec<u8>> {
    let mut decoded = Vec::with_capacity(payload.len());
    let mut bytes = payload.as_bytes().iter().copied();

    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes.next().and_then(hex_value)?;
            let low = bytes.next().and_then(hex_value)?;
            decoded.push((high << 4) | low);
        } else {
            decoded.push(byte);
        }
    }

    Some(decoded)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn execute_download_request(
    request: DownloadTransportRequest,
) -> Result<DownloadBlobResponse, DownloadError> {
    if request
        .abort_signal
        .as_ref()
        .is_some_and(LanguageModelAbortSignal::is_aborted)
    {
        return Err(DownloadError::with_cause_message(
            request.url,
            "The operation was aborted.",
        ));
    }

    let mut builder = ureq::get(&request.url);

    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }

    let mut response = builder
        .config()
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|error| DownloadError::with_cause_message(&request.url, error.to_string()))?;
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or("").to_string();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect::<Headers>();
    let body = response
        .body_mut()
        .read_to_vec()
        .map_err(|error| DownloadError::with_cause_message(&request.url, error.to_string()))?;

    Ok(DownloadBlobResponse::bytes(status.as_u16(), status_text, body).with_headers(headers))
}

/// Error returned when an async-iterable stream read or cancellation fails.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsyncIterableStreamError {
    message: String,
}

impl AsyncIterableStreamError {
    /// Creates an async-iterable stream error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<&str> for AsyncIterableStreamError {
    fn from(message: &str) -> Self {
        Self::new(message)
    }
}

impl From<String> for AsyncIterableStreamError {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

impl fmt::Display for AsyncIterableStreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AsyncIterableStreamError {}

/// Source consumed by [`AsyncIterableStream`].
pub trait AsyncIterableStreamSource<T> {
    /// Source-specific error.
    type Error: fmt::Display;

    /// Reads the next chunk, returning `None` after the source completes.
    fn read(&mut self) -> Result<Option<T>, Self::Error>;

    /// Cancels the source when stream consumption exits early.
    fn cancel(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// Vector-backed source for [`create_async_iterable_stream`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VecAsyncIterableStreamSource<T> {
    chunks: VecDeque<T>,
    cancelled: bool,
}

impl<T> VecAsyncIterableStreamSource<T> {
    /// Creates a vector-backed async-iterable stream source.
    pub fn new(chunks: Vec<T>) -> Self {
        Self {
            chunks: chunks.into(),
            cancelled: false,
        }
    }

    /// Returns whether the source has been cancelled.
    pub const fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

impl<T> AsyncIterableStreamSource<T> for VecAsyncIterableStreamSource<T> {
    type Error = AsyncIterableStreamError;

    fn read(&mut self) -> Result<Option<T>, Self::Error> {
        Ok(self.chunks.pop_front())
    }

    fn cancel(&mut self) -> Result<(), Self::Error> {
        self.cancelled = true;
        self.chunks.clear();
        Ok(())
    }
}

/// Rust analogue of upstream `createAsyncIterableStream`.
///
/// Upstream combines a Web `ReadableStream` with JavaScript async iteration.
/// Rust exposes the portable contract through direct reads plus an iterator
/// facade, while preserving completion, cancellation, and error behavior.
pub struct AsyncIterableStream<T, Source> {
    source: Source,
    finished: bool,
    pending_cancel_error: Option<String>,
    _chunk: PhantomData<T>,
}

impl<T, Source> fmt::Debug for AsyncIterableStream<T, Source>
where
    Source: fmt::Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AsyncIterableStream")
            .field("source", &self.source)
            .field("finished", &self.finished)
            .field("pending_cancel_error", &self.pending_cancel_error)
            .finish()
    }
}

impl<T, Source> AsyncIterableStream<T, Source>
where
    Source: AsyncIterableStreamSource<T>,
{
    /// Creates an async-iterable stream from a source.
    pub fn new(source: Source) -> Self {
        Self {
            source,
            finished: false,
            pending_cancel_error: None,
            _chunk: PhantomData,
        }
    }

    /// Returns the underlying source.
    pub fn source(&self) -> &Source {
        &self.source
    }

    /// Reads the next chunk directly from the stream.
    pub fn read(&mut self) -> Result<Option<T>, AsyncIterableStreamError> {
        if let Some(message) = self.pending_cancel_error.take() {
            return Err(AsyncIterableStreamError::new(message));
        }

        if self.finished {
            return Ok(None);
        }

        match self.source.read() {
            Ok(Some(chunk)) => Ok(Some(chunk)),
            Ok(None) => {
                self.finished = true;
                Ok(None)
            }
            Err(error) => {
                self.finished = true;
                Err(AsyncIterableStreamError::new(error.to_string()))
            }
        }
    }

    /// Collects all remaining chunks through the readable-stream facade.
    pub fn collect(&mut self) -> Result<Vec<T>, AsyncIterableStreamError> {
        let mut chunks = Vec::new();

        while let Some(chunk) = self.read()? {
            chunks.push(chunk);
        }

        Ok(chunks)
    }

    /// Cancels the stream without causing subsequent iteration to error.
    pub fn cancel(&mut self) -> Result<(), AsyncIterableStreamError> {
        self.cancel_inner(None)
    }

    /// Cancels the stream and makes the active iterator observe an error.
    pub fn cancel_with_reason(
        &mut self,
        reason: impl Into<String>,
    ) -> Result<(), AsyncIterableStreamError> {
        self.cancel_inner(Some(reason.into()))
    }

    fn cancel_inner(&mut self, reason: Option<String>) -> Result<(), AsyncIterableStreamError> {
        if !self.finished {
            self.source
                .cancel()
                .map_err(|error| AsyncIterableStreamError::new(error.to_string()))?;
            self.finished = true;
        }

        if let Some(reason) = reason {
            self.pending_cancel_error = Some(reason);
        }

        Ok(())
    }

    /// Creates an async-iteration facade over this stream.
    pub fn iter(&mut self) -> AsyncIterableStreamIterator<'_, T, Source> {
        AsyncIterableStreamIterator { stream: self }
    }
}

/// Iterator facade returned by [`AsyncIterableStream::iter`].
pub struct AsyncIterableStreamIterator<'a, T, Source>
where
    Source: AsyncIterableStreamSource<T>,
{
    stream: &'a mut AsyncIterableStream<T, Source>,
}

impl<T, Source> AsyncIterableStreamIterator<'_, T, Source>
where
    Source: AsyncIterableStreamSource<T>,
{
    /// Reads the next async-iterable chunk.
    pub fn read_next(&mut self) -> Result<Option<T>, AsyncIterableStreamError> {
        self.stream.read()
    }

    /// Mirrors async-iterator `return()` for early loop exit.
    pub fn return_stream(&mut self) -> Result<(), AsyncIterableStreamError> {
        self.stream.cancel()
    }

    /// Mirrors async-iterator `throw()` for exceptional loop exit.
    pub fn throw(
        &mut self,
        error: impl Into<AsyncIterableStreamError>,
    ) -> Result<(), AsyncIterableStreamError> {
        self.stream.cancel()?;
        Err(error.into())
    }
}

/// Creates an async-iterable stream from vector chunks.
pub fn create_async_iterable_stream<T>(
    chunks: Vec<T>,
) -> AsyncIterableStream<T, VecAsyncIterableStreamSource<T>> {
    create_async_iterable_stream_from_source(VecAsyncIterableStreamSource::new(chunks))
}

/// Creates an async-iterable stream from an injected source.
pub fn create_async_iterable_stream_from_source<T, Source>(
    source: Source,
) -> AsyncIterableStream<T, Source>
where
    Source: AsyncIterableStreamSource<T>,
{
    AsyncIterableStream::new(source)
}

/// Error returned when a stitchable stream operation fails.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StitchableStreamError {
    message: String,
}

impl StitchableStreamError {
    /// Creates a stitchable stream error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for StitchableStreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for StitchableStreamError {}

/// Read result returned by [`StitchableStream::read`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StitchableStreamRead<T> {
    /// No chunk is currently available because the outer stream is still open.
    Pending,

    /// One chunk was read from the current inner stream.
    Chunk(T),

    /// The outer stream is closed and all inner streams are exhausted.
    Done,
}

/// Rust analogue of upstream `createStitchableStream`.
pub struct StitchableStream<T, Source> {
    inner_streams: VecDeque<Source>,
    closed: bool,
    _chunk: PhantomData<T>,
}

impl<T, Source> fmt::Debug for StitchableStream<T, Source>
where
    Source: fmt::Debug,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StitchableStream")
            .field("inner_stream_count", &self.inner_streams.len())
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}

impl<T, Source> Default for StitchableStream<T, Source>
where
    Source: AsyncIterableStreamSource<T>,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T, Source> StitchableStream<T, Source>
where
    Source: AsyncIterableStreamSource<T>,
{
    /// Creates an empty stitchable stream.
    pub fn new() -> Self {
        Self {
            inner_streams: VecDeque::new(),
            closed: false,
            _chunk: PhantomData,
        }
    }

    /// Adds an inner stream to the read queue.
    pub fn add_stream(&mut self, inner_stream: Source) -> Result<(), StitchableStreamError> {
        if self.closed {
            return Err(StitchableStreamError::new(
                "Cannot add inner stream: outer stream is closed",
            ));
        }

        self.inner_streams.push_back(inner_stream);
        Ok(())
    }

    /// Gracefully closes the outer stream after queued inner streams finish.
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// Cancels all inner streams and closes the outer stream immediately.
    pub fn terminate(&mut self) -> Result<(), StitchableStreamError> {
        self.closed = true;
        self.cancel_all_inner_streams()
    }

    /// Cancels the outer stream and all queued inner streams.
    pub fn cancel(&mut self) -> Result<(), StitchableStreamError> {
        self.closed = true;
        self.cancel_all_inner_streams()
    }

    /// Reads one value, reports `Pending` when still open with no inner stream,
    /// and reports `Done` once closed and drained.
    pub fn read(&mut self) -> Result<StitchableStreamRead<T>, StitchableStreamError> {
        loop {
            let Some(inner_stream) = self.inner_streams.front_mut() else {
                return if self.closed {
                    Ok(StitchableStreamRead::Done)
                } else {
                    Ok(StitchableStreamRead::Pending)
                };
            };

            match inner_stream.read() {
                Ok(Some(chunk)) => return Ok(StitchableStreamRead::Chunk(chunk)),
                Ok(None) => {
                    self.inner_streams.pop_front();
                }
                Err(error) => {
                    let message = error.to_string();
                    let _ = self.terminate();
                    return Err(StitchableStreamError::new(message));
                }
            }
        }
    }

    /// Collects all currently readable chunks until the stream is done or pending.
    pub fn collect(&mut self) -> Result<Vec<T>, StitchableStreamError> {
        let mut chunks = Vec::new();

        while let StitchableStreamRead::Chunk(chunk) = self.read()? {
            chunks.push(chunk);
        }

        Ok(chunks)
    }

    fn cancel_all_inner_streams(&mut self) -> Result<(), StitchableStreamError> {
        let mut first_error = None;

        while let Some(mut inner_stream) = self.inner_streams.pop_front() {
            if let Err(error) = inner_stream.cancel()
                && first_error.is_none()
            {
                first_error = Some(error.to_string());
            }
        }

        if let Some(error) = first_error {
            return Err(StitchableStreamError::new(error));
        }

        Ok(())
    }
}

impl<T> StitchableStream<T, VecAsyncIterableStreamSource<T>> {
    /// Adds vector chunks as one inner stream.
    pub fn add_chunks(&mut self, chunks: Vec<T>) -> Result<(), StitchableStreamError> {
        self.add_stream(VecAsyncIterableStreamSource::new(chunks))
    }
}

/// Creates a stitchable stream.
pub fn create_stitchable_stream<T, Source>() -> StitchableStream<T, Source>
where
    Source: AsyncIterableStreamSource<T>,
{
    StitchableStream::new()
}

/// Error returned when a simulated readable stream delay fails.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimulateReadableStreamError {
    message: String,
}

impl SimulateReadableStreamError {
    /// Creates a simulated stream error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for SimulateReadableStreamError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SimulateReadableStreamError {}

/// Result returned by a simulated readable stream delay hook.
pub type SimulateReadableStreamResult = Result<(), SimulateReadableStreamError>;

/// Delay function accepted by [`simulate_readable_stream_with_delay`].
pub type SimulateReadableStreamDelayFunction<'a> =
    dyn FnMut(Option<u64>) -> SimulateReadableStreamResult + 'a;

/// Options accepted by [`simulate_readable_stream`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SimulateReadableStreamOptions<T> {
    chunks: Vec<T>,
    initial_delay_in_ms: Option<u64>,
    chunk_delay_in_ms: Option<u64>,
}

impl<T> SimulateReadableStreamOptions<T> {
    /// Creates options with upstream defaults: zero-millisecond initial and chunk delays.
    pub fn new(chunks: Vec<T>) -> Self {
        Self {
            chunks,
            initial_delay_in_ms: Some(0),
            chunk_delay_in_ms: Some(0),
        }
    }

    /// Sets the initial delay before the first chunk.
    pub fn with_initial_delay_in_ms(mut self, delay_in_ms: u64) -> Self {
        self.initial_delay_in_ms = Some(delay_in_ms);
        self
    }

    /// Sets the inter-chunk delay after the first chunk.
    pub fn with_chunk_delay_in_ms(mut self, delay_in_ms: u64) -> Self {
        self.chunk_delay_in_ms = Some(delay_in_ms);
        self
    }

    /// Mirrors upstream `initialDelayInMs: null`.
    pub fn without_initial_delay(mut self) -> Self {
        self.initial_delay_in_ms = None;
        self
    }

    /// Mirrors upstream `chunkDelayInMs: null`.
    pub fn without_chunk_delay(mut self) -> Self {
        self.chunk_delay_in_ms = None;
        self
    }
}

/// Readable-stream style chunk simulator for deterministic tests and fixtures.
///
/// Upstream returns a Web `ReadableStream`. Rust exposes the portable pull
/// contract directly through [`SimulatedReadableStream::read`] and
/// [`SimulatedReadableStream::collect`], preserving chunk order and the
/// `null` versus zero-delay distinction through `Option<u64>`.
pub struct SimulatedReadableStream<'a, T> {
    chunks: VecDeque<T>,
    emitted_chunks: usize,
    initial_delay_in_ms: Option<u64>,
    chunk_delay_in_ms: Option<u64>,
    delay: Box<SimulateReadableStreamDelayFunction<'a>>,
}

impl<T> fmt::Debug for SimulatedReadableStream<'_, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SimulatedReadableStream")
            .field("remaining_chunks", &self.chunks.len())
            .field("emitted_chunks", &self.emitted_chunks)
            .field("initial_delay_in_ms", &self.initial_delay_in_ms)
            .field("chunk_delay_in_ms", &self.chunk_delay_in_ms)
            .finish_non_exhaustive()
    }
}

impl<'a, T> SimulatedReadableStream<'a, T> {
    fn new(
        options: SimulateReadableStreamOptions<T>,
        delay: impl FnMut(Option<u64>) -> SimulateReadableStreamResult + 'a,
    ) -> Self {
        Self {
            chunks: options.chunks.into(),
            emitted_chunks: 0,
            initial_delay_in_ms: options.initial_delay_in_ms,
            chunk_delay_in_ms: options.chunk_delay_in_ms,
            delay: Box::new(delay),
        }
    }

    /// Reads the next chunk, returning `None` once the stream is closed.
    pub fn read(&mut self) -> Result<Option<T>, SimulateReadableStreamError> {
        let Some(chunk) = self.chunks.pop_front() else {
            return Ok(None);
        };

        let delay_in_ms = if self.emitted_chunks == 0 {
            self.initial_delay_in_ms
        } else {
            self.chunk_delay_in_ms
        };
        (self.delay)(delay_in_ms)?;
        self.emitted_chunks += 1;

        Ok(Some(chunk))
    }

    /// Collects all remaining chunks from the stream.
    pub fn collect(mut self) -> Result<Vec<T>, SimulateReadableStreamError> {
        let mut chunks = Vec::new();

        while let Some(chunk) = self.read()? {
            chunks.push(chunk);
        }

        Ok(chunks)
    }
}

/// Creates a simulated readable stream using the default sleep-based delay hook.
pub fn simulate_readable_stream<T>(
    options: SimulateReadableStreamOptions<T>,
) -> SimulatedReadableStream<'static, T> {
    simulate_readable_stream_with_delay(options, default_simulate_readable_stream_delay)
}

/// Creates a simulated readable stream with an injected delay hook.
pub fn simulate_readable_stream_with_delay<'a, T>(
    options: SimulateReadableStreamOptions<T>,
    delay: impl FnMut(Option<u64>) -> SimulateReadableStreamResult + 'a,
) -> SimulatedReadableStream<'a, T> {
    SimulatedReadableStream::new(options, delay)
}

fn default_simulate_readable_stream_delay(
    delay_in_ms: Option<u64>,
) -> SimulateReadableStreamResult {
    if let Some(delay_in_ms) = delay_in_ms {
        thread::sleep(Duration::from_millis(delay_in_ms));
    }

    Ok(())
}

/// Options accepted by [`write_to_server_response`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriteToServerResponseOptions {
    stream: Vec<Vec<u8>>,
    status: Option<u16>,
    status_text: Option<String>,
    headers: Headers,
}

impl WriteToServerResponseOptions {
    /// Creates response-writing options with the supplied byte chunks.
    pub fn new(stream: Vec<Vec<u8>>) -> Self {
        Self {
            stream,
            status: None,
            status_text: None,
            headers: Headers::new(),
        }
    }

    /// Sets the HTTP status code.
    pub const fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    /// Sets the optional HTTP status text.
    pub fn with_status_text(mut self, status_text: impl Into<String>) -> Self {
        self.status_text = Some(status_text.into());
        self
    }

    /// Sets all response headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = headers;
        self
    }

    /// Adds one response header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

/// Minimal writer abstraction used by [`write_to_server_response`].
///
/// It mirrors the portable contract of Node's `ServerResponse`: write status
/// and headers once, write byte chunks, wait for drain when a write reports
/// backpressure, then end the response.
pub trait ServerResponseWriter {
    /// Error type returned by the response writer.
    type Error;

    /// Writes the response status and headers.
    fn write_head(
        &mut self,
        status: u16,
        status_text: Option<&str>,
        headers: &Headers,
    ) -> Result<(), Self::Error>;

    /// Writes one response chunk, returning whether more writes can continue.
    fn write_chunk(&mut self, chunk: &[u8]) -> Result<bool, Self::Error>;

    /// Waits for the response writer to become writable after backpressure.
    fn wait_for_drain(&mut self) -> Result<(), Self::Error>;

    /// Finalizes the response.
    fn end(&mut self) -> Result<(), Self::Error>;
}

/// Writes byte chunks to a server-response writer.
///
/// This mirrors upstream `writeToServerResponse` without depending on Node's
/// concrete `ServerResponse` type. Status defaults to 200, headers are passed
/// through unchanged, and write backpressure is respected before the next chunk.
pub fn write_to_server_response<W>(
    response: &mut W,
    options: WriteToServerResponseOptions,
) -> Result<(), W::Error>
where
    W: ServerResponseWriter,
{
    let WriteToServerResponseOptions {
        stream,
        status,
        status_text,
        headers,
    } = options;
    let status = status.unwrap_or(200);

    response.write_head(status, status_text.as_deref(), &headers)?;

    for chunk in stream {
        if !response.write_chunk(&chunk)? {
            response.wait_for_drain()?;
        }
    }

    response.end()
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
    use std::collections::VecDeque;
    use std::future::{Future, ready};
    use std::pin::Pin;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex, mpsc};
    use std::task::{Context, Poll, Wake, Waker};
    use std::time::{Duration, Instant};

    use super::{
        AbortSignalSource, AbortTimeoutOptions, AsyncIterableStream, AsyncIterableStreamError,
        AsyncIterableStreamSource, Callback, CallbackResult, DataUrlTextError,
        DownloadTransportRequest, DownloadUrlOptions, InvalidArgumentError, NotifyCallbacks,
        PrepareRetriesOptions, SerialJobError, SerialJobExecutor, ServerResponseWriter,
        SimulateReadableStreamOptions, StitchableStream, StitchableStreamRead,
        VecAsyncIterableStreamSource, WriteToServerResponseOptions, cosine_similarity,
        create_async_iterable_stream, create_async_iterable_stream_from_source,
        create_stitchable_stream, download_with_transport, fix_json, get_potential_start_index,
        get_text_from_data_url, is_deep_equal_data, merge_abort_signals, merge_callbacks,
        merge_objects, notify, parse_partial_json, prepare_headers, prepare_retries,
        set_abort_timeout, simulate_readable_stream, simulate_readable_stream_with_delay,
        split_array, write_to_server_response,
    };
    use crate::headers::Headers;
    use crate::json::JsonValue;
    use crate::language_model::{LanguageModelAbortController, LanguageModelAbortSignal};
    use crate::provider_utils::{DownloadBlobResponse, DownloadError};
    use url::Url;

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
    struct MockServerResponse {
        status_code: u16,
        status_message: Option<String>,
        headers: Headers,
        written_chunks: Vec<Vec<u8>>,
        ended: bool,
        write_responses: VecDeque<bool>,
        drain_wait_count: usize,
        events: Vec<String>,
    }

    impl MockServerResponse {
        fn with_write_responses(write_responses: impl IntoIterator<Item = bool>) -> Self {
            Self {
                write_responses: write_responses.into_iter().collect(),
                ..Self::default()
            }
        }
    }

    impl ServerResponseWriter for MockServerResponse {
        type Error = String;

        fn write_head(
            &mut self,
            status: u16,
            status_text: Option<&str>,
            headers: &Headers,
        ) -> Result<(), Self::Error> {
            self.status_code = status;
            self.status_message = status_text.map(str::to_string);
            self.headers = headers.clone();
            Ok(())
        }

        fn write_chunk(&mut self, chunk: &[u8]) -> Result<bool, Self::Error> {
            self.written_chunks.push(chunk.to_vec());
            self.events.push(format!(
                "write:{}",
                String::from_utf8_lossy(chunk).into_owned()
            ));
            Ok(self.write_responses.pop_front().unwrap_or(true))
        }

        fn wait_for_drain(&mut self) -> Result<(), Self::Error> {
            self.drain_wait_count += 1;
            self.events.push("drain".to_string());
            Ok(())
        }

        fn end(&mut self) -> Result<(), Self::Error> {
            self.ended = true;
            self.events.push("end".to_string());
            Ok(())
        }
    }

    struct MockAsyncIterableStreamSource<T> {
        chunks: VecDeque<Result<T, String>>,
        cancelled: Rc<RefCell<bool>>,
    }

    impl<T> MockAsyncIterableStreamSource<T> {
        fn with_entries(entries: impl IntoIterator<Item = Result<T, String>>) -> Self {
            Self {
                chunks: entries.into_iter().collect(),
                cancelled: Rc::new(RefCell::new(false)),
            }
        }

        fn with_chunks(chunks: impl IntoIterator<Item = T>) -> Self {
            Self::with_entries(chunks.into_iter().map(Ok))
        }

        fn cancelled_handle(&self) -> Rc<RefCell<bool>> {
            Rc::clone(&self.cancelled)
        }
    }

    impl<T> AsyncIterableStreamSource<T> for MockAsyncIterableStreamSource<T> {
        type Error = String;

        fn read(&mut self) -> Result<Option<T>, Self::Error> {
            match self.chunks.pop_front() {
                Some(Ok(chunk)) => Ok(Some(chunk)),
                Some(Err(error)) => Err(error),
                None => Ok(None),
            }
        }

        fn cancel(&mut self) -> Result<(), Self::Error> {
            *self.cancelled.borrow_mut() = true;
            self.chunks.clear();
            Ok(())
        }
    }

    fn collect_async_iterable_stream<T, Source>(
        stream: &mut AsyncIterableStream<T, Source>,
    ) -> Result<Vec<T>, AsyncIterableStreamError>
    where
        Source: AsyncIterableStreamSource<T>,
    {
        let mut iterator = stream.iter();
        let mut chunks = Vec::new();

        while let Some(chunk) = iterator.read_next()? {
            chunks.push(chunk);
        }

        Ok(chunks)
    }

    fn stitchable_stream<T>() -> StitchableStream<T, VecAsyncIterableStreamSource<T>> {
        create_stitchable_stream()
    }

    fn vec_inner_stream<T>(chunks: Vec<T>) -> VecAsyncIterableStreamSource<T> {
        VecAsyncIterableStreamSource::new(chunks)
    }

    fn download_url_options(url: &str) -> DownloadUrlOptions {
        DownloadUrlOptions::new(Url::parse(url).expect("valid test URL"))
    }

    fn download_response(
        status_code: u16,
        status_text: &str,
        body: impl Into<Vec<u8>>,
        media_type: Option<&str>,
    ) -> DownloadBlobResponse {
        let mut headers = Headers::new();
        if let Some(media_type) = media_type {
            headers.insert("content-type".to_string(), media_type.to_string());
        }

        DownloadBlobResponse::bytes(status_code, status_text, body).with_headers(headers)
    }

    fn expect_download_rejected_before_transport(url: &str) -> DownloadError {
        let transport_called = Rc::new(RefCell::new(false));
        let transport_called_for_request = Rc::clone(&transport_called);
        let error = poll_ready(download_with_transport(
            download_url_options(url),
            move |_request| {
                *transport_called_for_request.borrow_mut() = true;
                ready(Ok(download_response(200, "OK", Vec::<u8>::new(), None)))
            },
        ))
        .expect_err("download should fail before transport");

        assert!(!*transport_called.borrow());
        error
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

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = test_waker();
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);

        match Future::poll(future.as_mut(), &mut context) {
            Poll::Ready(result) => result,
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
    fn download_should_reject_private_ipv4_addresses() {
        for url in [
            "http://127.0.0.1/file",
            "http://10.0.0.1/file",
            "http://169.254.169.254/latest/meta-data/",
        ] {
            let error = expect_download_rejected_before_transport(url);
            assert_eq!(error.name(), DownloadError::NAME);
        }
    }

    #[test]
    fn download_should_reject_localhost() {
        let error = expect_download_rejected_before_transport("http://localhost/file");

        assert_eq!(error.name(), DownloadError::NAME);
    }

    #[test]
    fn download_should_reject_redirects_to_private_ip_addresses() {
        let error = poll_ready(download_with_transport(
            download_url_options("https://evil.com/redirect"),
            |_request| {
                ready(Ok(download_response(
                    200,
                    "OK",
                    b"secret".to_vec(),
                    Some("text/plain"),
                )
                .with_final_url("http://169.254.169.254/latest/meta-data/")))
            },
        ))
        .expect_err("private redirect is rejected");

        assert_eq!(error.name(), DownloadError::NAME);
    }

    #[test]
    fn download_should_reject_redirects_to_localhost() {
        let error = poll_ready(download_with_transport(
            download_url_options("https://evil.com/redirect"),
            |_request| {
                ready(Ok(download_response(
                    200,
                    "OK",
                    b"secret".to_vec(),
                    Some("text/plain"),
                )
                .with_final_url("http://localhost:8080/admin")))
            },
        ))
        .expect_err("localhost redirect is rejected");

        assert_eq!(error.name(), DownloadError::NAME);
    }

    #[test]
    fn download_should_allow_redirects_to_safe_urls() {
        let content = vec![1, 2, 3];
        let result = poll_ready(download_with_transport(
            download_url_options("https://example.com/image.png"),
            {
                let content = content.clone();
                move |_request| {
                    ready(Ok(download_response(200, "OK", content, Some("image/png"))
                        .with_final_url("https://cdn.example.com/image.png")))
                }
            },
        ))
        .expect("safe redirect downloads");

        assert_eq!(result.data, vec![1, 2, 3]);
        assert_eq!(result.media_type.as_deref(), Some("image/png"));
    }

    #[test]
    fn download_should_download_data_successfully_and_match_expected_bytes() {
        let expected_bytes = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let captured_request = Rc::new(RefCell::new(None::<DownloadTransportRequest>));
        let captured_request_for_transport = Rc::clone(&captured_request);
        let result = poll_ready(download_with_transport(
            download_url_options("http://example.com/file"),
            {
                let expected_bytes = expected_bytes.clone();
                move |request| {
                    *captured_request_for_transport.borrow_mut() = Some(request);
                    ready(Ok(download_response(
                        200,
                        "OK",
                        expected_bytes,
                        Some("application/octet-stream"),
                    )))
                }
            },
        ))
        .expect("download succeeds");

        assert_eq!(result.data, expected_bytes);
        assert_eq!(
            result.media_type.as_deref(),
            Some("application/octet-stream")
        );
        let request = captured_request
            .borrow()
            .clone()
            .expect("transport was called");
        assert_eq!(request.url, "http://example.com/file");
        assert!(
            request
                .headers
                .get("user-agent")
                .is_some_and(|value| value.contains("ai-sdk/"))
        );
    }

    #[test]
    fn download_should_allow_inline_data_urls() {
        let transport_called = Rc::new(RefCell::new(false));
        let transport_called_for_request = Rc::clone(&transport_called);
        let result = poll_ready(download_with_transport(
            download_url_options("data:text/plain;base64,aGVsbG8="),
            move |_request| {
                *transport_called_for_request.borrow_mut() = true;
                ready(Ok(download_response(200, "OK", Vec::<u8>::new(), None)))
            },
        ))
        .expect("data URL downloads");

        assert_eq!(result.data, b"hello".to_vec());
        assert_eq!(result.media_type.as_deref(), Some("text/plain"));
        assert!(!*transport_called.borrow());
    }

    #[test]
    fn download_should_throw_download_error_when_response_is_not_ok() {
        let error = poll_ready(download_with_transport(
            download_url_options("http://example.com/file"),
            |_request| ready(Ok(DownloadBlobResponse::new(404, "Not Found"))),
        ))
        .expect_err("non-OK response errors");

        assert_eq!(error.name(), DownloadError::NAME);
        assert_eq!(error.status_code(), Some(404));
        assert_eq!(error.status_text(), Some("Not Found"));
    }

    #[test]
    fn download_should_throw_download_error_when_fetch_throws_an_error() {
        let error = poll_ready(download_with_transport(
            download_url_options("http://example.com/file"),
            |request| {
                ready(Err(DownloadError::with_cause_message(
                    request.url,
                    "Network error",
                )))
            },
        ))
        .expect_err("transport failure errors");

        assert_eq!(error.name(), DownloadError::NAME);
        assert_eq!(error.cause_message(), Some("Network error"));
    }

    #[test]
    fn download_should_abort_when_response_exceeds_default_size_limit() {
        let mut headers = Headers::new();
        headers.insert(
            "content-type".to_string(),
            "application/octet-stream".to_string(),
        );
        headers.insert(
            "content-length".to_string(),
            (3_u128 * 1024 * 1024 * 1024).to_string(),
        );
        let error = poll_ready(download_with_transport(
            download_url_options("http://example.com/large"),
            move |_request| {
                ready(Ok(DownloadBlobResponse::bytes(200, "OK", vec![0; 10])
                    .with_headers(headers.clone())))
            },
        ))
        .expect_err("oversized response errors");

        assert_eq!(error.name(), DownloadError::NAME);
        assert!(error.message().contains("exceeded maximum size"));
    }

    #[test]
    fn download_should_pass_abort_signal_to_fetch() {
        let abort_controller = LanguageModelAbortController::new();
        let signal = abort_controller.signal();
        abort_controller.abort();
        let captured_signal = Rc::new(RefCell::new(None::<LanguageModelAbortSignal>));
        let captured_signal_for_transport = Rc::clone(&captured_signal);
        let error = poll_ready(download_with_transport(
            download_url_options("http://example.com/file").with_abort_signal(signal.clone()),
            move |request| {
                *captured_signal_for_transport.borrow_mut() = request.abort_signal;
                ready(Err(DownloadError::with_cause_message(
                    request.url,
                    "The operation was aborted.",
                )))
            },
        ))
        .expect_err("aborted download errors");

        assert_eq!(error.name(), DownloadError::NAME);
        let captured_signal = captured_signal
            .borrow()
            .clone()
            .expect("transport received abort signal");
        assert!(captured_signal.is_aborted());
    }

    #[test]
    fn create_async_iterable_stream_should_read_all_chunks_from_a_non_empty_stream_using_async_iteration()
     {
        let mut stream = create_async_iterable_stream(vec!["chunk1", "chunk2", "chunk3"]);

        assert_eq!(
            collect_async_iterable_stream(&mut stream).unwrap(),
            vec!["chunk1", "chunk2", "chunk3"]
        );
    }

    #[test]
    fn create_async_iterable_stream_should_handle_an_empty_stream_gracefully() {
        let mut stream = create_async_iterable_stream::<String>(vec![]);

        assert_eq!(
            collect_async_iterable_stream(&mut stream).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn create_async_iterable_stream_should_maintain_readable_stream_functionality() {
        let mut stream = create_async_iterable_stream(vec!["chunk1", "chunk2", "chunk3"]);

        assert_eq!(
            stream.collect().unwrap(),
            vec!["chunk1", "chunk2", "chunk3"]
        );
    }

    #[test]
    fn create_async_iterable_stream_should_cancel_stream_on_early_exit_from_for_await_loop() {
        let source = MockAsyncIterableStreamSource::with_chunks(["chunk1", "chunk2", "chunk3"]);
        let cancelled = source.cancelled_handle();
        let mut stream = create_async_iterable_stream_from_source(source);

        let mut iterator = stream.iter();
        assert_eq!(iterator.read_next().unwrap(), Some("chunk1"));
        assert_eq!(iterator.read_next().unwrap(), Some("chunk2"));
        iterator.return_stream().unwrap();

        assert!(*cancelled.borrow());
    }

    #[test]
    fn create_async_iterable_stream_should_cancel_stream_when_exception_thrown_inside_for_await_loop()
     {
        let source = MockAsyncIterableStreamSource::with_chunks(["chunk1", "chunk2", "chunk3"]);
        let cancelled = source.cancelled_handle();
        let mut stream = create_async_iterable_stream_from_source(source);

        let mut iterator = stream.iter();
        assert_eq!(iterator.read_next().unwrap(), Some("chunk1"));
        assert_eq!(iterator.read_next().unwrap(), Some("chunk2"));
        let error = iterator.throw("Test error").expect_err("throw rethrows");

        assert_eq!(error.message(), "Test error");
        assert!(*cancelled.borrow());
    }

    #[test]
    fn create_async_iterable_stream_should_not_cancel_stream_when_exception_thrown_inside_for_await_loop()
     {
        let source = MockAsyncIterableStreamSource::with_chunks(["chunk1", "chunk2", "chunk3"]);
        let cancelled = source.cancelled_handle();
        let mut stream = create_async_iterable_stream_from_source(source);

        assert_eq!(
            collect_async_iterable_stream(&mut stream).unwrap(),
            vec!["chunk1", "chunk2", "chunk3"]
        );

        assert!(!*cancelled.borrow());
    }

    #[test]
    fn create_async_iterable_stream_should_not_allow_iterating_twice_after_breaking() {
        let mut stream = create_async_iterable_stream(vec!["chunk1", "chunk2", "chunk3"]);
        let mut collected = Vec::new();

        {
            let mut iterator = stream.iter();
            let chunk = iterator
                .read_next()
                .unwrap()
                .expect("first iteration yields one chunk");
            collected.push(chunk);
            iterator.return_stream().unwrap();
        }

        let mut iterator = stream.iter();
        while let Some(chunk) = iterator.read_next().unwrap() {
            collected.push(chunk);
        }

        assert_eq!(collected, vec!["chunk1"]);
    }

    #[test]
    fn create_async_iterable_stream_should_propagate_errors_from_source_stream_to_async_iterable() {
        let source = MockAsyncIterableStreamSource::with_entries([
            Ok("chunk1"),
            Ok("chunk2"),
            Err("Stream error".to_string()),
        ]);
        let mut stream = create_async_iterable_stream_from_source(source);
        let mut iterator = stream.iter();

        let collected = vec![
            iterator.read_next().unwrap().expect("first chunk"),
            iterator.read_next().unwrap().expect("second chunk"),
        ];
        let error = iterator
            .read_next()
            .expect_err("source stream error propagates");

        assert_eq!(collected, vec!["chunk1", "chunk2"]);
        assert_eq!(error.message(), "Stream error");
    }

    #[test]
    fn create_async_iterable_stream_should_stop_async_iterable_when_stream_is_cancelled() {
        let mut stream = create_async_iterable_stream(vec!["chunk1", "chunk2", "chunk3"]);

        assert_eq!(stream.read().unwrap(), Some("chunk1"));
        stream.cancel_with_reason("Test cancellation").unwrap();
        let error = stream.read().expect_err("cancelled active stream errors");

        assert_eq!(error.message(), "Test cancellation");
    }

    #[test]
    fn create_async_iterable_stream_should_not_collect_any_chunks_when_iterating_on_already_cancelled_stream()
     {
        let mut stream = create_async_iterable_stream(vec!["chunk1", "chunk2", "chunk3"]);

        stream.cancel().unwrap();

        assert_eq!(
            collect_async_iterable_stream(&mut stream).unwrap(),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn create_async_iterable_stream_should_not_throw_when_return_is_called_after_the_stream_completed()
     {
        let input = vec![
            "chunk1".to_string(),
            "chunk2".to_string(),
            "chunk3".to_string(),
        ];
        let mut stream = create_async_iterable_stream(input.clone());
        let mut iterator = stream.iter();
        let mut output = Vec::new();

        while let Some(chunk) = iterator.read_next().unwrap() {
            output.push(chunk);
        }

        assert_eq!(output, input);
        iterator.return_stream().unwrap();
        assert_eq!(iterator.read_next().unwrap(), None);
    }

    #[test]
    fn create_stitchable_stream_should_return_no_stream_when_immediately_closed() {
        let mut stream = stitchable_stream::<i32>();

        stream.close();

        assert_eq!(stream.collect().unwrap(), Vec::<i32>::new());
    }

    #[test]
    fn create_stitchable_stream_should_return_all_values_from_a_single_inner_stream() {
        let mut stream = stitchable_stream();

        stream.add_chunks(vec![1, 2, 3]).unwrap();
        stream.close();

        assert_eq!(stream.collect().unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn create_stitchable_stream_should_return_all_values_from_2_inner_streams() {
        let mut stream = stitchable_stream();

        stream.add_chunks(vec![1, 2, 3]).unwrap();
        stream.add_chunks(vec![4, 5, 6]).unwrap();
        stream.close();

        assert_eq!(stream.collect().unwrap(), vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn create_stitchable_stream_should_return_all_values_from_3_inner_streams() {
        let mut stream = stitchable_stream();

        stream.add_chunks(vec![1, 2, 3]).unwrap();
        stream.add_chunks(vec![4, 5, 6]).unwrap();
        stream.add_chunks(vec![7, 8, 9]).unwrap();
        stream.close();

        assert_eq!(stream.collect().unwrap(), vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn create_stitchable_stream_should_handle_empty_inner_streams() {
        let mut stream = stitchable_stream();

        stream.add_chunks(Vec::<i32>::new()).unwrap();
        stream.add_chunks(vec![1, 2]).unwrap();
        stream.add_chunks(Vec::<i32>::new()).unwrap();
        stream.add_chunks(vec![3, 4]).unwrap();
        stream.close();

        assert_eq!(stream.collect().unwrap(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn create_stitchable_stream_should_handle_reading_a_single_value_before_it_is_added() {
        let mut stream = stitchable_stream();

        assert_eq!(stream.read().unwrap(), StitchableStreamRead::Pending);

        stream.add_chunks(vec![42]).unwrap();
        stream.close();

        assert_eq!(stream.read().unwrap(), StitchableStreamRead::Chunk(42));
        assert_eq!(stream.read().unwrap(), StitchableStreamRead::Done);
    }

    #[test]
    fn create_stitchable_stream_should_return_all_values_from_2_inner_streams_when_reads_start_before_they_are_added()
     {
        let mut stream = stitchable_stream();

        assert_eq!(stream.read().unwrap(), StitchableStreamRead::Pending);

        stream.add_chunks(vec![1, 2, 3]).unwrap();
        stream.add_chunks(vec![4, 5]).unwrap();
        stream.close();

        assert_eq!(
            [
                stream.read().unwrap(),
                stream.read().unwrap(),
                stream.read().unwrap(),
                stream.read().unwrap(),
                stream.read().unwrap(),
                stream.read().unwrap(),
            ],
            [
                StitchableStreamRead::Chunk(1),
                StitchableStreamRead::Chunk(2),
                StitchableStreamRead::Chunk(3),
                StitchableStreamRead::Chunk(4),
                StitchableStreamRead::Chunk(5),
                StitchableStreamRead::Done,
            ]
        );
    }

    #[test]
    fn create_stitchable_stream_should_handle_errors_from_inner_streams() {
        let mut stream: StitchableStream<&str, MockAsyncIterableStreamSource<&str>> =
            create_stitchable_stream();

        stream
            .add_stream(MockAsyncIterableStreamSource::with_chunks(["1", "2"]))
            .unwrap();
        stream
            .add_stream(MockAsyncIterableStreamSource::with_entries([Err(
                "Test error".to_string(),
            )]))
            .unwrap();
        stream
            .add_stream(MockAsyncIterableStreamSource::with_chunks(["3", "4"]))
            .unwrap();
        stream.close();

        let error = stream.collect().expect_err("inner stream error propagates");

        assert_eq!(error.message(), "Test error");
    }

    #[test]
    fn create_stitchable_stream_should_cancel_all_inner_streams_when_cancelled() {
        let first = MockAsyncIterableStreamSource::with_chunks([1, 2]);
        let first_cancelled = first.cancelled_handle();
        let second = MockAsyncIterableStreamSource::with_chunks([3, 4]);
        let second_cancelled = second.cancelled_handle();
        let mut stream: StitchableStream<i32, MockAsyncIterableStreamSource<i32>> =
            create_stitchable_stream();

        stream.add_stream(first).unwrap();
        stream.add_stream(second).unwrap();
        stream.cancel().unwrap();

        assert!(*first_cancelled.borrow());
        assert!(*second_cancelled.borrow());
    }

    #[test]
    fn create_stitchable_stream_should_throw_an_error_when_adding_a_stream_after_closing() {
        let mut stream = stitchable_stream();

        stream.close();
        let error = stream
            .add_stream(vec_inner_stream(vec![1, 2]))
            .expect_err("closed stream rejects new inner stream");

        assert_eq!(
            error.message(),
            "Cannot add inner stream: outer stream is closed"
        );
    }

    #[test]
    fn create_stitchable_stream_should_immediately_close_the_stream_and_cancel_all_inner_streams() {
        let first = MockAsyncIterableStreamSource::with_chunks([1, 2]);
        let first_cancelled = first.cancelled_handle();
        let second = MockAsyncIterableStreamSource::with_chunks([3, 4]);
        let second_cancelled = second.cancelled_handle();
        let mut stream: StitchableStream<i32, MockAsyncIterableStreamSource<i32>> =
            create_stitchable_stream();

        stream.add_stream(first).unwrap();
        stream.add_stream(second).unwrap();
        let first_read = stream.read().unwrap();

        stream.terminate().unwrap();

        assert_eq!(first_read, StitchableStreamRead::Chunk(1));
        assert_eq!(stream.read().unwrap(), StitchableStreamRead::Done);
        assert!(*first_cancelled.borrow());
        assert!(*second_cancelled.borrow());
    }

    #[test]
    fn create_stitchable_stream_should_throw_an_error_when_adding_a_stream_after_terminating() {
        let mut stream = stitchable_stream();

        stream.terminate().unwrap();
        let error = stream
            .add_stream(vec_inner_stream(vec![1, 2]))
            .expect_err("terminated stream rejects new inner stream");

        assert_eq!(
            error.message(),
            "Cannot add inner stream: outer stream is closed"
        );
    }

    #[test]
    fn simulate_readable_stream_should_create_a_readable_stream_with_provided_values() {
        let stream = simulate_readable_stream(SimulateReadableStreamOptions::new(vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
        ]));

        assert_eq!(
            stream.collect().unwrap(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn simulate_readable_stream_should_respect_the_chunk_delay_in_ms_setting() {
        let mut delay_values = Vec::new();
        let chunks = {
            let stream = simulate_readable_stream_with_delay(
                SimulateReadableStreamOptions::new(vec![1, 2, 3])
                    .with_initial_delay_in_ms(500)
                    .with_chunk_delay_in_ms(100),
                |delay_in_ms| {
                    delay_values.push(delay_in_ms);
                    Ok(())
                },
            );
            stream.collect().unwrap()
        };

        assert_eq!(chunks, vec![1, 2, 3]);
        assert_eq!(delay_values, vec![Some(500), Some(100), Some(100)]);
    }

    #[test]
    fn simulate_readable_stream_should_handle_empty_values_array() {
        let mut stream =
            simulate_readable_stream(SimulateReadableStreamOptions::<i32>::new(vec![]));

        assert_eq!(stream.read().unwrap(), None);
    }

    #[test]
    fn simulate_readable_stream_should_handle_different_types_of_values() {
        let chunks = vec![
            serde_json::json!({ "id": 1, "text": "hello" }),
            serde_json::json!({ "id": 2, "text": "world" }),
        ];
        let stream = simulate_readable_stream(SimulateReadableStreamOptions::new(chunks));

        assert_eq!(
            stream.collect().unwrap(),
            vec![
                serde_json::json!({ "id": 1, "text": "hello" }),
                serde_json::json!({ "id": 2, "text": "world" }),
            ]
        );
    }

    #[test]
    fn simulate_readable_stream_should_skip_all_delays_when_both_delay_settings_are_null() {
        let mut delay_values = Vec::new();
        let chunks = {
            let stream = simulate_readable_stream_with_delay(
                SimulateReadableStreamOptions::new(vec![1, 2, 3])
                    .without_initial_delay()
                    .without_chunk_delay(),
                |delay_in_ms| {
                    delay_values.push(delay_in_ms);
                    Ok(())
                },
            );
            stream.collect().unwrap()
        };

        assert_eq!(chunks, vec![1, 2, 3]);
        assert_eq!(delay_values, vec![None, None, None]);
    }

    #[test]
    fn simulate_readable_stream_should_apply_chunk_delays_but_skip_initial_delay_when_initial_delay_in_ms_is_null()
     {
        let mut delay_values = Vec::new();
        let chunks = {
            let stream = simulate_readable_stream_with_delay(
                SimulateReadableStreamOptions::new(vec![1, 2, 3])
                    .without_initial_delay()
                    .with_chunk_delay_in_ms(100),
                |delay_in_ms| {
                    delay_values.push(delay_in_ms);
                    Ok(())
                },
            );
            stream.collect().unwrap()
        };

        assert_eq!(chunks, vec![1, 2, 3]);
        assert_eq!(delay_values, vec![None, Some(100), Some(100)]);
    }

    #[test]
    fn simulate_readable_stream_should_apply_initial_delay_but_skip_chunk_delays_when_chunk_delay_in_ms_is_null()
     {
        let mut delay_values = Vec::new();
        let chunks = {
            let stream = simulate_readable_stream_with_delay(
                SimulateReadableStreamOptions::new(vec![1, 2, 3])
                    .with_initial_delay_in_ms(500)
                    .without_chunk_delay(),
                |delay_in_ms| {
                    delay_values.push(delay_in_ms);
                    Ok(())
                },
            );
            stream.collect().unwrap()
        };

        assert_eq!(chunks, vec![1, 2, 3]);
        assert_eq!(delay_values, vec![Some(500), None, None]);
    }

    #[test]
    fn write_to_server_response_should_write_data_to_server_response() {
        let mut response = MockServerResponse::default();
        let headers = Headers::from([("Content-Type".to_string(), "text/plain".to_string())]);

        write_to_server_response(
            &mut response,
            WriteToServerResponseOptions::new(vec![b"chunk1".to_vec(), b"chunk2".to_vec()])
                .with_status(200)
                .with_status_text("OK")
                .with_headers(headers.clone()),
        )
        .unwrap();

        assert_eq!(response.status_code, 200);
        assert_eq!(response.status_message.as_deref(), Some("OK"));
        assert_eq!(response.headers, headers);
        assert_eq!(response.written_chunks.len(), 2);
        assert!(response.ended);
    }

    #[test]
    fn write_to_server_response_should_respect_backpressure_and_wait_for_drain_event() {
        let mut response = MockServerResponse::with_write_responses([true, false, true]);

        write_to_server_response(
            &mut response,
            WriteToServerResponseOptions::new(vec![
                b"chunk1".to_vec(),
                b"chunk2".to_vec(),
                b"chunk3".to_vec(),
            ])
            .with_status(200),
        )
        .unwrap();

        assert_eq!(
            response.events,
            vec![
                "write:chunk1".to_string(),
                "write:chunk2".to_string(),
                "drain".to_string(),
                "write:chunk3".to_string(),
                "end".to_string(),
            ]
        );
        assert_eq!(response.drain_wait_count, 1);
        assert_eq!(response.written_chunks.len(), 3);
        assert!(response.ended);
    }

    #[test]
    fn write_to_server_response_should_set_headers_correctly_when_status_text_is_undefined() {
        let mut response = MockServerResponse::default();
        let expected_headers = Headers::from([
            ("X-Example-Header".to_string(), "example-value".to_string()),
            (
                "X-Example-Chat-Title".to_string(),
                "My Conversation".to_string(),
            ),
        ]);

        write_to_server_response(
            &mut response,
            WriteToServerResponseOptions::new(vec![b"test data".to_vec()])
                .with_status(200)
                .with_headers(expected_headers.clone()),
        )
        .unwrap();

        assert_eq!(response.status_code, 200);
        assert_eq!(response.status_message, None);
        assert_eq!(response.headers, expected_headers);
        assert!(response.ended);
        assert_eq!(response.written_chunks.len(), 1);
    }

    #[test]
    fn write_to_server_response_should_set_headers_correctly_when_status_text_is_provided() {
        let mut response = MockServerResponse::default();
        let expected_headers = Headers::from([
            ("X-Example-Header".to_string(), "example-value".to_string()),
            (
                "X-Example-Chat-Title".to_string(),
                "New Chat Session".to_string(),
            ),
        ]);

        write_to_server_response(
            &mut response,
            WriteToServerResponseOptions::new(vec![b"test data".to_vec()])
                .with_status(201)
                .with_status_text("Created")
                .with_headers(expected_headers.clone()),
        )
        .unwrap();

        assert_eq!(response.status_code, 201);
        assert_eq!(response.status_message.as_deref(), Some("Created"));
        assert_eq!(response.headers, expected_headers);
        assert!(response.ended);
        assert_eq!(response.written_chunks.len(), 1);
    }

    #[test]
    fn write_to_server_response_should_set_headers_correctly_when_status_text_is_not_set_and_status_is_not_set()
     {
        let mut response = MockServerResponse::default();
        let expected_headers = Headers::from([
            ("X-Example-Header".to_string(), "example-value".to_string()),
            ("X-Example-Message".to_string(), "Hello World".to_string()),
        ]);

        write_to_server_response(
            &mut response,
            WriteToServerResponseOptions::new(vec![b"test data".to_vec()])
                .with_headers(expected_headers.clone()),
        )
        .unwrap();

        assert_eq!(response.status_code, 200);
        assert_eq!(response.status_message, None);
        assert_eq!(response.headers, expected_headers);
        assert!(response.ended);
        assert_eq!(response.written_chunks.len(), 1);
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
