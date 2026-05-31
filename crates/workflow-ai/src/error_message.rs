use std::error::Error;
use std::fmt;

use ai_sdk_provider::json::JsonValue;

/// Error-shaped input accepted by [`get_error_message`].
#[derive(Clone, Debug, PartialEq)]
pub enum WorkflowErrorLike {
    /// JavaScript/Rust error instance equivalent; return the message directly.
    ErrorMessage(String),

    /// A thrown string value.
    String(String),

    /// A thrown JSON value such as an object, array, boolean, or number.
    Json(JsonValue),

    /// Missing/undefined error value.
    Unknown,
}

impl WorkflowErrorLike {
    /// Creates an error-instance equivalent.
    pub fn error(message: impl Into<String>) -> Self {
        Self::ErrorMessage(message.into())
    }
}

impl From<&str> for WorkflowErrorLike {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<String> for WorkflowErrorLike {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<JsonValue> for WorkflowErrorLike {
    fn from(value: JsonValue) -> Self {
        Self::Json(value)
    }
}

/// Returns the portable upstream `getErrorMessage` formatting.
pub fn get_error_message(error: impl Into<WorkflowErrorLike>) -> String {
    match error.into() {
        WorkflowErrorLike::ErrorMessage(message) | WorkflowErrorLike::String(message) => message,
        WorkflowErrorLike::Json(JsonValue::Null) | WorkflowErrorLike::Unknown => {
            "unknown error".to_string()
        }
        WorkflowErrorLike::Json(value) => value.to_string(),
    }
}

/// Error wrapper useful for Rust callers that need an [`Error`] value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowMessageError {
    message: String,
}

impl WorkflowMessageError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for WorkflowMessageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for WorkflowMessageError {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn get_error_message_upstream_should_return_message_from_error_instance() {
        assert_eq!(
            get_error_message(WorkflowErrorLike::error("Something went wrong")),
            "Something went wrong"
        );
    }

    #[test]
    fn get_error_message_upstream_should_return_string_errors_as_is() {
        assert_eq!(
            get_error_message("plain string error"),
            "plain string error"
        );
    }

    #[test]
    fn get_error_message_upstream_should_json_serialize_plain_objects_instead_of_object_object() {
        assert_eq!(
            get_error_message(json!({ "code": "ERR", "message": "Failed" })),
            r#"{"code":"ERR","message":"Failed"}"#
        );
    }

    #[test]
    fn get_error_message_upstream_should_json_serialize_nested_objects() {
        assert_eq!(
            get_error_message(json!({ "outer": { "inner": true } })),
            r#"{"outer":{"inner":true}}"#
        );
    }

    #[test]
    fn get_error_message_upstream_should_return_unknown_error_for_null_and_undefined() {
        assert_eq!(get_error_message(json!(null)), "unknown error");
        assert_eq!(
            get_error_message(WorkflowErrorLike::Unknown),
            "unknown error"
        );
    }

    #[test]
    fn get_error_message_upstream_should_handle_scalar_and_array_errors() {
        assert_eq!(get_error_message(json!(42)), "42");
        assert_eq!(get_error_message(json!(false)), "false");
        assert_eq!(get_error_message(json!(["a", 1])), r#"["a",1]"#);
        assert_eq!(get_error_message(""), "");
    }

    #[test]
    fn get_error_message_upstream_should_handle_error_subclass() {
        assert_eq!(
            get_error_message(WorkflowErrorLike::error("subclass message")),
            "subclass message"
        );
    }
}
