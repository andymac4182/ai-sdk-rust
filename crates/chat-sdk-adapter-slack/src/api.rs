//! Slack Web API helpers - pure-function subset.
//!
//! 1:1 port (in progress) of `packages/adapter-slack/src/api/index.ts`.
//! The upstream module has both pure helpers (encodeSlackApiBody,
//! assertSlackOk, SlackApiError) and HTTP-call helpers
//! (callSlackApi, postSlackMessage, postSlackEphemeral,
//! updateSlackMessage, deleteSlackMessage, sendSlackResponseUrl,
//! uploadSlackFiles, fetchSlackFile). This slice ports the pure
//! helpers + the [`SlackApiError`] shape. The HTTP-call wrappers
//! follow in a future slice (the underlying `chat.postMessage` /
//! `chat.update` / `chat.delete` / etc. methods are already wired
//! on `SlackAdapter` - the wrappers here would expose them as
//! free-standing functions matching the upstream API surface).

use serde::{Deserialize, Serialize};

/// Slack Web API response envelope. 1:1 with upstream
/// `interface SlackApiResponse`. The `ok` field is required;
/// everything else is optional and forwarded as JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackApiResponse {
    /// Always present.
    pub ok: bool,
    /// Slack snake_case error code (e.g. `channel_not_found`)
    /// when `ok == false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Scope / capability that the call needed but lacked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needed: Option<String>,
    /// Scope / capability the token actually has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provided: Option<String>,
    /// Catch-all for the rest of the JSON payload.
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// Error returned by the Slack API helpers. 1:1 with upstream
/// `class SlackApiError extends Error`. Carries the calling
/// method name plus the original response envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackApiError {
    pub method: String,
    pub message: String,
    /// Optional HTTP status (set by `callSlackApi` when the
    /// response is non-2xx; absent when the failure was an
    /// application-level `{ok: false}`).
    pub status: Option<u16>,
    /// Slack error code from the response envelope, if any.
    pub error_code: Option<String>,
}

impl std::fmt::Display for SlackApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SlackApiError {}

/// Body-encoded Slack API request. 1:1 with upstream
/// `encodeSlackApiBody`'s return `{body, contentType}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedSlackApiBody {
    pub body: String,
    pub content_type: String,
}

/// Encoding format for [`encode_slack_api_body`]. 1:1 with
/// upstream's `"form" | "json"` literal union.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlackApiBodyEncoding {
    /// `application/x-www-form-urlencoded` (Slack's default).
    #[default]
    Form,
    /// `application/json`.
    Json,
}

/// Encode a Slack Web API body. 1:1 port of upstream
/// `encodeSlackApiBody(body, contentType="form")`:
///
/// - `Json`: `JSON.stringify(removeUndefined(body))` ->
///   `application/json`.
/// - `Form`: stringified `URLSearchParams`. Each value goes
///   through `encodeSlackApiValue`: strings/numbers/booleans
///   become their `String(...)` form; objects/arrays become
///   `JSON.stringify(value)`. `undefined`/`null` values are
///   dropped. Returns `application/x-www-form-urlencoded`.
pub fn encode_slack_api_body(
    body: &serde_json::Map<String, serde_json::Value>,
    encoding: SlackApiBodyEncoding,
) -> EncodedSlackApiBody {
    match encoding {
        SlackApiBodyEncoding::Json => {
            let filtered: serde_json::Map<String, serde_json::Value> = body
                .iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            EncodedSlackApiBody {
                body: serde_json::Value::Object(filtered).to_string(),
                content_type: "application/json".to_string(),
            }
        }
        SlackApiBodyEncoding::Form => {
            let mut pairs: Vec<(String, String)> = Vec::with_capacity(body.len());
            for (key, value) in body {
                if value.is_null() {
                    continue;
                }
                pairs.push((key.clone(), encode_slack_api_value(value)));
            }
            EncodedSlackApiBody {
                body: form_url_encode(&pairs),
                content_type: "application/x-www-form-urlencoded".to_string(),
            }
        }
    }
}

