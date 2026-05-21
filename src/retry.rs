use std::{fmt, future::Future};

use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::gateway_error::GatewayError;
use crate::headers::Headers;
use crate::provider::ApiCallError;
use crate::provider::get_error_message;
use crate::provider_utils::is_abort_error;

/// Default number of retries used by upstream high-level AI SDK calls.
pub const DEFAULT_MAX_RETRIES: usize = 2;

/// Default initial retry delay used before exponential backoff is applied.
pub const DEFAULT_INITIAL_RETRY_DELAY_MS: u64 = 2_000;

/// Default multiplier applied to the retry delay after each retryable failure.
pub const DEFAULT_RETRY_BACKOFF_FACTOR: u64 = 2;

/// Reason a high-level retry operation failed.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RetryErrorReason {
    /// The operation exhausted the configured retry count.
    MaxRetriesExceeded,

    /// A later failure was not retryable.
    ErrorNotRetryable,

    /// The operation was aborted.
    Abort,
}

impl RetryErrorReason {
    /// Returns the upstream retry-error reason string.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MaxRetriesExceeded => "maxRetriesExceeded",
            Self::ErrorNotRetryable => "errorNotRetryable",
            Self::Abort => "abort",
        }
    }
}

impl fmt::Display for RetryErrorReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Error returned when a high-level retry operation fails after one or more attempts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RetryError {
    message: String,
    reason: RetryErrorReason,
    errors: Vec<String>,
}

impl RetryError {
    /// Creates a retry error with the upstream message, reason, and retained error list.
    pub fn new(
        message: impl Into<String>,
        reason: RetryErrorReason,
        errors: impl IntoIterator<Item = impl fmt::Display>,
    ) -> Self {
        let errors = errors
            .into_iter()
            .map(|error| get_error_message(Some(&error)))
            .collect::<Vec<_>>();

        Self {
            message: message.into(),
            reason,
            errors,
        }
    }

    /// Returns the human-readable retry failure message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Returns why retrying failed.
    pub fn reason(&self) -> RetryErrorReason {
        self.reason
    }

    /// Returns the retained attempt error messages.
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Returns the final attempt error message, when at least one error was retained.
    pub fn last_error(&self) -> Option<&str> {
        self.errors.last().map(String::as_str)
    }

    /// Converts this error into its retained parts.
    pub fn into_parts(self) -> (String, RetryErrorReason, Vec<String>, Option<String>) {
        let last_error = self.errors.last().cloned();
        (self.message, self.reason, self.errors, last_error)
    }
}

impl Serialize for RetryError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut field_count = 3;
        field_count += usize::from(self.last_error().is_some());

        let mut state = serializer.serialize_struct("RetryError", field_count)?;
        state.serialize_field("message", &self.message)?;
        state.serialize_field("reason", &self.reason)?;
        state.serialize_field("errors", &self.errors)?;

        if let Some(last_error) = self.last_error() {
            state.serialize_field("lastError", last_error)?;
        }

        state.end()
    }
}

impl fmt::Display for RetryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RetryError {}

/// Options for retrying API operations with exponential backoff.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryWithExponentialBackoffOptions {
    /// Maximum number of retries after the initial attempt.
    pub max_retries: usize,

    /// Initial exponential-backoff delay in milliseconds.
    pub initial_delay_in_ms: u64,

    /// Multiplier applied to the exponential-backoff delay after each retry.
    pub backoff_factor: u64,
}

impl RetryWithExponentialBackoffOptions {
    /// Creates retry options with upstream defaults.
    pub const fn new() -> Self {
        Self {
            max_retries: DEFAULT_MAX_RETRIES,
            initial_delay_in_ms: DEFAULT_INITIAL_RETRY_DELAY_MS,
            backoff_factor: DEFAULT_RETRY_BACKOFF_FACTOR,
        }
    }

    /// Sets the maximum number of retries.
    pub const fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Sets the initial retry delay in milliseconds.
    pub const fn with_initial_delay_in_ms(mut self, initial_delay_in_ms: u64) -> Self {
        self.initial_delay_in_ms = initial_delay_in_ms;
        self
    }

    /// Sets the retry delay multiplier.
    pub const fn with_backoff_factor(mut self, backoff_factor: u64) -> Self {
        self.backoff_factor = backoff_factor;
        self
    }
}

impl Default for RetryWithExponentialBackoffOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// Error returned by one attempted operation inside the retry loop.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum RetryAttemptError {
    /// A provider API call failed.
    ApiCall {
        /// API-call failure details.
        error: Box<ApiCallError>,
    },

    /// A Vercel AI Gateway error failed.
    Gateway {
        /// Upstream Gateway error class name.
        name: String,

        /// Human-readable Gateway error message.
        message: String,

        /// Whether the Gateway error should be retried.
        is_retryable: bool,

        /// Retry headers from an underlying API-call cause, when available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cause_response_headers: Option<Headers>,
    },

    /// A runtime or caller-defined error occurred.
    Runtime {
        /// Runtime-specific error name, when available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,

        /// Human-readable error message.
        message: String,
    },
}

impl RetryAttemptError {
    /// Creates an API-call retry attempt error.
    pub fn api_call(error: ApiCallError) -> Self {
        Self::ApiCall {
            error: Box::new(error),
        }
    }

    /// Creates a Gateway retry attempt error.
    pub fn gateway(error: impl Into<GatewayError>) -> Self {
        Self::gateway_with_cause_response_headers_option(error.into(), None)
    }

