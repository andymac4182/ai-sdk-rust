//! Messenger Graph API error classification.
//!
//! 1:1 port of upstream
//! `MessengerAdapter#throwGraphApiError(endpoint, status, data)`
//! from `packages/adapter-messenger/src/index.ts`. The upstream
//! method takes a `Response.status` + `data.error.{message, code,
//! type}` body and `throw`s the matching subclass of
//! `@chat-adapter/shared` errors. The Rust port surfaces the same
//! dispatch as a pure [`classify_graph_api_error`] function returning
//! a typed [`GraphApiError`]; the async HTTP layer maps it to
//! [`chat_sdk_chat::types::AdapterError`].
//!
//! Dispatch rules (1:1 with upstream):
//! - HTTP 429 or error code `4` / `32` / `613` -> RateLimit
//! - HTTP 401 or error code `190` -> Authentication
//! - HTTP 403 or error code `10` / `200` -> Validation
//! - HTTP 404 -> ResourceNotFound
//! - otherwise -> Network (with `(status N, code M)` suffix on the
//!   message)
//!
//! Fallback message + code: `data.error.message ?? "Messenger API
//! <endpoint> failed"`; `data.error.code ?? status`. When `data` has
//! no `error` object at all (e.g. `{}`), the helper still constructs
//! a Network error using the fallback message + status as code.

/// Classified Messenger Graph API error. 1:1 with the throw shape
/// of upstream `throwGraphApiError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphApiError {
    /// 1:1 with `AdapterRateLimitError("messenger")`. Triggered by
    /// HTTP 429 or Meta error codes 4 / 32 / 613.
    RateLimit,
    /// 1:1 with `AuthenticationError("messenger", message)`. Triggered
    /// by HTTP 401 or Meta error code 190.
    Authentication { message: String },
    /// 1:1 with `ValidationError("messenger", message)`. Triggered by
    /// HTTP 403 or Meta error codes 10 / 200.
    Validation { message: String },
    /// 1:1 with `ResourceNotFoundError("messenger", endpoint)`.
    /// Triggered by HTTP 404.
    ResourceNotFound { endpoint: String },
    /// 1:1 with `NetworkError("messenger", "<message> (status N, code
    /// M)")`. Used for every other (status, code) combination.
    Network { message: String },
}

/// Parsed error sub-block from a Graph API response body. 1:1 with
/// upstream `data.error as { message?: string; code?: number; type?:
/// string }`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphApiErrorBody {
    /// `data.error.message`.
    pub message: Option<String>,
    /// `data.error.code`.
    pub code: Option<i64>,
}

/// Dispatch a (status, body) pair to the matching [`GraphApiError`].
/// 1:1 with upstream's `throwGraphApiError(endpoint, status, data)`.
pub fn classify_graph_api_error(
    endpoint: &str,
    status: u16,
    error: &GraphApiErrorBody,
) -> GraphApiError {
    let fallback_message = format!("Messenger API {endpoint} failed");
    let message = error
        .message
        .clone()
        .unwrap_or_else(|| fallback_message.clone());
    let code = error.code.unwrap_or(i64::from(status));

    if status == 429 || code == 4 || code == 32 || code == 613 {
        return GraphApiError::RateLimit;
    }
    if status == 401 || code == 190 {
        return GraphApiError::Authentication { message };
    }
    if status == 403 || code == 10 || code == 200 {
        return GraphApiError::Validation { message };
    }
    if status == 404 {
        return GraphApiError::ResourceNotFound {
            endpoint: endpoint.to_string(),
        };
    }
    GraphApiError::Network {
        message: format!("{message} (status {status}, code {code})"),
    }
}

/// Convenience wrapper that classifies "the JSON body could not be
/// parsed" as a Network error with the upstream-shaped message
/// "Failed to parse Messenger API response for <endpoint>". 1:1 with
/// the upstream `catch { throw new NetworkError("messenger", ...) }`
/// surrounding `await response.json()`.
pub fn graph_api_json_parse_error(endpoint: &str) -> GraphApiError {
    GraphApiError::Network {
        message: format!("Failed to parse Messenger API response for {endpoint}"),
    }
}

