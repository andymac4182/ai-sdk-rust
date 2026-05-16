use std::fmt;

use crate::json::JsonValue;

/// Error returned when prompt data content is not a supported media-data value.
#[derive(Clone, Debug, PartialEq)]
pub struct InvalidDataContentError {
    content: JsonValue,
    message: String,
}

impl InvalidDataContentError {
    /// Creates an invalid-data-content error with the upstream default message.
    pub fn new(content: impl Into<JsonValue>) -> Self {
        let content = content.into();
        let message = invalid_data_content_default_message(&content);

        Self { content, message }
    }

    /// Creates an invalid-data-content error with a caller-supplied message.
    pub fn with_message(content: impl Into<JsonValue>, message: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            message: message.into(),
        }
    }

    /// Returns the invalid content value.
    pub fn content(&self) -> &JsonValue {
        &self.content
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its retained content and message.
    pub fn into_parts(self) -> (JsonValue, String) {
        (self.content, self.message)
    }
}

impl fmt::Display for InvalidDataContentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InvalidDataContentError {}

/// Error returned when a prompt message role is not supported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidMessageRoleError {
    role: String,
    message: String,
}

impl InvalidMessageRoleError {
    /// Creates an invalid-message-role error with the upstream default message.
    pub fn new(role: impl Into<String>) -> Self {
        let role = role.into();
        let message = invalid_message_role_default_message(&role);

        Self { role, message }
    }

    /// Creates an invalid-message-role error with a caller-supplied message.
    pub fn with_message(role: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            message: message.into(),
        }
    }

    /// Returns the unsupported message role.
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its retained role and message.
    pub fn into_parts(self) -> (String, String) {
        (self.role, self.message)
    }
}

impl fmt::Display for InvalidMessageRoleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InvalidMessageRoleError {}

fn invalid_message_role_default_message(role: &str) -> String {
    format!(
        r#"Invalid message role: '{role}'. Must be one of: "system", "user", "assistant", "tool"."#
    )
}

fn invalid_data_content_default_message(content: &JsonValue) -> String {
    format!(
        "Invalid data content. Expected a base64 string, Uint8Array, ArrayBuffer, or Buffer, but got {}.",
        json_value_js_typeof(content)
    )
}

fn json_value_js_typeof(content: &JsonValue) -> &'static str {
    match content {
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Null | JsonValue::Array(_) | JsonValue::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::json::JsonValue;

    use super::{InvalidDataContentError, InvalidMessageRoleError};

    #[test]
    fn invalid_data_content_error_matches_upstream_default_message() {
        let content = json!({ "data": false });
        let error = InvalidDataContentError::new(content.clone());

        assert_eq!(error.content(), &content);
        assert_eq!(
            error.message(),
            "Invalid data content. Expected a base64 string, Uint8Array, ArrayBuffer, or Buffer, but got object."
        );
        assert_eq!(error.to_string(), error.message());
    }

    #[test]
    fn invalid_data_content_error_uses_json_typeof_for_default_message() {
        assert_eq!(
            InvalidDataContentError::new(true).message(),
            "Invalid data content. Expected a base64 string, Uint8Array, ArrayBuffer, or Buffer, but got boolean."
        );
        assert_eq!(
            InvalidDataContentError::new(42).message(),
            "Invalid data content. Expected a base64 string, Uint8Array, ArrayBuffer, or Buffer, but got number."
        );
        assert_eq!(
            InvalidDataContentError::new("not-base64").message(),
            "Invalid data content. Expected a base64 string, Uint8Array, ArrayBuffer, or Buffer, but got string."
        );
        assert_eq!(
            InvalidDataContentError::new(JsonValue::Null).message(),
            "Invalid data content. Expected a base64 string, Uint8Array, ArrayBuffer, or Buffer, but got object."
        );
    }

    #[test]
    fn invalid_data_content_error_supports_custom_message_and_parts() {
        let error = InvalidDataContentError::with_message(
            "data:text/plain,hello",
            "Invalid data URL format in content data:text/plain,hello",
        );

        assert_eq!(
            error.into_parts(),
            (
                JsonValue::String("data:text/plain,hello".to_string()),
                "Invalid data URL format in content data:text/plain,hello".to_string()
            )
        );
    }

    #[test]
    fn invalid_message_role_error_matches_upstream_default_message() {
        let error = InvalidMessageRoleError::new("developer");

        assert_eq!(error.role(), "developer");
        assert_eq!(
            error.message(),
            r#"Invalid message role: 'developer'. Must be one of: "system", "user", "assistant", "tool"."#
        );
        assert_eq!(error.to_string(), error.message());
    }

    #[test]
    fn invalid_message_role_error_supports_custom_message_and_parts() {
        let error = InvalidMessageRoleError::with_message("chat", "custom role failure");

        assert_eq!(error.role(), "chat");
        assert_eq!(error.message(), "custom role failure");
        assert_eq!(
            error.into_parts(),
            ("chat".to_string(), "custom role failure".to_string())
        );
    }
}
