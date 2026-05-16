use std::fmt;

use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::headers::Headers;
use crate::provider::ApiCallError;
use crate::provider::get_error_message;

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
        RetryError, RetryErrorReason, get_retry_delay_in_ms, retry_delay_from_response_headers,
    };
    use crate::headers::Headers;
    use crate::provider::ApiCallError;
    use serde_json::json;
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