/// 1:1 port of upstream `encodeSlackApiValue(value)`. Strings/
/// numbers/booleans render via `String(value)`; everything else
/// goes through `JSON.stringify`.
fn encode_slack_api_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        // Object / Array / Null go through JSON.stringify. Null
        // is filtered upstream and shouldn't reach here.
        _ => value.to_string(),
    }
}

/// Application/x-www-form-urlencoded serializer matching the byte
/// shape of JavaScript's `URLSearchParams.toString()`:
/// percent-encode anything outside `A-Za-z0-9-._~*` (with `*` left
/// alone because URLSearchParams treats it as unreserved), and use
/// `+` for ASCII space.
fn form_url_encode(pairs: &[(String, String)]) -> String {
    let mut out = String::new();
    for (i, (key, value)) in pairs.iter().enumerate() {
        if i > 0 {
            out.push('&');
        }
        push_url_param(&mut out, key);
        out.push('=');
        push_url_param(&mut out, value);
    }
    out
}

fn push_url_param(out: &mut String, s: &str) {
    for &byte in s.as_bytes() {
        match byte {
            b' ' => out.push('+'),
            b if b.is_ascii_alphanumeric()
                || b == b'-'
                || b == b'.'
                || b == b'_'
                || b == b'~'
                || b == b'*' =>
            {
                out.push(b as char);
            }
            other => {
                out.push_str(&format!("%{other:02X}"));
            }
        }
    }
}

