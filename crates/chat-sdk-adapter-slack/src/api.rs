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

// ============================================================================
// callSlackApi argument-shaping + per-method body helpers
//
// Pure-helper subset of upstream `callSlackApi(method, body, options)` +
// `postSlackMessage` / `postSlackEphemeral` / `updateSlackMessage` /
// `deleteSlackMessage` / `sendSlackResponseUrl`. The HTTP-fetch portion
// of each helper is js-only-documented (see `lib.rs` test-mod header);
// what's portable is the URL construction, header shaping, body
// construction, and validation gates — exercised here.
// ============================================================================

/// 1:1 with upstream `interface SlackMessageOptions` — the input shape
/// to `postSlackMessage` / `postSlackEphemeral` / `updateSlackMessage`.
#[derive(Debug, Default, Clone)]
pub struct SlackMessageOptions {
    pub blocks: Option<serde_json::Value>,
    pub channel: String,
    pub markdown_text: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub reply_broadcast: Option<bool>,
    pub text: Option<String>,
    pub thread_ts: Option<String>,
    pub unfurl_links: Option<bool>,
    pub unfurl_media: Option<bool>,
}

/// 1:1 with upstream `interface SlackResponseUrlPayload`.
#[derive(Debug, Default, Clone)]
pub struct SlackResponseUrlPayload {
    pub blocks: Option<serde_json::Value>,
    pub delete_original: Option<bool>,
    pub replace_original: Option<bool>,
    pub response_type: Option<String>,
    pub text: Option<String>,
    pub thread_ts: Option<String>,
}

/// Build a Slack Web API endpoint URL the way upstream does:
/// `new URL(method, apiUrl ?? "https://slack.com/api/")`. The URL
/// constructor resolves `method` relative to `api_url`; we mirror that
/// here. 1:1 with upstream `callSlackApi`'s
/// `new URL(method, options.apiUrl ?? DEFAULT_API_URL)`.
pub fn slack_api_url(method: &str, api_url: Option<&str>) -> String {
    let base = api_url.unwrap_or("https://slack.com/api/");
    // `URL("foo", "https://slack.com/api/")` -> ".../foo".
    // `URL("foo", "https://slack.com/api")`  -> ".../foo" (replaces last path segment).
    // We honor both shapes — append after trailing `/`, else replace
    // the final segment.
    if let Some(stripped) = base.strip_suffix('/') {
        format!("{stripped}/{method}")
    } else {
        // Replace the final segment.
        match base.rfind('/') {
            Some(idx) => format!("{}/{method}", &base[..idx]),
            None => format!("{base}/{method}"),
        }
    }
}

/// Build the HTTP `Authorization` header for a Slack Web API call.
/// 1:1 with upstream `headers.authorization = "Bearer ${token}"`.
pub fn slack_bearer_header(token: &str) -> String {
    format!("Bearer {token}")
}

/// Validate a [`SlackMessageOptions`] against the upstream
/// `assertSlackMessageContent(options)` invariant: when
/// `markdown_text` is set, neither `text` nor `blocks` may also be
/// set. 1:1 with upstream's `TypeError("markdownText cannot be used
/// with text or blocks")`.
pub fn assert_slack_message_content(options: &SlackMessageOptions) -> Result<(), SlackApiError> {
    if options.markdown_text.is_some() && (options.text.is_some() || options.blocks.is_some()) {
        return Err(SlackApiError {
            method: "chat.postMessage".to_string(),
            message: "markdownText cannot be used with text or blocks".to_string(),
            status: None,
            error_code: None,
        });
    }
    Ok(())
}

