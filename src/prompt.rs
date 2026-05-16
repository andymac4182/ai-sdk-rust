use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::file_data::FileDataContent;
use crate::headers::Headers;
use crate::json::JsonValue;
use crate::provider_utils::convert_to_base64;

/// Timeout configuration for high-level model and tool requests.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TimeoutConfiguration {
    /// A single total request timeout in milliseconds.
    TotalMs(u64),

    /// Granular timeout settings for individual request phases.
    Detailed(TimeoutConfigurationOptions),
}

impl TimeoutConfiguration {
    /// Creates a total timeout configuration in milliseconds.
    pub const fn total_ms(total_ms: u64) -> Self {
        Self::TotalMs(total_ms)
    }

    /// Creates a detailed timeout configuration.
    pub const fn detailed(options: TimeoutConfigurationOptions) -> Self {
        Self::Detailed(options)
    }
}

impl From<u64> for TimeoutConfiguration {
    fn from(total_ms: u64) -> Self {
        Self::TotalMs(total_ms)
    }
}

impl From<TimeoutConfigurationOptions> for TimeoutConfiguration {
    fn from(options: TimeoutConfigurationOptions) -> Self {
        Self::Detailed(options)
    }
}

/// Granular request timeout settings.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeoutConfigurationOptions {
    /// Total request timeout in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_ms: Option<u64>,

    /// Timeout for each model step in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_ms: Option<u64>,

    /// Timeout between stream chunks in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunk_ms: Option<u64>,

    /// Default timeout for each tool execution in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_ms: Option<u64>,

    /// Per-tool timeout overrides keyed as `{toolName}Ms`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<String, u64>,
}

impl TimeoutConfigurationOptions {
    /// Creates an empty detailed timeout configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the total request timeout in milliseconds.
    pub const fn with_total_ms(mut self, total_ms: u64) -> Self {
        self.total_ms = Some(total_ms);
        self
    }

    /// Sets the per-step timeout in milliseconds.
    pub const fn with_step_ms(mut self, step_ms: u64) -> Self {
        self.step_ms = Some(step_ms);
        self
    }

    /// Sets the stream chunk timeout in milliseconds.
    pub const fn with_chunk_ms(mut self, chunk_ms: u64) -> Self {
        self.chunk_ms = Some(chunk_ms);
        self
    }

    /// Sets the default per-tool timeout in milliseconds.
    pub const fn with_tool_ms(mut self, tool_ms: u64) -> Self {
        self.tool_ms = Some(tool_ms);
        self
    }

    /// Sets a per-tool timeout override in milliseconds.
    pub fn with_tool_timeout(mut self, tool_name: impl Into<String>, timeout_ms: u64) -> Self {
        self.tools
            .insert(format!("{}Ms", tool_name.into()), timeout_ms);
        self
    }
}

/// Request-facing controls for high-level SDK calls.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestOptions {
    /// Maximum number of retries. Set to 0 to disable retries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<usize>,

    /// Additional HTTP headers sent by HTTP-based providers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Headers>,

    /// Timeout configuration for the request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<TimeoutConfiguration>,
}

impl RequestOptions {
    /// Creates empty request options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum number of retries.
    pub const fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = Some(max_retries);
        self
    }

    /// Adds an HTTP header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }

    /// Sets the timeout configuration.
    pub fn with_timeout(mut self, timeout: impl Into<TimeoutConfiguration>) -> Self {
        self.timeout = Some(timeout.into());
        self
    }
}

/// Extracts the total timeout in milliseconds from a timeout configuration.
pub const fn get_total_timeout_ms(timeout: Option<&TimeoutConfiguration>) -> Option<u64> {
    match timeout {
        None => None,
        Some(TimeoutConfiguration::TotalMs(total_ms)) => Some(*total_ms),
        Some(TimeoutConfiguration::Detailed(options)) => options.total_ms,
    }
}