    /// Creates a Gateway retry attempt error with response headers from an API-call cause.
    pub fn gateway_with_cause_response_headers(
        error: impl Into<GatewayError>,
        cause_response_headers: Headers,
    ) -> Self {
        Self::gateway_with_cause_response_headers_option(error.into(), Some(cause_response_headers))
    }

    fn gateway_with_cause_response_headers_option(
        error: GatewayError,
        cause_response_headers: Option<Headers>,
    ) -> Self {
        Self::Gateway {
            name: error.name().to_string(),
            message: error.message().to_string(),
            is_retryable: error.is_retryable(),
            cause_response_headers,
        }
    }

    /// Creates a runtime retry attempt error with only a message.
    pub fn runtime(message: impl Into<String>) -> Self {
        Self::Runtime {
            name: None,
            message: message.into(),
        }
    }

    /// Creates a runtime retry attempt error with a JavaScript-style name.
    pub fn named_runtime(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Runtime {
            name: Some(name.into()),
            message: message.into(),
        }
    }

    /// Creates an abort-style retry attempt error.
    pub fn abort(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::named_runtime(name, message)
    }

    /// Returns API-call context for this attempt, when available.
    pub fn api_call_error(&self) -> Option<&ApiCallError> {
        match self {
            Self::ApiCall { error } => Some(error),
            Self::Gateway { .. } | Self::Runtime { .. } => None,
        }
    }

    /// Returns Gateway error name for this attempt, when available.
    pub fn gateway_error_name(&self) -> Option<&str> {
        match self {
            Self::Gateway { name, .. } => Some(name),
            Self::Runtime { .. } => None,
            Self::ApiCall { .. } => None,
        }
    }

    /// Returns the runtime error name, when available.
    pub fn runtime_name(&self) -> Option<&str> {
        match self {
            Self::ApiCall { .. } | Self::Gateway { .. } => None,
            Self::Runtime { name, .. } => name.as_deref(),
        }
    }

    /// Returns whether this attempt represents an aborted request.
    pub fn is_abort(&self) -> bool {
        self.runtime_name().is_some_and(is_abort_error)
    }

    /// Returns whether this attempt should be retried.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::ApiCall { error } => error.is_retryable(),
            Self::Gateway { is_retryable, .. } => *is_retryable,
            Self::Runtime { .. } => false,
        }
    }

    fn retry_delay_in_ms(&self, exponential_backoff_delay_ms: u64, now: OffsetDateTime) -> u64 {
        match self {
            Self::ApiCall { error } => {
                get_retry_delay_in_ms(error, exponential_backoff_delay_ms, now)
            }
            Self::Gateway {
                cause_response_headers,
                ..
            } => retry_delay_from_response_headers(
                cause_response_headers.as_ref(),
                exponential_backoff_delay_ms,
                now,
            ),
            Self::Runtime { .. } => exponential_backoff_delay_ms,
        }
    }
}

impl From<ApiCallError> for RetryAttemptError {
    fn from(error: ApiCallError) -> Self {
        Self::api_call(error)
    }
}

impl From<GatewayError> for RetryAttemptError {
    fn from(error: GatewayError) -> Self {
        Self::gateway(error)
    }
}

impl fmt::Display for RetryAttemptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiCall { error } => error.fmt(formatter),
            Self::Gateway { message, .. } => formatter.write_str(message),
            Self::Runtime { message, .. } => formatter.write_str(message),
        }
    }
}

impl std::error::Error for RetryAttemptError {}

/// Error returned by the retry executor.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum RetryFailure {
    /// The original attempt error should be propagated unchanged.
    Attempt {
        /// Original attempt error.
        error: Box<RetryAttemptError>,
    },

    /// Retries failed and were wrapped in an upstream-style retry error.
    Retry {
        /// Wrapped retry failure.
        error: Box<RetryError>,
    },
}

impl RetryFailure {
    /// Creates a pass-through attempt failure.
    pub fn attempt(error: RetryAttemptError) -> Self {
        Self::Attempt {
            error: Box::new(error),
        }
    }

    /// Creates a wrapped retry failure.
    pub fn retry(error: RetryError) -> Self {
        Self::Retry {
            error: Box::new(error),
        }
    }

    /// Returns the original attempt error, when this failure was not wrapped.
    pub fn attempt_error(&self) -> Option<&RetryAttemptError> {
        match self {
            Self::Attempt { error } => Some(error),
            Self::Retry { .. } => None,
        }
    }

    /// Returns the wrapped retry error, when retrying failed after multiple attempts.
    pub fn retry_error(&self) -> Option<&RetryError> {
        match self {
            Self::Attempt { .. } => None,
            Self::Retry { error } => Some(error),
        }
    }
}

impl From<RetryAttemptError> for RetryFailure {
    fn from(error: RetryAttemptError) -> Self {
        Self::attempt(error)
    }
}

impl From<RetryError> for RetryFailure {
    fn from(error: RetryError) -> Self {
        Self::retry(error)
    }
}

impl fmt::Display for RetryFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Attempt { error } => error.fmt(formatter),
            Self::Retry { error } => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RetryFailure {}

/// Retries an operation using upstream exponential-backoff semantics.
///
/// The caller supplies a sleep function so this helper remains independent of a
/// concrete async runtime. JavaScript-only `AbortSignal` mechanics are omitted;
/// abort-style runtime errors can still be passed through by returning a
/// [`RetryAttemptError`] with one of the upstream abort error names.
pub async fn retry_with_exponential_backoff_respecting_retry_headers<
    T,
    Operation,
    OperationFuture,
    Sleep,
    SleepFuture,
    Now,