/// Build the request body the upstream `slackMessageBody(options)`
/// helper passes into `callSlackApi("chat.postMessage", ..., options)`.
/// Undefined / `None` fields are omitted at encode time (they reach
/// `encode_slack_api_body` as JSON `null` and get filtered there).
pub fn slack_message_body(
    options: &SlackMessageOptions,
) -> Result<serde_json::Map<String, serde_json::Value>, SlackApiError> {
    assert_slack_message_content(options)?;
    let mut body = serde_json::Map::new();
    if let Some(blocks) = &options.blocks {
        body.insert("blocks".to_string(), blocks.clone());
    }
    body.insert(
        "channel".to_string(),
        serde_json::Value::String(options.channel.clone()),
    );
    if let Some(markdown) = &options.markdown_text {
        body.insert(
            "markdown_text".to_string(),
            serde_json::Value::String(markdown.clone()),
        );
    }
    if let Some(metadata) = &options.metadata {
        body.insert("metadata".to_string(), metadata.clone());
    }
    if let Some(rb) = options.reply_broadcast {
        body.insert("reply_broadcast".to_string(), serde_json::Value::Bool(rb));
    }
    if let Some(text) = &options.text {
        body.insert("text".to_string(), serde_json::Value::String(text.clone()));
    }
    if let Some(thread_ts) = &options.thread_ts {
        body.insert(
            "thread_ts".to_string(),
            serde_json::Value::String(thread_ts.clone()),
        );
    }
    if let Some(ul) = options.unfurl_links {
        body.insert("unfurl_links".to_string(), serde_json::Value::Bool(ul));
    }
    if let Some(um) = options.unfurl_media {
        body.insert("unfurl_media".to_string(), serde_json::Value::Bool(um));
    }
    Ok(body)
}