/// Convenience wrapper that classifies "the fetch itself threw"
/// (DNS / connection-refused / TLS) as a Network error with the
/// upstream-shaped message "Network error calling Messenger Graph
/// API <endpoint>". 1:1 with the upstream `catch { throw new
/// NetworkError("messenger", ..., error instanceof Error ? error :
/// undefined) }` surrounding `await fetch(...)`.
pub fn graph_api_fetch_error(endpoint: &str) -> GraphApiError {
    GraphApiError::Network {
        message: format!("Network error calling Messenger Graph API {endpoint}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err_body(message: Option<&str>, code: Option<i64>) -> GraphApiErrorBody {
        GraphApiErrorBody {
            message: message.map(str::to_string),
            code,
        }
    }

    // ---------- Graph API error handling (15 upstream cases) ----------

    #[test]
    fn throws_adapter_rate_limit_error_on_429() {
        // 1:1 with upstream index.test.ts:1970 > "throws AdapterRateLimitError on 429"
        let got =
            classify_graph_api_error("me/messages", 429, &err_body(Some("Rate limited"), None));
        assert_eq!(got, GraphApiError::RateLimit);
    }

    #[test]
    fn throws_adapter_rate_limit_error_on_error_code_4() {
        // 1:1 with upstream index.test.ts:1980 > "throws AdapterRateLimitError on error code 4"
        let got = classify_graph_api_error(
            "me/messages",
            400,
            &err_body(Some("Too many calls"), Some(4)),
        );
        assert_eq!(got, GraphApiError::RateLimit);
    }

    #[test]
    fn throws_adapter_rate_limit_error_on_error_code_32() {
        // 1:1 with upstream index.test.ts:1990 > "throws AdapterRateLimitError on error code 32"
        let got = classify_graph_api_error(
            "me/messages",
            400,
            &err_body(Some("Page rate limit"), Some(32)),
        );
        assert_eq!(got, GraphApiError::RateLimit);
    }

    #[test]
    fn throws_adapter_rate_limit_error_on_error_code_613() {
        // 1:1 with upstream index.test.ts:2000 > "throws AdapterRateLimitError on error code 613"
        let got = classify_graph_api_error(
            "me/messages",
            400,
            &err_body(Some("Custom rate limit"), Some(613)),
        );
        assert_eq!(got, GraphApiError::RateLimit);
    }

    #[test]
    fn throws_authentication_error_on_401() {
        // 1:1 with upstream index.test.ts:2010 > "throws AuthenticationError on 401"
        let got = classify_graph_api_error(
            "me/messages",
            401,
            &err_body(Some("Invalid token"), Some(190)),
        );
        assert_eq!(
            got,
            GraphApiError::Authentication {
                message: "Invalid token".to_string()
            }
        );
    }

    #[test]
    fn throws_authentication_error_on_error_code_190_regardless_of_status() {
        // 1:1 with upstream index.test.ts:2020 > "throws AuthenticationError on error code 190 regardless of status"
        let got = classify_graph_api_error(
            "me/messages",
            400,
            &err_body(Some("Token expired"), Some(190)),
        );
        assert!(matches!(got, GraphApiError::Authentication { .. }));
    }

    #[test]
    fn throws_validation_error_on_403_permission_error() {
        // 1:1 with upstream index.test.ts:2030 > "throws ValidationError on 403 (permission error)"
        let got = classify_graph_api_error(
            "me/messages",
            403,
            &err_body(Some("Permission denied"), Some(10)),
        );
        assert!(matches!(got, GraphApiError::Validation { .. }));
    }

    #[test]
    fn throws_validation_error_on_error_code_200_permission() {
        // 1:1 with upstream index.test.ts:2040 > "throws ValidationError on error code 200 (permission)"
        let got = classify_graph_api_error(
            "me/messages",
            400,
            &err_body(Some("Requires permission"), Some(200)),
        );
        assert!(matches!(got, GraphApiError::Validation { .. }));
    }

    #[test]
    fn throws_resource_not_found_error_on_404() {
        // 1:1 with upstream index.test.ts:2050 > "throws ResourceNotFoundError on 404"
        let got = classify_graph_api_error("me/messages", 404, &err_body(Some("Not found"), None));
        assert_eq!(
            got,
            GraphApiError::ResourceNotFound {
                endpoint: "me/messages".to_string()
            }
        );
    }

    #[test]
    fn throws_network_error_on_generic_api_error() {
        // 1:1 with upstream index.test.ts:2060 > "throws NetworkError on generic API error"
        let got = classify_graph_api_error(
            "me/messages",
            500,
            &err_body(Some("Internal error"), Some(2)),
        );
        match got {
            GraphApiError::Network { message } => {
                assert!(message.contains("Internal error"));
                assert!(message.contains("status 500"));
                assert!(message.contains("code 2"));
            }
            other => panic!("expected Network, got {other:?}"),
        }
    }

    #[test]
    fn throws_network_error_when_fetch_throws() {
        // 1:1 with upstream index.test.ts:2070 > "throws NetworkError when fetch throws"
        // The Rust dispatcher for this case is the `graph_api_fetch_error`
        // helper invoked by the async HTTP layer when `fetch(...)` itself
        // throws (DNS / connect / TLS failure).
        let got = graph_api_fetch_error("me/messages");
        match got {
            GraphApiError::Network { message } => {
                assert!(message.contains("Network error calling Messenger Graph API"));
                assert!(message.contains("me/messages"));
            }
            other => panic!("expected Network, got {other:?}"),
        }
    }

    #[test]
    fn throws_network_error_when_response_is_not_valid_json() {
        // 1:1 with upstream index.test.ts:2085 > "throws NetworkError when response is not valid JSON"
        let got = graph_api_json_parse_error("me/messages");
        match got {
            GraphApiError::Network { message } => {
                assert!(message.contains("Failed to parse Messenger API response"));
                assert!(message.contains("me/messages"));
            }
            other => panic!("expected Network, got {other:?}"),
        }
    }

    #[test]
    fn uses_fallback_message_when_error_object_has_no_message() {
        // 1:1 with upstream index.test.ts:2105 > "uses fallback message when error object has no message"
        let got = classify_graph_api_error("me/messages", 500, &err_body(None, Some(999)));
        match got {
            GraphApiError::Network { message } => {
                assert!(
                    message.contains("Messenger API"),
                    "fallback contains pattern; got {message}"
                );
                assert!(message.contains("me/messages"));
            }
            other => panic!("expected Network, got {other:?}"),
        }
    }

    #[test]
    fn uses_status_as_code_when_error_object_has_no_code() {
        // 1:1 with upstream index.test.ts:2112 > "uses status as code when error object has no code"
        let got = classify_graph_api_error(
            "me/messages",
            500,
            &err_body(Some("Something failed"), None),
        );
        match got {
            GraphApiError::Network { message } => {
                assert!(message.contains("Something failed"));
                assert!(message.contains("status 500"));
                assert!(message.contains("code 500"));
            }
            other => panic!("expected Network, got {other:?}"),
        }
    }

    #[test]
    fn handles_response_with_no_error_object_at_all() {
        // 1:1 with upstream index.test.ts:2122 > "handles response with no error object at all"
        let got = classify_graph_api_error("me/messages", 500, &GraphApiErrorBody::default());
        assert!(matches!(got, GraphApiError::Network { .. }));
    }
}