>(
    mut operation: Operation,
    options: RetryWithExponentialBackoffOptions,
    mut sleep: Sleep,
    mut now: Now,
) -> Result<T, RetryFailure>
where
    Operation: FnMut() -> OperationFuture,
    OperationFuture: Future<Output = Result<T, RetryAttemptError>>,
    Sleep: FnMut(u64) -> SleepFuture,
    SleepFuture: Future<Output = ()>,
    Now: FnMut() -> OffsetDateTime,
{
    let mut delay_in_ms = options.initial_delay_in_ms;
    let mut errors = Vec::new();

    loop {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) => {
                if error.is_abort() || options.max_retries == 0 {
                    return Err(RetryFailure::attempt(error));
                }

                let error_message = get_error_message(Some(&error));
                errors.push(error_message.clone());
                let try_number = errors.len();

                if try_number > options.max_retries {
                    return Err(RetryFailure::retry(RetryError::new(
                        format!("Failed after {try_number} attempts. Last error: {error_message}"),
                        RetryErrorReason::MaxRetriesExceeded,
                        errors,
                    )));
                }

                if error.is_retryable() {
                    let retry_delay_ms = error.retry_delay_in_ms(delay_in_ms, now());
                    sleep(retry_delay_ms).await;
                    delay_in_ms = delay_in_ms.saturating_mul(options.backoff_factor);
                    continue;
                }

                if try_number == 1 {
                    return Err(RetryFailure::attempt(error));
                }

                return Err(RetryFailure::retry(RetryError::new(
                    format!(
                        "Failed after {try_number} attempts with non-retryable error: '{error_message}'"
                    ),
                    RetryErrorReason::ErrorNotRetryable,
                    errors,
                )));
            }
        }
    }
}

/// Returns the retry delay for a retryable API error.
///
/// This mirrors upstream `getRetryDelayInMs`: `retry-after-ms` takes priority
/// over `retry-after`, parsed values are used only when they are reasonable
/// (non-negative and under 60 seconds, unless shorter than the exponential
/// backoff delay), and otherwise the exponential backoff delay is returned.
pub fn get_retry_delay_in_ms(
    error: &ApiCallError,
    exponential_backoff_delay_ms: u64,
    now: OffsetDateTime,
) -> u64 {
    retry_delay_from_response_headers(error.response_headers(), exponential_backoff_delay_ms, now)
}

/// Returns the retry delay for response headers and an exponential backoff delay.
pub fn retry_delay_from_response_headers(
    response_headers: Option<&Headers>,
    exponential_backoff_delay_ms: u64,
    now: OffsetDateTime,
) -> u64 {
    let Some(response_headers) = response_headers else {
        return exponential_backoff_delay_ms;
    };

    if let Some(ms) = header_value(response_headers, "retry-after-ms").and_then(parse_float_prefix)
    {
        return reasonable_retry_delay_ms(ms, exponential_backoff_delay_ms)
            .unwrap_or(exponential_backoff_delay_ms);
    }

    let retry_after = header_value(response_headers, "retry-after")
        .and_then(|value| retry_after_delay_ms(value, now))
        .and_then(|ms| reasonable_retry_delay_ms(ms, exponential_backoff_delay_ms));

    retry_after.unwrap_or(exponential_backoff_delay_ms)
}

fn header_value<'a>(headers: &'a Headers, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .or_else(|| {
            headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value)
        })
        .map(String::as_str)
}

fn retry_after_delay_ms(retry_after: &str, now: OffsetDateTime) -> Option<f64> {
    parse_float_prefix(retry_after)
        .map(|seconds| seconds * 1000.0)
        .or_else(|| {
            OffsetDateTime::parse(retry_after, &time::format_description::well_known::Rfc2822)
                .ok()
                .map(|date| (date - now).whole_milliseconds() as f64)
        })
}

fn reasonable_retry_delay_ms(ms: f64, exponential_backoff_delay_ms: u64) -> Option<u64> {
    (ms.is_finite() && ms >= 0.0 && (ms < 60_000.0 || ms < exponential_backoff_delay_ms as f64))
        .then_some(ms as u64)
}

fn parse_float_prefix(value: &str) -> Option<f64> {
    let value = value.trim_start();
    let mut position = 0;

    if matches!(value.as_bytes().first(), Some(b'+' | b'-')) {
        position = 1;
    }

    let integer_start = position;
    position = consume_ascii_digits(value, position);
    let mut has_digits = position > integer_start;

    if value.as_bytes().get(position) == Some(&b'.') {
        position += 1;
        let fraction_start = position;
        position = consume_ascii_digits(value, position);
        has_digits |= position > fraction_start;
    }

    if !has_digits {
        return None;
    }

    let mut end = position;

    if matches!(value.as_bytes().get(position), Some(b'e' | b'E')) {
        position += 1;

        if matches!(value.as_bytes().get(position), Some(b'+' | b'-')) {
            position += 1;
        }

        let exponent_start = position;
        position = consume_ascii_digits(value, position);

        if position > exponent_start {
            end = position;
        }
    }

    value[..end].parse().ok()
}

