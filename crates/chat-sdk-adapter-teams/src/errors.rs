//! Teams-specific error classifier. 1:1 port of upstream
//! `packages/adapter-teams/src/errors.ts`.
//!
//! Upstream throws one of `AuthenticationError`, `PermissionError`,
//! `NetworkError`, or `AdapterRateLimitError` (all from
//! `@chat-adapter/shared`) based on the status code or message shape
//! of a Teams SDK error. The Rust port returns the equivalent
//! [`AdapterError`] variant from `chat_sdk_adapter_shared::errors`
//! (Rust idiom: ports of throwing functions return rather than
//! panic).

use chat_sdk_adapter_shared::errors::AdapterError;

/// Classify a Teams SDK error into a shared
/// [`chat_sdk_adapter_shared::errors::AdapterError`] variant. 1:1
/// port of upstream `handleTeamsError(error, operation)`:
///
/// - statusCode (or `status` or `code`) 401, or `innerHttpError.statusCode`
///   401 -> `AuthenticationError`.
/// - statusCode 403, OR `message` substring "permission" (case
///   insensitive) -> `PermissionError`.
/// - statusCode 404 -> `NetworkError("Resource not found...")`.
/// - statusCode 429 -> `AdapterRateLimitError` (with optional
///   `retryAfter`).
/// - `message` present (string) -> `NetworkError` wrapping it.
/// - otherwise -> `NetworkError` wrapping `String(error)`.
pub fn handle_teams_error(error: &serde_json::Value, operation: &str) -> AdapterError {
    if error.is_object() {
        let inner_status = error
            .get("innerHttpError")
            .and_then(|v| v.get("statusCode"))
            .and_then(|v| v.as_i64());
        let status = inner_status
            .or_else(|| error.get("statusCode").and_then(|v| v.as_i64()))
            .or_else(|| error.get("status").and_then(|v| v.as_i64()))
            .or_else(|| error.get("code").and_then(|v| v.as_i64()));

        let message = error.get("message").and_then(|v| v.as_str());

        if status == Some(401) {
            let suffix = message.unwrap_or("unauthorized");
            return AdapterError::authentication_with(
                "teams",
                format!("Authentication failed for {operation}: {suffix}"),
            );
        }

        let message_contains_permission = message
            .map(|s| s.to_lowercase().contains("permission"))
            .unwrap_or(false);
        if status == Some(403) || message_contains_permission {
            return AdapterError::permission("teams", operation);
        }

        if status == Some(404) {
            return AdapterError::network_with(
                "teams",
                format!(
                    "Resource not found during {operation}: conversation or message may no longer exist"
                ),
            );
        }

        if status == Some(429) {
            let retry_after = error.get("retryAfter").and_then(|v| v.as_u64());
            return match retry_after {
                Some(s) => AdapterError::rate_limit_after("teams", s),
                None => AdapterError::rate_limit("teams"),
            };
        }

        if let Some(msg) = message {
            return AdapterError::network_with(
                "teams",
                format!("Teams API error during {operation}: {msg}"),
            );
        }
    }

    let rendered = match error {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    AdapterError::network_with(
        "teams",
        format!("Teams API error during {operation}: {rendered}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_authentication(err: &AdapterError) -> bool {
        matches!(err, AdapterError::Authentication { .. })
    }
    fn is_permission(err: &AdapterError) -> bool {
        matches!(err, AdapterError::Permission { .. })
    }
    fn is_network(err: &AdapterError) -> bool {
        matches!(err, AdapterError::Network { .. })
    }
    fn is_rate_limit(err: &AdapterError) -> bool {
        matches!(err, AdapterError::RateLimit { .. })
    }

    // ---------- 12 cases ported from upstream errors.test.ts ----------

    #[test]
    fn should_throw_authentication_error_for_401_status() {
        let err = handle_teams_error(
            &serde_json::json!({ "statusCode": 401, "message": "Unauthorized" }),
            "postMessage",
        );
        assert!(is_authentication(&err), "got {err:?}");
    }

    #[test]
    fn should_throw_permission_error_for_403_status() {
        let err = handle_teams_error(
            &serde_json::json!({ "statusCode": 403, "message": "Forbidden" }),
            "postMessage",
        );
        assert!(is_permission(&err), "got {err:?}");
    }

    #[test]
    fn should_throw_network_error_for_404_status() {
        let err = handle_teams_error(
            &serde_json::json!({ "statusCode": 404, "message": "Not found" }),
            "editMessage",
        );
        assert!(is_network(&err), "got {err:?}");
    }

    #[test]
    fn should_throw_rate_limit_error_for_429_status() {
        let err = handle_teams_error(
            &serde_json::json!({ "statusCode": 429, "retryAfter": 30 }),
            "postMessage",
        );
        assert!(is_rate_limit(&err), "got {err:?}");
    }

    #[test]
    fn should_handle_inner_http_error_status_code() {
        let err = handle_teams_error(
            &serde_json::json!({
                "innerHttpError": { "statusCode": 401 },
                "message": "Auth failed",
            }),
            "postMessage",
        );
        assert!(is_authentication(&err), "got {err:?}");
    }

    #[test]
    fn should_throw_rate_limit_error_with_retry_after_for_429() {
        let err = handle_teams_error(
            &serde_json::json!({ "statusCode": 429, "retryAfter": 60 }),
            "postMessage",
        );
        match err {
            AdapterError::RateLimit { retry_after, .. } => {
                assert_eq!(retry_after, Some(60));
            }
            other => panic!("expected RateLimit, got {other:?}"),
        }
    }

    #[test]
    fn should_throw_permission_error_for_messages_containing_permission() {
        let err = handle_teams_error(
            &serde_json::json!({
                "message": "Insufficient Permission to complete the operation",
            }),
            "deleteMessage",
        );
        assert!(is_permission(&err), "got {err:?}");
    }

    #[test]
    fn should_throw_network_error_for_generic_errors_with_message() {
        let err = handle_teams_error(
            &serde_json::json!({ "message": "Connection reset" }),
            "startTyping",
        );
        assert!(is_network(&err), "got {err:?}");
    }

    #[test]
    fn should_throw_network_error_for_unknown_error_types() {
        let err = handle_teams_error(&serde_json::json!("some string error"), "postMessage");
        assert!(is_network(&err), "got {err:?}");
    }

    #[test]
    fn should_throw_network_error_for_null_errors() {
        let err = handle_teams_error(&serde_json::Value::Null, "postMessage");
        assert!(is_network(&err), "got {err:?}");
    }

    #[test]
    fn should_use_status_field_if_status_code_not_present() {
        let err = handle_teams_error(
            &serde_json::json!({ "status": 401, "message": "Unauthorized" }),
            "postMessage",
        );
        assert!(is_authentication(&err), "got {err:?}");
    }

    #[test]
    fn should_use_code_field_if_status_code_and_status_not_present() {
        let err = handle_teams_error(&serde_json::json!({ "code": 429 }), "postMessage");
        assert!(is_rate_limit(&err), "got {err:?}");
    }
}