/// Extracts the step timeout in milliseconds from a timeout configuration.
pub const fn get_step_timeout_ms(timeout: Option<&TimeoutConfiguration>) -> Option<u64> {
    match timeout {
        Some(TimeoutConfiguration::Detailed(options)) => options.step_ms,
        Some(TimeoutConfiguration::TotalMs(_)) | None => None,
    }
}

/// Extracts the chunk timeout in milliseconds from a timeout configuration.
pub const fn get_chunk_timeout_ms(timeout: Option<&TimeoutConfiguration>) -> Option<u64> {
    match timeout {
        Some(TimeoutConfiguration::Detailed(options)) => options.chunk_ms,
        Some(TimeoutConfiguration::TotalMs(_)) | None => None,
    }
}

/// Extracts a tool-specific timeout in milliseconds from a timeout configuration.
pub fn get_tool_timeout_ms(timeout: Option<&TimeoutConfiguration>, tool_name: &str) -> Option<u64> {
    let Some(TimeoutConfiguration::Detailed(options)) = timeout else {
        return None;
    };

    options
        .tools
        .get(&format!("{tool_name}Ms"))
        .copied()
        .or(options.tool_ms)
}

/// Converts prompt data content to a base64-encoded string.
///
/// This mirrors upstream `convertDataContentToBase64String`: string content is
/// already base64 and passes through unchanged, while byte content is encoded.
pub fn convert_data_content_to_base64_string(content: &FileDataContent) -> String {
    convert_to_base64(content)
}

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

/// Error returned when a UI message cannot be converted to a model message.
#[derive(Clone, Debug, PartialEq)]
pub struct MessageConversionError {
    original_message: JsonValue,
    message: String,
}

impl MessageConversionError {
    /// Creates a message-conversion error with the original UI message context.
    pub fn new(original_message: impl Into<JsonValue>, message: impl Into<String>) -> Self {
        Self {
            original_message: original_message.into(),
            message: message.into(),
        }
    }

    /// Returns the original UI message that failed conversion.
    pub fn original_message(&self) -> &JsonValue {
        &self.original_message
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into its retained original message and message text.
    pub fn into_parts(self) -> (JsonValue, String) {
        (self.original_message, self.message)
    }
}

impl fmt::Display for MessageConversionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for MessageConversionError {}

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
    use std::collections::BTreeMap;

    use serde_json::json;

    use crate::file_data::FileDataContent;
    use crate::json::JsonValue;

    use super::{
        InvalidDataContentError, InvalidMessageRoleError, MessageConversionError, RequestOptions,
        TimeoutConfiguration, TimeoutConfigurationOptions, convert_data_content_to_base64_string,
        get_chunk_timeout_ms, get_step_timeout_ms, get_tool_timeout_ms, get_total_timeout_ms,
    };

    #[test]
    fn timeout_configuration_serializes_number_form() {
        let timeout = TimeoutConfiguration::total_ms(5_000);

        assert_eq!(
            serde_json::to_value(timeout).expect("timeout serialize"),
            json!(5000)
        );
    }

    #[test]
    fn timeout_configuration_serializes_detailed_form() {
        let timeout = TimeoutConfiguration::detailed(
            TimeoutConfigurationOptions::new()
                .with_total_ms(30_000)
                .with_step_ms(10_000)
                .with_chunk_ms(2_000)
                .with_tool_ms(5_000)
                .with_tool_timeout("search", 1_000),
        );

        assert_eq!(
            serde_json::to_value(timeout).expect("timeout serialize"),
            json!({
                "totalMs": 30000,
                "stepMs": 10000,
                "chunkMs": 2000,
                "toolMs": 5000,
                "tools": {
                    "searchMs": 1000
                }
            })
        );
    }

