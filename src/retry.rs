use std::fmt;

use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::{RetryError, RetryErrorReason};
    use serde_json::json;

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
}