/// Assert that a Slack Web API response carries `ok == true`,
/// or return a [`SlackApiError`]. 1:1 with upstream
/// `assertSlackOk(method, response)`.
pub fn assert_slack_ok(method: &str, response: &SlackApiResponse) -> Result<(), SlackApiError> {
    if response.ok {
        return Ok(());
    }
    let error_code = response
        .error
        .clone()
        .unwrap_or_else(|| "unknown_error".to_string());
    Err(SlackApiError {
        method: method.to_string(),
        message: format!("Slack {method} failed: {error_code}"),
        status: None,
        error_code: response.error.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- encodeSlackApiBody (1 case ported from upstream) ----------

    #[test]
    fn form_encodes_slack_api_bodies_with_json_object_values() {
        // Upstream sends `blocks` (an array) which should JSON-stringify,
        // `channel` (string) verbatim, `reply_broadcast: false` -> "false",
        // and `thread_ts: undefined` -> omitted.
        let mut body = serde_json::Map::new();
        body.insert(
            "blocks".to_string(),
            serde_json::json!([{ "type": "section" }]),
        );
        body.insert("channel".to_string(), serde_json::json!("C123"));
        body.insert("reply_broadcast".to_string(), serde_json::json!(false));
        body.insert("text".to_string(), serde_json::json!("hello"));
        body.insert("thread_ts".to_string(), serde_json::Value::Null);

        let encoded = encode_slack_api_body(&body, SlackApiBodyEncoding::Form);
        assert_eq!(encoded.content_type, "application/x-www-form-urlencoded");

        // Parse the body back through URL-decoding to verify pairs.
        let pairs: Vec<(String, String)> = encoded
            .body
            .split('&')
            .map(|kv| {
                let mut it = kv.splitn(2, '=');
                let k = it.next().unwrap_or("");
                let v = it.next().unwrap_or("");
                (url_decode(k), url_decode(v))
            })
            .collect();
        let blocks = pairs.iter().find(|(k, _)| k == "blocks").unwrap();
        assert_eq!(blocks.1, r#"[{"type":"section"}]"#);
        let reply = pairs.iter().find(|(k, _)| k == "reply_broadcast").unwrap();
        assert_eq!(reply.1, "false");
        assert!(!pairs.iter().any(|(k, _)| k == "thread_ts"));
    }

    // ---------- assert_slack_ok ----------

    #[test]
    fn assert_slack_ok_passes_for_ok_response() {
        let resp = SlackApiResponse {
            ok: true,
            error: None,
            needed: None,
            provided: None,
            other: serde_json::Map::new(),
        };
        assert!(assert_slack_ok("chat.postMessage", &resp).is_ok());
    }

    #[test]
    fn assert_slack_ok_returns_error_for_not_ok_with_code() {
        let resp = SlackApiResponse {
            ok: false,
            error: Some("channel_not_found".to_string()),
            needed: None,
            provided: None,
            other: serde_json::Map::new(),
        };
        let err = assert_slack_ok("chat.postMessage", &resp).unwrap_err();
        assert_eq!(err.method, "chat.postMessage");
        assert!(err.message.contains("chat.postMessage"));
        assert!(err.message.contains("channel_not_found"));
        assert_eq!(err.error_code.as_deref(), Some("channel_not_found"));
    }

    #[test]
    fn assert_slack_ok_uses_unknown_error_when_no_code() {
        let resp = SlackApiResponse {
            ok: false,
            error: None,
            needed: None,
            provided: None,
            other: serde_json::Map::new(),
        };
        let err = assert_slack_ok("chat.update", &resp).unwrap_err();
        assert!(err.message.contains("unknown_error"));
    }

    // ---------- additive Rust-side ----------

    #[test]
    fn encode_slack_api_body_json_emits_application_json() {
        let mut body = serde_json::Map::new();
        body.insert("channel".to_string(), serde_json::json!("C123"));
        body.insert("text".to_string(), serde_json::json!("hi"));
        let encoded = encode_slack_api_body(&body, SlackApiBodyEncoding::Json);
        assert_eq!(encoded.content_type, "application/json");
        let parsed: serde_json::Value = serde_json::from_str(&encoded.body).unwrap();
        assert_eq!(parsed["channel"], "C123");
        assert_eq!(parsed["text"], "hi");
    }

    // ---------- import boundary (1 upstream case from api/boundary.test.ts) ----------

    #[test]
    fn api_import_boundary_does_not_pull_in_chat_or_adapter_shared() {
        // 1:1 with upstream `packages/adapter-slack/src/api/boundary.test.ts`.
        // Forbidden import strings are built with concat! so they don't
        // match the test body itself when scanning the source file.
        let source = include_str!("api.rs");
        let forbidden_chat = concat!("use ", "chat_sdk_chat::");
        let forbidden_shared = concat!("use ", "chat_sdk_adapter_shared::");
        let forbidden_super = concat!("use ", "super::lib");
        assert!(!source.contains(forbidden_chat), "api.rs imports chat_sdk_chat");
        assert!(
            !source.contains(forbidden_shared),
            "api.rs imports chat_sdk_adapter_shared"
        );
        assert!(
            !source.contains(forbidden_super),
            "api.rs reaches back into the adapter's main module"
        );
    }

    #[test]
    fn encode_slack_api_body_form_escapes_special_chars() {
        let mut body = serde_json::Map::new();
        body.insert("text".to_string(), serde_json::json!("hello world & more"));
        let encoded = encode_slack_api_body(&body, SlackApiBodyEncoding::Form);
        // Space -> '+', '&' -> %26.
        assert_eq!(encoded.body, "text=hello+world+%26+more");
    }

    // Tiny URL decoder for the test — supports the subset our encoder
    // produces (alphanumerics + '+' for space + %HH).
    fn url_decode(input: &str) -> String {
        let bytes = input.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'+' => {
                    out.push(b' ');
                    i += 1;
                }
                b'%' if i + 2 < bytes.len() => {
                    let hex = &input[i + 1..i + 3];
                    if let Ok(byte) = u8::from_str_radix(hex, 16) {
                        out.push(byte);
                    }
                    i += 3;
                }
                b => {
                    out.push(b);
                    i += 1;
                }
            }
        }
        String::from_utf8(out).unwrap_or_default()
    }
}
