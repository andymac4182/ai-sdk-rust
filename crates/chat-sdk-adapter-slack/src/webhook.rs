//! Slack webhook parsing helpers.
//!
//! 1:1 port (in progress) of the pure-function helpers in
//! `packages/adapter-slack/src/webhook/utils.ts`. These are used by
//! the larger webhook parser (`parse.ts`) and signature verifier
//! (`verify.ts`), both of which land in follow-up slices.
//!
//! The upstream module is deliberately portable to edge runtimes
//! (Cloudflare Workers, Vercel Edge) - it avoids `node:crypto`,
//! `@chat-adapter/shared`, and `chat` imports. The Rust port matches
//! that posture: this module depends only on `std` + `serde_json`.

/// Whether `body` looks like a form-encoded payload. 1:1 port of
/// upstream `isFormBody(body, contentType)`:
///
/// - If the content type explicitly says
///   `application/x-www-form-urlencoded`, treat it as form.
/// - If it says `application/json`, treat it as JSON.
/// - Otherwise, sniff the body: starts-with `{` -> JSON; else, if
///   it contains a `=`, treat as form.
pub fn is_form_body(body: &str, content_type: &str) -> bool {
    if content_type.contains("application/x-www-form-urlencoded") {
        return true;
    }
    if content_type.contains("application/json") {
        return false;
    }
    let trimmed = body.trim_start();
    !trimmed.starts_with('{') && body.contains('=')
}

/// Parse a JSON body or return an error. 1:1 port of upstream
/// `parseJsonBody(body)` which throws
/// `SlackWebhookParseError("Slack webhook body is invalid JSON")`.
pub fn parse_json_body(body: &str) -> Result<serde_json::Value, SlackWebhookParseError> {
    serde_json::from_str(body)
        .map_err(|_| SlackWebhookParseError::new("Slack webhook body is invalid JSON"))
}

/// Whether a JSON value is a plain object (record). 1:1 port of
/// upstream `isRecord(value)` which checks
/// `typeof === "object" && value !== null && !Array.isArray(value)`.
pub fn is_record(value: &serde_json::Value) -> bool {
    value.is_object()
}

/// Extract a string view of a JSON value, returning `""` for any
/// non-string. 1:1 port of upstream `stringValue(value)`.
pub fn string_value(value: &serde_json::Value) -> &str {
    value.as_str().unwrap_or("")
}

/// Extract a string view of a JSON value, returning `None` for
/// non-strings or empty strings. 1:1 port of upstream
/// `optionalString(value)` which returns `text || undefined`.
pub fn optional_string(value: &serde_json::Value) -> Option<&str> {
    match value.as_str() {
        Some(s) if !s.is_empty() => Some(s),
        _ => None,
    }
}

/// Error returned by [`parse_json_body`]. 1:1 with upstream
/// `class SlackWebhookParseError extends Error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackWebhookParseError {
    message: String,
}

impl SlackWebhookParseError {
    /// Construct with a caller-supplied message. Mirrors
    /// `new SlackWebhookParseError(message)`.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Message accessor.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for SlackWebhookParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SlackWebhookParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- is_form_body: behavior matches upstream isFormBody ----------

    #[test]
    fn is_form_body_returns_true_for_form_content_type() {
        assert!(is_form_body("foo=bar", "application/x-www-form-urlencoded"));
        // Charset suffix preserved.
        assert!(is_form_body(
            "foo=bar",
            "application/x-www-form-urlencoded; charset=utf-8"
        ));
    }

    #[test]
    fn is_form_body_returns_false_for_json_content_type() {
        assert!(!is_form_body(r#"{"a":1}"#, "application/json"));
        assert!(!is_form_body(
            r#"{"a":1}"#,
            "application/json; charset=utf-8"
        ));
    }

    #[test]
    fn is_form_body_sniffs_body_when_content_type_unknown() {
        // Starts with `{` -> JSON.
        assert!(!is_form_body(r#"{"a":1}"#, "text/plain"));
        // Leading whitespace before `{` is also JSON (upstream uses
        // `trimStart`).
        assert!(!is_form_body(r#"   {"a":1}"#, "text/plain"));
        // Has `=` and doesn't start with `{` -> form.
        assert!(is_form_body("foo=bar", "text/plain"));
        // Neither `{` nor `=` -> not form.
        assert!(!is_form_body("hello world", "text/plain"));
    }

    // ---------- parse_json_body ----------

    #[test]
    fn parse_json_body_parses_valid_json() {
        let json = parse_json_body(r#"{"foo": "bar"}"#).unwrap();
        assert_eq!(json["foo"], "bar");
    }

    #[test]
    fn parse_json_body_returns_error_for_invalid_json() {
        let err = parse_json_body("not json").unwrap_err();
        assert_eq!(err.message(), "Slack webhook body is invalid JSON");
    }

    // ---------- is_record ----------

    #[test]
    fn is_record_returns_true_for_objects() {
        assert!(is_record(&serde_json::json!({})));
        assert!(is_record(&serde_json::json!({"a": 1})));
    }

    #[test]
    fn is_record_returns_false_for_non_objects() {
        assert!(!is_record(&serde_json::json!(null)));
        assert!(!is_record(&serde_json::json!("string")));
        assert!(!is_record(&serde_json::json!(123)));
        assert!(!is_record(&serde_json::json!(true)));
        assert!(!is_record(&serde_json::json!([1, 2, 3])));
    }

    // ---------- string_value ----------

    #[test]
    fn string_value_returns_the_string_for_string_values() {
        assert_eq!(string_value(&serde_json::json!("hello")), "hello");
    }

    #[test]
    fn string_value_returns_empty_for_non_strings() {
        assert_eq!(string_value(&serde_json::json!(null)), "");
        assert_eq!(string_value(&serde_json::json!(123)), "");
        assert_eq!(string_value(&serde_json::json!({})), "");
        assert_eq!(string_value(&serde_json::json!([])), "");
    }

    // ---------- optional_string ----------

    #[test]
    fn optional_string_returns_some_for_non_empty_strings() {
        assert_eq!(
            optional_string(&serde_json::json!("hello")),
            Some("hello")
        );
    }

    #[test]
    fn optional_string_returns_none_for_empty_or_non_string() {
        assert!(optional_string(&serde_json::json!("")).is_none());
        assert!(optional_string(&serde_json::json!(null)).is_none());
        assert!(optional_string(&serde_json::json!(123)).is_none());
        assert!(optional_string(&serde_json::json!({})).is_none());
    }
}