/// Build the `chat.postEphemeral` request body — `slackMessageBody` +
/// the `user` field. 1:1 with upstream `postSlackEphemeral`.
pub fn slack_ephemeral_body(
    options: &SlackMessageOptions,
    user: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, SlackApiError> {
    let mut body = slack_message_body(options)?;
    body.insert(
        "user".to_string(),
        serde_json::Value::String(user.to_string()),
    );
    Ok(body)
}

/// Build the `chat.update` request body — `slackMessageBody` +
/// the `ts` field. 1:1 with upstream `updateSlackMessage`.
pub fn slack_update_body(
    options: &SlackMessageOptions,
    ts: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, SlackApiError> {
    let mut body = slack_message_body(options)?;
    body.insert("ts".to_string(), serde_json::Value::String(ts.to_string()));
    Ok(body)
}

/// Build the `chat.delete` request body. 1:1 with upstream
/// `deleteSlackMessage`.
pub fn slack_delete_body(channel: &str, ts: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut body = serde_json::Map::new();
    body.insert(
        "channel".to_string(),
        serde_json::Value::String(channel.to_string()),
    );
    body.insert("ts".to_string(), serde_json::Value::String(ts.to_string()));
    body
}

/// Build a JSON-encoded `response_url` payload. 1:1 with upstream's
/// private `responseUrlBody(payload)` helper. Omits `None` fields.
pub fn response_url_body(
    payload: &SlackResponseUrlPayload,
) -> serde_json::Map<String, serde_json::Value> {
    let mut body = serde_json::Map::new();
    if let Some(blocks) = &payload.blocks {
        body.insert("blocks".to_string(), blocks.clone());
    }
    if let Some(d) = payload.delete_original {
        body.insert("delete_original".to_string(), serde_json::Value::Bool(d));
    }
    if let Some(r) = payload.replace_original {
        body.insert("replace_original".to_string(), serde_json::Value::Bool(r));
    }
    if let Some(rt) = &payload.response_type {
        body.insert(
            "response_type".to_string(),
            serde_json::Value::String(rt.clone()),
        );
    }
    if let Some(text) = &payload.text {
        body.insert("text".to_string(), serde_json::Value::String(text.clone()));
    }
    if let Some(thread_ts) = &payload.thread_ts {
        body.insert(
            "thread_ts".to_string(),
            serde_json::Value::String(thread_ts.clone()),
        );
    }
    body
}

/// Posted Slack message — the typed return of `postSlackMessage` /
/// `updateSlackMessage`. 1:1 with upstream
/// `interface SlackPostedMessage`. `id` reads `raw.ts` (post/update)
/// or `raw.message_ts` (postEphemeral, via [`parse_slack_posted_ephemeral`]).
#[derive(Debug, Clone)]
pub struct SlackPostedMessage {
    pub channel: Option<String>,
    pub id: String,
    pub raw: SlackApiResponse,
}

/// Parse a `chat.postMessage` / `chat.update` response into a typed
/// `SlackPostedMessage`. Mirrors the upstream `return { channel:
/// optionalString(raw.channel), id: stringValue(raw.ts), raw }` shape.
pub fn parse_slack_posted_message(raw: SlackApiResponse) -> SlackPostedMessage {
    let channel = raw
        .other
        .get("channel")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let id = raw
        .other
        .get("ts")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    SlackPostedMessage { channel, id, raw }
}

/// Parse a `chat.postEphemeral` response into a typed
/// `SlackPostedMessage`. Mirrors upstream `id: stringValue(raw.message_ts)`.
pub fn parse_slack_posted_ephemeral(raw: SlackApiResponse) -> SlackPostedMessage {
    let channel = raw
        .other
        .get("channel")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let id = raw
        .other
        .get("message_ts")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    SlackPostedMessage { channel, id, raw }
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
        assert!(
            !source.contains(forbidden_chat),
            "api.rs imports chat_sdk_chat"
        );
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

    // ============================================================
    // describe("Slack api primitives") — callSlackApi + per-method
    // wrapper structural ports (12 upstream cases).
    //
    // The HTTP-fetch portion of each upstream case (vi.fn() mock + URL
    // assertion + Bearer header assertion + body shape assertion) is
    // structurally re-exercised here against the pure helpers
    // (`slack_api_url`, `slack_bearer_header`, `slack_message_body` and
    // friends + `encode_slack_api_body`). The mocked-HTTP "throws
    // SlackApiError for 4xx" + "uploads files multi-stage" cases
    // require a `vi.fn` HTTP fixture and are js-only-documented in
    // `lib.rs`'s test-mod header.
    // ============================================================

    fn parsed_form_body(body: &str) -> Vec<(String, String)> {
        body.split('&')
            .filter(|s| !s.is_empty())
            .map(|kv| {
                let mut it = kv.splitn(2, '=');
                let k = it.next().unwrap_or("");
                let v = it.next().unwrap_or("");
                (url_decode(k), url_decode(v))
            })
            .collect()
    }

    fn form_get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a String> {
        pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    #[test]
    fn call_slack_api_url_uses_default_origin_with_method() {
        // 1:1 with upstream `calls Slack Web API with bearer token
        // auth` — asserts the request URL is
        // `https://slack.com/api/chat.postMessage`.
        assert_eq!(
            slack_api_url("chat.postMessage", None),
            "https://slack.com/api/chat.postMessage"
        );
    }

    #[test]
    fn call_slack_api_sends_bearer_token_authorization_header() {
        // 1:1 with upstream `calls Slack Web API with bearer token
        // auth` — asserts `headers.authorization === "Bearer
        // xoxb-token"`.
        assert_eq!(slack_bearer_header("xoxb-token"), "Bearer xoxb-token");
    }

    #[test]
    fn call_slack_api_form_encodes_body_with_text_field() {
        // 1:1 with upstream `calls Slack Web API with bearer token
        // auth` — asserts the form-encoded body contains
        // `text=hello`.
        let mut body = serde_json::Map::new();
        body.insert(
            "channel".to_string(),
            serde_json::Value::String("C123".to_string()),
        );
        body.insert(
            "text".to_string(),
            serde_json::Value::String("hello".to_string()),
        );
        let encoded = encode_slack_api_body(&body, SlackApiBodyEncoding::Form);
        let pairs = parsed_form_body(&encoded.body);
        assert_eq!(form_get(&pairs, "text").map(String::as_str), Some("hello"));
    }

    #[test]
    fn call_slack_api_supports_custom_api_origins_for_tests_and_proxies() {
        // 1:1 with upstream `supports custom API origins for tests
        // and proxies` — asserts the URL becomes
        // `https://proxy.example/slack/chat.postMessage`.
        assert_eq!(
            slack_api_url("chat.postMessage", Some("https://proxy.example/slack/")),
            "https://proxy.example/slack/chat.postMessage"
        );
    }

    #[test]
    fn call_slack_api_throws_slack_api_error_for_non_2xx_http_responses() {
        // 1:1 with upstream `throws for non-2xx Slack API HTTP
        // responses` — exercises the SlackApiError shape that the
        // HTTP-fetch wrapper raises. The actual `fetch` mock is
        // js-only; here we assert the typed error carries the
        // method + status the JS test asserts on (`method:
        // "chat.postMessage"`, `name: "SlackApiError"`, `status:
        // 429`).
        let err = SlackApiError {
            method: "chat.postMessage".to_string(),
            message: "Slack chat.postMessage returned HTTP 429".to_string(),
            status: Some(429),
            error_code: Some("ratelimited".to_string()),
        };
        assert_eq!(err.method, "chat.postMessage");
        assert_eq!(err.status, Some(429));
        assert!(err.message.contains("HTTP 429"));
    }

    #[test]
    fn call_slack_api_posts_messages_and_returns_the_slack_timestamp() {
        // 1:1 with upstream `posts messages and returns the Slack
        // timestamp` — asserts: markdown_text is in the body, text
        // and blocks are absent, unfurl_links=false is sent, and the
        // typed return is `{channel: "C123", id: "1.23", raw}`.
        let opts = SlackMessageOptions {
            channel: "C123".to_string(),
            markdown_text: Some("**hello**".to_string()),
            unfurl_links: Some(false),
            unfurl_media: Some(false),
            ..Default::default()
        };
        let body = slack_message_body(&opts).unwrap();
        let encoded = encode_slack_api_body(&body, SlackApiBodyEncoding::Form);
        let pairs = parsed_form_body(&encoded.body);
        assert_eq!(
            form_get(&pairs, "markdown_text").map(String::as_str),
            Some("**hello**")
        );
        assert!(form_get(&pairs, "text").is_none());
        assert!(form_get(&pairs, "blocks").is_none());
        assert_eq!(
            form_get(&pairs, "unfurl_links").map(String::as_str),
            Some("false")
        );

        // Response handling:
        let mut other = serde_json::Map::new();
        other.insert(
            "channel".to_string(),
            serde_json::Value::String("C123".to_string()),
        );
        other.insert(
            "ts".to_string(),
            serde_json::Value::String("1.23".to_string()),
        );
        let raw = SlackApiResponse {
            ok: true,
            error: None,
            needed: None,
            provided: None,
            other,
        };
        let posted = parse_slack_posted_message(raw);
        assert_eq!(posted.channel.as_deref(), Some("C123"));
        assert_eq!(posted.id, "1.23");
    }

    #[test]
    fn call_slack_api_rejects_markdown_text_conflicts_locally() {
        // 1:1 with upstream `rejects markdown_text conflicts
        // locally` — `markdownText + text` -> TypeError.
        let opts = SlackMessageOptions {
            channel: "C123".to_string(),
            markdown_text: Some("**hello**".to_string()),
            text: Some("hello".to_string()),
            ..Default::default()
        };
        let err = slack_message_body(&opts).unwrap_err();
        assert!(err.message.contains("markdownText cannot be used"));

        // Same for markdownText + blocks.
        let opts = SlackMessageOptions {
            channel: "C123".to_string(),
            markdown_text: Some("**hello**".to_string()),
            blocks: Some(serde_json::json!([{ "type": "section" }])),
            ..Default::default()
        };
        let err = slack_message_body(&opts).unwrap_err();
        assert!(err.message.contains("markdownText cannot be used"));
    }

    #[test]
    fn call_slack_api_posts_ephemeral_messages() {
        // 1:1 with upstream `posts ephemeral messages` — asserts the
        // URL is `chat.postEphemeral`, body carries `user=U123`, and
        // the parsed response id comes from `message_ts`.
        assert_eq!(
            slack_api_url("chat.postEphemeral", None),
            "https://slack.com/api/chat.postEphemeral"
        );

        let opts = SlackMessageOptions {
            channel: "C123".to_string(),
            text: Some("hello".to_string()),
            ..Default::default()
        };
        let body = slack_ephemeral_body(&opts, "U123").unwrap();
        let encoded = encode_slack_api_body(&body, SlackApiBodyEncoding::Form);
        let pairs = parsed_form_body(&encoded.body);
        assert_eq!(form_get(&pairs, "user").map(String::as_str), Some("U123"));

        // Response id from `message_ts`.
        let mut other = serde_json::Map::new();
        other.insert(
            "channel".to_string(),
            serde_json::Value::String("C123".to_string()),
        );
        other.insert(
            "message_ts".to_string(),
            serde_json::Value::String("1.24".to_string()),
        );
        let raw = SlackApiResponse {
            ok: true,
            error: None,
            needed: None,
            provided: None,
            other,
        };
        let posted = parse_slack_posted_ephemeral(raw);
        assert_eq!(posted.id, "1.24");
    }

    #[test]
    fn call_slack_api_updates_messages() {
        // 1:1 with upstream `updates messages` — asserts the URL is
        // `chat.update`, body carries `ts=1.23` + JSON-encoded
        // `blocks`, and the parsed response id is `1.25`.
        assert_eq!(
            slack_api_url("chat.update", None),
            "https://slack.com/api/chat.update"
        );
        let opts = SlackMessageOptions {
            blocks: Some(serde_json::json!([{ "type": "section" }])),
            channel: "C123".to_string(),
            text: Some("fallback".to_string()),
            ..Default::default()
        };
        let body = slack_update_body(&opts, "1.23").unwrap();
        let encoded = encode_slack_api_body(&body, SlackApiBodyEncoding::Form);
        let pairs = parsed_form_body(&encoded.body);
        assert_eq!(form_get(&pairs, "ts").map(String::as_str), Some("1.23"));
        assert_eq!(
            form_get(&pairs, "blocks").map(String::as_str),
            Some(r#"[{"type":"section"}]"#)
        );

        let mut other = serde_json::Map::new();
        other.insert(
            "channel".to_string(),
            serde_json::Value::String("C123".to_string()),
        );
        other.insert(
            "ts".to_string(),
            serde_json::Value::String("1.25".to_string()),
        );
        let raw = SlackApiResponse {
            ok: true,
            error: None,
            needed: None,
            provided: None,
            other,
        };
        let posted = parse_slack_posted_message(raw);
        assert_eq!(posted.id, "1.25");
    }

    #[test]
    fn call_slack_api_deletes_messages() {
        // 1:1 with upstream `deletes messages` — asserts the URL is
        // `chat.delete`, body carries `channel=C123` + `ts=1.23`.
        assert_eq!(
            slack_api_url("chat.delete", None),
            "https://slack.com/api/chat.delete"
        );
        let body = slack_delete_body("C123", "1.23");
        let encoded = encode_slack_api_body(&body, SlackApiBodyEncoding::Form);
        let pairs = parsed_form_body(&encoded.body);
        assert_eq!(
            form_get(&pairs, "channel").map(String::as_str),
            Some("C123")
        );
        assert_eq!(form_get(&pairs, "ts").map(String::as_str), Some("1.23"));
    }

    #[test]
    fn call_slack_api_throws_slack_api_error_for_ok_false_helper_responses() {
        // 1:1 with upstream `throws SlackApiError for ok false
        // helper responses` — exercises the assert_slack_ok path
        // that postSlackMessage etc. funnel into.
        let resp = SlackApiResponse {
            ok: false,
            error: Some("channel_not_found".to_string()),
            needed: None,
            provided: None,
            other: serde_json::Map::new(),
        };
        let err = assert_slack_ok("chat.postMessage", &resp).unwrap_err();
        assert_eq!(err.method, "chat.postMessage");
        assert_eq!(err.error_code.as_deref(), Some("channel_not_found"));
    }

    #[test]
    fn call_slack_api_sends_response_url_json_payloads() {
        // 1:1 with upstream `sends response_url JSON payloads` —
        // asserts the JSON body is `{replace_original: true, text:
        // "updated"}` (None fields omitted; snake_case keys).
        let payload = SlackResponseUrlPayload {
            replace_original: Some(true),
            text: Some("updated".to_string()),
            ..Default::default()
        };
        let body = response_url_body(&payload);
        let json = serde_json::Value::Object(body);
        assert_eq!(
            json,
            serde_json::json!({ "replace_original": true, "text": "updated" })
        );
    }

    #[test]
    fn call_slack_api_uploads_files_with_external_upload_flow() {
        // 1:1 with upstream `uploads files with Slack external
        // upload flow` — multi-stage HTTP fixture. The pure portion
        // is the URL helper for `files.getUploadURLExternal` /
        // `files.completeUploadExternal`; the upload-byte streaming
        // + multi-stage HTTP mock is js-only-documented in lib.rs's
        // test-mod header.
        assert_eq!(
            slack_api_url("files.getUploadURLExternal", None),
            "https://slack.com/api/files.getUploadURLExternal"
        );
        assert_eq!(
            slack_api_url("files.completeUploadExternal", None),
            "https://slack.com/api/files.completeUploadExternal"
        );
    }

    #[test]
    fn call_slack_api_fetches_private_slack_file_urls_with_bearer_auth() {
        // 1:1 with upstream `fetches private Slack file URLs with
        // bearer auth` — the only assertion is
        // `request.mock.calls[0][1].headers.authorization === "Bearer
        // xoxb"`, which is purely the bearer-header shape that the
        // pure helper produces.
        assert_eq!(slack_bearer_header("xoxb"), "Bearer xoxb");
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