fn consume_ascii_digits(value: &str, start: usize) -> usize {
    start
        + value.as_bytes()[start..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count()
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_INITIAL_RETRY_DELAY_MS, DEFAULT_MAX_RETRIES, DEFAULT_RETRY_BACKOFF_FACTOR,
        RetryAttemptError, RetryError, RetryErrorReason, RetryFailure,
        RetryWithExponentialBackoffOptions, get_retry_delay_in_ms,
        retry_delay_from_response_headers, retry_with_exponential_backoff_respecting_retry_headers,
    };
    use crate::gateway_error::{
        GatewayAuthenticationError, GatewayInternalServerError, GatewayRateLimitError,
    };
    use crate::headers::Headers;
    use crate::provider::ApiCallError;
    use serde_json::json;
    use std::cell::{Cell, RefCell};
    use std::future::{Future, ready};
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};
    use time::OffsetDateTime;

    fn now() -> OffsetDateTime {
        OffsetDateTime::parse(
            "2024-01-02T03:04:05Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("timestamp parses")
    }

    fn api_error_with_headers(headers: Headers) -> ApiCallError {
        ApiCallError::new("rate limited", "https://api.example.com", json!({}))
            .with_status_code(429)
            .with_response_headers(headers)
    }

    fn retryable_api_error(message: impl Into<String>) -> ApiCallError {
        ApiCallError::new(message, "https://api.example.com", json!({})).with_status_code(429)
    }

    fn retryable_api_error_with_headers(
        message: impl Into<String>,
        headers: Headers,
    ) -> ApiCallError {
        retryable_api_error(message).with_response_headers(headers)
    }

    fn retry_once_then_success(
        error: RetryAttemptError,
        options: RetryWithExponentialBackoffOptions,
    ) -> (&'static str, usize, Vec<u64>) {
        let attempts = Cell::new(0);
        let sleeps = RefCell::new(Vec::new());

        let result = poll_ready(retry_with_exponential_backoff_respecting_retry_headers(
            || {
                let attempt = attempts.get() + 1;
                attempts.set(attempt);

                if attempt == 1 {
                    ready(Err(error.clone()))
                } else {
                    ready(Ok("success"))
                }
            },
            options,
            |delay| {
                sleeps.borrow_mut().push(delay);
                ready(())
            },
            now,
        ))
        .expect("retry eventually succeeds");

        (result, attempts.get(), sleeps.into_inner())
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
    fn retry_error_reason_matches_upstream_strings() {
        assert_eq!(
            serde_json::to_value(RetryErrorReason::MaxRetriesExceeded).expect("reason serializes"),
            json!("maxRetriesExceeded")
        );
        assert_eq!(
            serde_json::to_value(RetryErrorReason::ErrorNotRetryable).expect("reason serializes"),
            json!("errorNotRetryable")
        );
        assert_eq!(
            serde_json::to_value(RetryErrorReason::Abort).expect("reason serializes"),
            json!("abort")
        );

        let reason: RetryErrorReason =
            serde_json::from_value(json!("errorNotRetryable")).expect("reason deserializes");
        assert_eq!(reason, RetryErrorReason::ErrorNotRetryable);
        assert_eq!(reason.as_str(), "errorNotRetryable");
        assert_eq!(reason.to_string(), "errorNotRetryable");
    }

    #[test]
    fn retry_error_retains_upstream_context_and_serializes() {
        let error = RetryError::new(
            "Failed after 3 attempts. Last error: timeout",
            RetryErrorReason::MaxRetriesExceeded,
            ["429 rate limit", "socket hang up", "timeout"],
        );

        assert_eq!(
            error.message(),
            "Failed after 3 attempts. Last error: timeout"
        );
        assert_eq!(error.reason(), RetryErrorReason::MaxRetriesExceeded);
        assert_eq!(
            error.errors(),
            &[
                "429 rate limit".to_string(),
                "socket hang up".to_string(),
                "timeout".to_string()
            ]
        );
        assert_eq!(error.last_error(), Some("timeout"));
        assert_eq!(error.to_string(), error.message());
        assert_eq!(
            serde_json::to_value(&error).expect("retry error serializes"),
            json!({
                "message": "Failed after 3 attempts. Last error: timeout",
                "reason": "maxRetriesExceeded",
                "errors": ["429 rate limit", "socket hang up", "timeout"],
                "lastError": "timeout"
            })
        );
    }

    #[test]
    fn retry_error_deserializes_and_omits_absent_last_error() {
        let deserialized: RetryError = serde_json::from_value(json!({
            "message": "Request was aborted.",
            "reason": "abort",
            "errors": []
        }))
        .expect("retry error deserializes");

        assert_eq!(deserialized.reason(), RetryErrorReason::Abort);
        assert!(deserialized.errors().is_empty());
        assert_eq!(deserialized.last_error(), None);
        assert_eq!(
            serde_json::to_value(&deserialized).expect("retry error serializes"),
            json!({
                "message": "Request was aborted.",
                "reason": "abort",
                "errors": []
            })
        );

        assert_eq!(
            deserialized.into_parts(),
            (
                "Request was aborted.".to_string(),
                RetryErrorReason::Abort,
                Vec::new(),
                None
            )
        );
    }

    #[test]
    fn retry_options_use_upstream_defaults_and_serialize() {
        let options = RetryWithExponentialBackoffOptions::new()
            .with_max_retries(3)
            .with_initial_delay_in_ms(1_000)
            .with_backoff_factor(3);

        assert_eq!(
            RetryWithExponentialBackoffOptions::default(),
            RetryWithExponentialBackoffOptions {
                max_retries: DEFAULT_MAX_RETRIES,
                initial_delay_in_ms: DEFAULT_INITIAL_RETRY_DELAY_MS,
                backoff_factor: DEFAULT_RETRY_BACKOFF_FACTOR,
            }
        );
        assert_eq!(
            serde_json::to_value(options).expect("retry options serialize"),
            json!({
                "maxRetries": 3,
                "initialDelayInMs": 1000,
                "backoffFactor": 3
            })
        );

        let deserialized: RetryWithExponentialBackoffOptions = serde_json::from_value(json!({
            "maxRetries": 4,
            "initialDelayInMs": 500,
            "backoffFactor": 2
        }))
        .expect("retry options deserialize");
        assert_eq!(deserialized.max_retries, 4);
        assert_eq!(deserialized.initial_delay_in_ms, 500);
        assert_eq!(deserialized.backoff_factor, 2);
    }

    #[test]
    fn retry_attempt_and_failure_errors_serialize() {
        let api_error = RetryAttemptError::api_call(retryable_api_error("rate limited"));
        assert!(api_error.is_retryable());
        assert!(!api_error.is_abort());
        assert!(api_error.api_call_error().is_some());

        let gateway_error =
            RetryAttemptError::gateway(GatewayRateLimitError::with_message("gateway rate limit"));
        assert!(gateway_error.is_retryable());
        assert_eq!(
            gateway_error.gateway_error_name(),
            Some("GatewayRateLimitError")
        );

        let abort_error = RetryAttemptError::named_runtime("AbortError", "request aborted");
        assert!(abort_error.is_abort());
        assert_eq!(abort_error.runtime_name(), Some("AbortError"));
        assert_eq!(abort_error.to_string(), "request aborted");

        let failure = RetryFailure::attempt(abort_error.clone());
        assert_eq!(failure.attempt_error(), Some(&abort_error));
        assert!(failure.retry_error().is_none());
        assert_eq!(
            serde_json::to_value(&failure).expect("retry failure serializes"),
            json!({
                "type": "attempt",
                "error": {
                    "type": "runtime",
                    "name": "AbortError",
                    "message": "request aborted"
                }
            })
        );

        let deserialized: RetryFailure = serde_json::from_value(json!({
            "type": "retry",
            "error": {
                "message": "Failed after 2 attempts. Last error: rate limited",
                "reason": "maxRetriesExceeded",
                "errors": ["first", "rate limited"],
                "lastError": "rate limited"
            }
        }))
        .expect("retry failure deserializes");
        assert_eq!(
            deserialized.retry_error().and_then(RetryError::last_error),
            Some("rate limited")
        );
    }

    #[test]
    fn retry_executor_returns_success_without_retrying() {
        let attempts = Cell::new(0);
        let sleeps = RefCell::new(Vec::new());

        let result = poll_ready(retry_with_exponential_backoff_respecting_retry_headers(
            || {
                attempts.set(attempts.get() + 1);
                ready(Ok::<_, RetryAttemptError>("done"))
            },
            RetryWithExponentialBackoffOptions::new(),
            |delay| {
                sleeps.borrow_mut().push(delay);
                ready(())
            },
            now,
        ))
        .expect("operation succeeds");

        assert_eq!(result, "done");
        assert_eq!(attempts.get(), 1);
        assert!(sleeps.borrow().is_empty());
    }

    #[test]
    fn retry_executor_retries_retryable_api_errors_with_headers_and_backoff() {
        let attempts = Cell::new(0);
        let sleeps = RefCell::new(Vec::new());

        let result = poll_ready(retry_with_exponential_backoff_respecting_retry_headers(
            || {
                let attempt = attempts.get() + 1;
                attempts.set(attempt);

                if attempt == 1 {
                    ready(Err(RetryAttemptError::api_call(
                        retryable_api_error("first").with_response_header("retry-after-ms", "3000"),
                    )))
                } else if attempt == 2 {
                    ready(Err(RetryAttemptError::api_call(retryable_api_error(
                        "second",
                    ))))
                } else {
                    ready(Ok("done"))
                }
            },
            RetryWithExponentialBackoffOptions::new()
                .with_max_retries(2)
                .with_initial_delay_in_ms(2_000)
                .with_backoff_factor(2),
            |delay| {
                sleeps.borrow_mut().push(delay);
                ready(())
            },
            now,
        ))
        .expect("retry eventually succeeds");

        assert_eq!(result, "done");
        assert_eq!(attempts.get(), 3);
        assert_eq!(&*sleeps.borrow(), &[3_000, 4_000]);
    }

    #[test]
    fn retry_with_exponential_backoff_uses_rate_limit_header_delay_when_present_and_reasonable() {
        let (result, attempts, sleeps) = retry_once_then_success(
            RetryAttemptError::api_call(retryable_api_error_with_headers(
                "Rate limited",
                Headers::from([("retry-after-ms".to_string(), "3000".to_string())]),
            )),
            RetryWithExponentialBackoffOptions::new(),
        );

        assert_eq!(result, "success");
        assert_eq!(attempts, 2);
        assert_eq!(sleeps, vec![3_000]);
    }

    #[test]
    fn retry_with_exponential_backoff_parses_retry_after_header_in_seconds() {
        let (result, attempts, sleeps) = retry_once_then_success(
            RetryAttemptError::api_call(retryable_api_error_with_headers(
                "Rate limited",
                Headers::from([("retry-after".to_string(), "5".to_string())]),
            )),
            RetryWithExponentialBackoffOptions::new(),
        );

        assert_eq!(result, "success");
        assert_eq!(attempts, 2);
        assert_eq!(sleeps, vec![5_000]);
    }

    #[test]
    fn retry_with_exponential_backoff_uses_exponential_backoff_when_rate_limit_delay_is_too_long() {
        let (result, attempts, sleeps) = retry_once_then_success(
            RetryAttemptError::api_call(retryable_api_error_with_headers(
                "Rate limited",
                Headers::from([("retry-after-ms".to_string(), "70000".to_string())]),
            )),
            RetryWithExponentialBackoffOptions::new().with_initial_delay_in_ms(2_000),
        );

        assert_eq!(result, "success");
        assert_eq!(attempts, 2);
        assert_eq!(sleeps, vec![2_000]);
    }

    #[test]
    fn retry_with_exponential_backoff_falls_back_to_exponential_backoff_when_no_rate_limit_headers()
    {
        let (result, attempts, sleeps) = retry_once_then_success(
            RetryAttemptError::api_call(retryable_api_error_with_headers(
                "Temporary error",
                Headers::new(),
            )),
            RetryWithExponentialBackoffOptions::new().with_initial_delay_in_ms(2_000),
        );

        assert_eq!(result, "success");
        assert_eq!(attempts, 2);
        assert_eq!(sleeps, vec![2_000]);
    }

    #[test]
    fn retry_with_exponential_backoff_handles_invalid_rate_limit_header_values() {
        let (result, attempts, sleeps) = retry_once_then_success(
            RetryAttemptError::api_call(retryable_api_error_with_headers(
                "Rate limited",
                Headers::from([
                    ("retry-after-ms".to_string(), "invalid".to_string()),
                    ("retry-after".to_string(), "not-a-number".to_string()),
                ]),
            )),
            RetryWithExponentialBackoffOptions::new().with_initial_delay_in_ms(2_000),
        );

        assert_eq!(result, "success");
        assert_eq!(attempts, 2);
        assert_eq!(sleeps, vec![2_000]);
    }

    #[test]
    fn retry_with_exponential_backoff_handles_anthropic_429_response_with_retry_after_ms_header() {
        let (result, attempts, sleeps) = retry_once_then_success(
            RetryAttemptError::api_call(
                ApiCallError::new(
                    "Rate limit exceeded",
                    "https://api.anthropic.com/v1/messages",
                    json!({}),
                )
                .with_status_code(429)
                .with_data(json!({
                    "error": {
                        "type": "rate_limit_error",
                        "message": "Rate limit exceeded"
                    }
                }))
                .with_response_headers(Headers::from([
                    ("retry-after-ms".to_string(), "5000".to_string()),
                    ("x-request-id".to_string(), "req_123456".to_string()),
                ])),
            ),
            RetryWithExponentialBackoffOptions::new(),
        );

        assert_eq!(result, "success");
        assert_eq!(attempts, 2);
        assert_eq!(sleeps, vec![5_000]);
    }

    #[test]
    fn retry_with_exponential_backoff_handles_openai_429_response_with_retry_after_header() {
        let (result, attempts, sleeps) = retry_once_then_success(
            RetryAttemptError::api_call(
                ApiCallError::new(
                    "Rate limit reached for requests",
                    "https://api.openai.com/v1/chat/completions",
                    json!({}),
                )
                .with_status_code(429)
                .with_data(json!({
                    "error": {
                        "message": "Rate limit reached for requests",
                        "type": "requests",
                        "param": null,
                        "code": "rate_limit_exceeded"
                    }
                }))
                .with_response_headers(Headers::from([
                    ("retry-after".to_string(), "30".to_string()),
                    ("x-request-id".to_string(), "req_abcdef123456".to_string()),
                ])),
            ),
            RetryWithExponentialBackoffOptions::new(),
        );

        assert_eq!(result, "success");
        assert_eq!(attempts, 2);
        assert_eq!(sleeps, vec![30_000]);
    }

    #[test]
    fn retry_with_exponential_backoff_handles_multiple_retries_with_exponential_backoff_progression()
     {
        let attempts = Cell::new(0);
        let sleeps = RefCell::new(Vec::new());

        let result = poll_ready(retry_with_exponential_backoff_respecting_retry_headers(
            || {
                let attempt = attempts.get() + 1;
                attempts.set(attempt);

                if attempt == 1 {
                    ready(Err(RetryAttemptError::api_call(
                        retryable_api_error_with_headers(
                            "Rate limited",
                            Headers::from([("retry-after-ms".to_string(), "5000".to_string())]),
                        ),
                    )))
                } else if attempt == 2 {
                    ready(Err(RetryAttemptError::api_call(
                        retryable_api_error_with_headers(
                            "Rate limited",
                            Headers::from([("retry-after-ms".to_string(), "2000".to_string())]),
                        ),
                    )))
                } else {
                    ready(Ok("Success after retries!"))
                }
            },
            RetryWithExponentialBackoffOptions::new().with_max_retries(3),
            |delay| {
                sleeps.borrow_mut().push(delay);
                ready(())
            },
            now,
        ))
        .expect("retry eventually succeeds");

        assert_eq!(result, "Success after retries!");
        assert_eq!(attempts.get(), 3);
        assert_eq!(&*sleeps.borrow(), &[5_000, 2_000]);
    }

    #[test]
    fn retry_with_exponential_backoff_prefers_retry_after_ms_over_retry_after_when_both_present() {
        let (result, attempts, sleeps) = retry_once_then_success(
            RetryAttemptError::api_call(retryable_api_error_with_headers(
                "Rate limited",
                Headers::from([
                    ("retry-after-ms".to_string(), "3000".to_string()),
                    ("retry-after".to_string(), "10".to_string()),
                ]),
            )),
            RetryWithExponentialBackoffOptions::new(),
        );

        assert_eq!(result, "success");
        assert_eq!(attempts, 2);
        assert_eq!(sleeps, vec![3_000]);
    }

    #[test]
    fn retry_with_exponential_backoff_handles_retry_after_header_with_http_date_format() {
        let (result, attempts, sleeps) = retry_once_then_success(
            RetryAttemptError::api_call(retryable_api_error_with_headers(
                "Rate limit exceeded",
                Headers::from([(
                    "retry-after".to_string(),
                    "Tue, 02 Jan 2024 03:04:10 GMT".to_string(),
                )]),
            )),
            RetryWithExponentialBackoffOptions::new(),
        );

        assert_eq!(result, "success");
        assert_eq!(attempts, 2);
        assert_eq!(sleeps, vec![5_000]);
    }

    #[test]
    fn retry_with_exponential_backoff_falls_back_to_exponential_backoff_when_rate_limit_delay_is_negative()
     {
        let (result, attempts, sleeps) = retry_once_then_success(
            RetryAttemptError::api_call(retryable_api_error_with_headers(
                "Rate limited",
                Headers::from([("retry-after-ms".to_string(), "-1000".to_string())]),
            )),
            RetryWithExponentialBackoffOptions::new().with_initial_delay_in_ms(2_000),
        );

        assert_eq!(result, "success");
        assert_eq!(attempts, 2);
        assert_eq!(sleeps, vec![2_000]);
    }

    #[test]
    fn retry_with_exponential_backoff_retries_on_gateway_internal_server_error() {
        let (result, attempts, sleeps) = retry_once_then_success(
            RetryAttemptError::gateway(
                GatewayInternalServerError::with_message("Internal server error")
                    .with_status_code(503),
            ),
            RetryWithExponentialBackoffOptions::new().with_initial_delay_in_ms(2_000),
        );

        assert_eq!(result, "success");
        assert_eq!(attempts, 2);
        assert_eq!(sleeps, vec![2_000]);
    }

    #[test]
    fn retry_with_exponential_backoff_retries_on_gateway_rate_limit_error() {
        let (result, attempts, sleeps) = retry_once_then_success(
            RetryAttemptError::gateway(GatewayRateLimitError::with_message("Rate limit exceeded")),
            RetryWithExponentialBackoffOptions::new().with_initial_delay_in_ms(2_000),
        );

        assert_eq!(result, "success");
        assert_eq!(attempts, 2);
        assert_eq!(sleeps, vec![2_000]);
    }

    #[test]
    fn retry_with_exponential_backoff_does_not_retry_on_non_retryable_gateway_authentication_error()
    {
        let attempts = Cell::new(0);
        let sleeps = RefCell::new(Vec::new());

        let failure = poll_ready(retry_with_exponential_backoff_respecting_retry_headers(
            || {
                attempts.set(attempts.get() + 1);
                ready(Err::<(), _>(RetryAttemptError::gateway(
                    GatewayAuthenticationError::with_message("Invalid API key"),
                )))
            },
            RetryWithExponentialBackoffOptions::new(),
            |delay| {
                sleeps.borrow_mut().push(delay);
                ready(())
            },
            now,
        ))
        .expect_err("non-retryable Gateway auth error fails");

        assert_eq!(attempts.get(), 1);
        assert!(sleeps.borrow().is_empty());
        assert_eq!(failure.to_string(), "Invalid API key");
        assert_eq!(
            failure
                .attempt_error()
                .and_then(RetryAttemptError::gateway_error_name),
            Some("GatewayAuthenticationError")
        );
    }

    #[test]
    fn retry_with_exponential_backoff_uses_retry_after_headers_from_api_call_error_cause() {
        let (result, attempts, sleeps) = retry_once_then_success(
            RetryAttemptError::gateway_with_cause_response_headers(
                GatewayInternalServerError::with_message("Internal server error")
                    .with_status_code(503),
                Headers::from([("retry-after-ms".to_string(), "3000".to_string())]),
            ),
            RetryWithExponentialBackoffOptions::new(),
        );

        assert_eq!(result, "success");
        assert_eq!(attempts, 2);
        assert_eq!(sleeps, vec![3_000]);
    }

    #[test]
    fn retry_executor_wraps_max_retries_exceeded() {
        let attempts = Cell::new(0);
        let sleeps = RefCell::new(Vec::new());

        let failure = poll_ready(retry_with_exponential_backoff_respecting_retry_headers(
            || {
                let attempt = attempts.get() + 1;
                attempts.set(attempt);
                ready(Err::<(), _>(RetryAttemptError::api_call(
                    retryable_api_error(format!("failure {attempt}")),
                )))
            },
            RetryWithExponentialBackoffOptions::new()
                .with_max_retries(2)
                .with_initial_delay_in_ms(1_000),
            |delay| {
                sleeps.borrow_mut().push(delay);
                ready(())
            },
            now,
        ))
        .expect_err("retry exhaustion fails");

        let retry_error = failure.retry_error().expect("failure is wrapped");
        assert_eq!(retry_error.reason(), RetryErrorReason::MaxRetriesExceeded);
        assert_eq!(
            retry_error.message(),
            "Failed after 3 attempts. Last error: failure 3"
        );
        assert_eq!(
            retry_error.errors(),
            &[
                "failure 1".to_string(),
                "failure 2".to_string(),
                "failure 3".to_string()
            ]
        );
        assert_eq!(attempts.get(), 3);
        assert_eq!(&*sleeps.borrow(), &[1_000, 2_000]);
    }

    #[test]
    fn retry_executor_passes_through_non_retryable_first_errors_and_disabled_retries() {
        let non_retryable = poll_ready(retry_with_exponential_backoff_respecting_retry_headers(
            || ready(Err::<(), _>(RetryAttemptError::runtime("bad request"))),
            RetryWithExponentialBackoffOptions::new(),
            |_| ready(()),
            now,
        ))
        .expect_err("non-retryable first failure passes through");
        assert_eq!(non_retryable.to_string(), "bad request");
        assert!(non_retryable.attempt_error().is_some());

        let disabled = poll_ready(retry_with_exponential_backoff_respecting_retry_headers(
            || {
                ready(Err::<(), _>(RetryAttemptError::api_call(
                    retryable_api_error("rate limited"),
                )))
            },
            RetryWithExponentialBackoffOptions::new().with_max_retries(0),
            |_| ready(()),
            now,
        ))
        .expect_err("disabled retries pass through");
        assert!(disabled.attempt_error().is_some());
        assert_eq!(disabled.to_string(), "rate limited");
    }

    #[test]
    fn retry_executor_wraps_non_retryable_error_after_prior_retry() {
        let attempts = Cell::new(0);
        let sleeps = RefCell::new(Vec::new());

        let failure = poll_ready(retry_with_exponential_backoff_respecting_retry_headers(
            || {
                let attempt = attempts.get() + 1;
                attempts.set(attempt);

                if attempt == 1 {
                    ready(Err::<(), _>(RetryAttemptError::api_call(
                        retryable_api_error("rate limited"),
                    )))
                } else {
                    ready(Err::<(), _>(RetryAttemptError::runtime("schema failed")))
                }
            },
            RetryWithExponentialBackoffOptions::new().with_initial_delay_in_ms(250),
            |delay| {
                sleeps.borrow_mut().push(delay);
                ready(())
            },
            now,
        ))
        .expect_err("non-retryable second failure is wrapped");

        let retry_error = failure.retry_error().expect("failure is wrapped");
        assert_eq!(retry_error.reason(), RetryErrorReason::ErrorNotRetryable);
        assert_eq!(
            retry_error.message(),
            "Failed after 2 attempts with non-retryable error: 'schema failed'"
        );
        assert_eq!(
            retry_error.errors(),
            &["rate limited".to_string(), "schema failed".to_string()]
        );
        assert_eq!(&*sleeps.borrow(), &[250]);
    }

    #[test]
    fn retry_executor_passes_through_abort_errors() {
        let sleeps = RefCell::new(Vec::new());
        let failure = poll_ready(retry_with_exponential_backoff_respecting_retry_headers(
            || {
                ready(Err::<(), _>(RetryAttemptError::abort(
                    "TimeoutError",
                    "operation timed out",
                )))
            },
            RetryWithExponentialBackoffOptions::new(),
            |delay| {
                sleeps.borrow_mut().push(delay);
                ready(())
            },
            now,
        ))
        .expect_err("abort failures pass through");

        assert_eq!(failure.to_string(), "operation timed out");
        assert!(
            failure
                .attempt_error()
                .is_some_and(RetryAttemptError::is_abort)
        );
        assert!(sleeps.borrow().is_empty());
    }

    #[test]
    fn retry_delay_uses_exponential_backoff_without_headers() {
        let error = ApiCallError::new("rate limited", "https://api.example.com", json!({}))
            .with_status_code(429);

        assert_eq!(get_retry_delay_in_ms(&error, 2_000, now()), 2_000);
        assert_eq!(retry_delay_from_response_headers(None, 2_000, now()), 2_000);
    }

    #[test]
    fn retry_delay_prefers_retry_after_ms_header() {
        let error = api_error_with_headers(Headers::from([
            ("retry-after-ms".to_string(), "3000".to_string()),
            ("retry-after".to_string(), "10".to_string()),
        ]));

        assert_eq!(get_retry_delay_in_ms(&error, 2_000, now()), 3_000);
    }

    #[test]
    fn retry_delay_parses_retry_after_seconds_and_float_prefixes() {
        let error = api_error_with_headers(Headers::from([(
            "retry-after".to_string(),
            "5 seconds".to_string(),
        )]));

        assert_eq!(get_retry_delay_in_ms(&error, 2_000, now()), 5_000);
    }

    #[test]
    fn retry_delay_falls_back_when_header_delay_is_unreasonable() {
        let error = api_error_with_headers(Headers::from([(
            "retry-after-ms".to_string(),
            "70000".to_string(),
        )]));

        assert_eq!(get_retry_delay_in_ms(&error, 2_000, now()), 2_000);

        assert_eq!(get_retry_delay_in_ms(&error, 120_000, now()), 70_000);
    }

    #[test]
    fn retry_delay_uses_retry_after_when_retry_after_ms_is_invalid() {
        let error = api_error_with_headers(Headers::from([
            ("retry-after-ms".to_string(), "not-a-number".to_string()),
            ("retry-after".to_string(), "2".to_string()),
        ]));

        assert_eq!(get_retry_delay_in_ms(&error, 5_000, now()), 2_000);
    }

    #[test]
    fn retry_delay_does_not_use_retry_after_when_retry_after_ms_is_unreasonable() {
        let error = api_error_with_headers(Headers::from([
            ("retry-after-ms".to_string(), "70000".to_string()),
            ("retry-after".to_string(), "2".to_string()),
        ]));

        assert_eq!(get_retry_delay_in_ms(&error, 5_000, now()), 5_000);
    }

    #[test]
    fn retry_delay_parses_retry_after_http_dates() {
        let error = api_error_with_headers(Headers::from([(
            "Retry-After".to_string(),
            "Tue, 02 Jan 2024 03:04:08 GMT".to_string(),
        )]));

        assert_eq!(get_retry_delay_in_ms(&error, 2_000, now()), 3_000);
    }

    #[test]
    fn retry_delay_rejects_negative_and_past_date_values() {
        let negative = api_error_with_headers(Headers::from([(
            "retry-after".to_string(),
            "-1".to_string(),
        )]));
        let past_date = api_error_with_headers(Headers::from([(
            "retry-after".to_string(),
            "Tue, 02 Jan 2024 03:04:00 GMT".to_string(),
        )]));

        assert_eq!(get_retry_delay_in_ms(&negative, 2_000, now()), 2_000);
        assert_eq!(get_retry_delay_in_ms(&past_date, 2_000, now()), 2_000);
    }
}