    #[test]
    fn timeout_configuration_deserializes_detailed_form() {
        let timeout: TimeoutConfiguration = serde_json::from_value(json!({
            "totalMs": 10000,
            "tools": {
                "weatherMs": 2500
            }
        }))
        .expect("timeout deserialize");

        assert_eq!(
            timeout,
            TimeoutConfiguration::Detailed(TimeoutConfigurationOptions {
                total_ms: Some(10_000),
                step_ms: None,
                chunk_ms: None,
                tool_ms: None,
                tools: BTreeMap::from([("weatherMs".to_string(), 2_500)])
            })
        );
    }

    #[test]
    fn request_options_serializes_upstream_shape_without_abort_signal() {
        let options = RequestOptions::new()
            .with_max_retries(3)
            .with_header("x-api-key", "sk-test")
            .with_timeout(TimeoutConfigurationOptions::new().with_step_ms(4_000));

        assert_eq!(
            serde_json::to_value(options).expect("request options serialize"),
            json!({
                "maxRetries": 3,
                "headers": {
                    "x-api-key": "sk-test"
                },
                "timeout": {
                    "stepMs": 4000
                }
            })
        );
    }

    #[test]
    fn request_options_deserializes_minimal_shape() {
        let options: RequestOptions = serde_json::from_value(json!({})).expect("deserialize");

        assert_eq!(options, RequestOptions::new());
    }

    #[test]
    fn timeout_helpers_match_upstream_number_and_missing_behavior() {
        let total = TimeoutConfiguration::total_ms(5_000);

        assert_eq!(get_total_timeout_ms(None), None);
        assert_eq!(get_total_timeout_ms(Some(&total)), Some(5_000));
        assert_eq!(get_step_timeout_ms(Some(&total)), None);
        assert_eq!(get_chunk_timeout_ms(Some(&total)), None);
        assert_eq!(get_tool_timeout_ms(Some(&total), "search"), None);
    }

    #[test]
    fn timeout_helpers_read_detailed_timeouts() {
        let timeout = TimeoutConfiguration::detailed(
            TimeoutConfigurationOptions::new()
                .with_total_ms(30_000)
                .with_step_ms(10_000)
                .with_chunk_ms(2_000)
                .with_tool_ms(5_000)
                .with_tool_timeout("search", 1_000),
        );

        assert_eq!(get_total_timeout_ms(Some(&timeout)), Some(30_000));
        assert_eq!(get_step_timeout_ms(Some(&timeout)), Some(10_000));
        assert_eq!(get_chunk_timeout_ms(Some(&timeout)), Some(2_000));
        assert_eq!(get_tool_timeout_ms(Some(&timeout), "search"), Some(1_000));
        assert_eq!(get_tool_timeout_ms(Some(&timeout), "weather"), Some(5_000));
    }

    #[test]
    fn convert_data_content_to_base64_string_passes_base64_strings_through() {
        assert_eq!(
            convert_data_content_to_base64_string(&FileDataContent::Base64(
                "already-base64".to_string()
            )),
            "already-base64"
        );
    }

    #[test]
    fn convert_data_content_to_base64_string_encodes_bytes() {
        assert_eq!(
            convert_data_content_to_base64_string(&FileDataContent::Bytes(b"Hello".to_vec())),
            "SGVsbG8="
        );
    }

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
    fn message_conversion_error_retains_original_message_and_message_text() {
        let original_message = json!({
            "role": "unknown",
            "parts": [{ "type": "text", "text": "unknown role message" }]
        });
        let error =
            MessageConversionError::new(original_message.clone(), "Unsupported role: unknown");

        assert_eq!(error.original_message(), &original_message);
        assert_eq!(error.message(), "Unsupported role: unknown");
        assert_eq!(error.to_string(), error.message());
    }

    #[test]
    fn message_conversion_error_supports_parts_conversion() {
        let original_message = json!({
            "role": "assistant",
            "parts": [{ "type": "custom", "kind": "example.part" }]
        });
        let error = MessageConversionError::new(
            original_message.clone(),
            "Unsupported custom UI message part",
        );

        assert_eq!(
            error.into_parts(),
            (
                original_message,
                "Unsupported custom UI message part".to_string()
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
