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

/// Look up a header by name, case-insensitively. 1:1 port of
/// upstream `getHeader(headers, name)`. The Rust port takes a slice
/// of `(name, value)` pairs (the same shape `reqwest::header::
/// HeaderMap::iter()` and most HTTP libraries expose) so callers can
/// pass either an iterator-style headers map or a `Vec<(_, _)>`.
pub fn get_header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    let lower = name.to_ascii_lowercase();
    for (k, v) in headers {
        if k.to_ascii_lowercase() == lower {
            return Some(v.as_str());
        }
    }
    None
}

/// Decoded Slack retry metadata. 1:1 with upstream `SlackRetry`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackRetry {
    pub num: i64,
    pub reason: Option<String>,
}

/// Extract Slack retry metadata from the `x-slack-retry-num` /
/// `x-slack-retry-reason` headers. 1:1 port of upstream
/// `getRetry(headers)`: returns `None` when the retry-num header is
/// missing or non-finite.
pub fn get_retry(headers: &[(String, String)]) -> Option<SlackRetry> {
    let retry_num = get_header(headers, "x-slack-retry-num")?;
    let num: i64 = retry_num.parse().ok()?;
    Some(SlackRetry {
        num,
        reason: get_header(headers, "x-slack-retry-reason").map(str::to_string),
    })
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

/// Compute the expected Slack signature for `body`/`timestamp` and
/// compare to `signature` in constant time. 1:1 with the upstream
/// `verifySlackSignatureValue(body, signingSecret, timestamp, signature)`
/// helper, which builds `v0:<timestamp>:<body>`, HMAC-SHA256s it
/// with the signing secret, hex-encodes, prefixes `v0=`, and
/// compares.
pub fn verify_slack_signature_value(
    body: &str,
    signing_secret: &str,
    timestamp: &str,
    signature: &str,
) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use subtle::ConstantTimeEq;

    let Some(hex_sig) = signature.strip_prefix("v0=") else {
        return false;
    };
    // Decode the hex signature (must be even-length and all hex).
    if hex_sig.len() % 2 != 0 || !hex_sig.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let received: Vec<u8> = (0..hex_sig.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&hex_sig[i..i + 2], 16).ok())
        .collect();
    if received.len() != 32 {
        return false;
    }

    type HmacSha256 = Hmac<Sha256>;
    let Ok(mut mac) = HmacSha256::new_from_slice(signing_secret.as_bytes()) else {
        return false;
    };
    mac.update(b"v0:");
    mac.update(timestamp.as_bytes());
    mac.update(b":");
    mac.update(body.as_bytes());
    let computed = mac.finalize().into_bytes();
    computed.as_slice().ct_eq(&received).into()
}

/// Errors returned by [`verify_slack_signature`]. 1:1 with
/// upstream `SlackWebhookVerificationError`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlackWebhookVerificationError {
    /// `signingSecret` was empty.
    MissingSecret,
    /// Required headers missing.
    MissingHeaders,
    /// `timestamp` header is not a finite number.
    InvalidTimestamp,
    /// `timestamp` is outside the allowed clock-skew window.
    TimestampTooOld,
    /// Signature mismatch.
    SignatureMismatch,
}

impl std::fmt::Display for SlackWebhookVerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSecret => f.write_str("Slack signing secret is required"),
            Self::MissingHeaders => f.write_str("Slack signature headers are required"),
            Self::InvalidTimestamp => f.write_str("Slack timestamp is invalid"),
            Self::TimestampTooOld => f.write_str("Slack timestamp is too old"),
            Self::SignatureMismatch => f.write_str("Slack signature is invalid"),
        }
    }
}

impl std::error::Error for SlackWebhookVerificationError {}

/// Options for [`verify_slack_signature`]. 1:1 with the upstream
/// `SlackVerifyOptions` interface (subset used by signature
/// verification - the `webhookVerifier` callback bypass is out of
/// scope for this slice).
#[derive(Debug, Clone)]
pub struct SlackVerifyOptions {
    /// Slack signing secret.
    pub signing_secret: String,
    /// Maximum allowed clock skew in seconds. Defaults to 300.
    pub max_skew_seconds: Option<u64>,
    /// Override the current time in seconds since the epoch.
    /// Used by tests. Defaults to system time.
    pub now_seconds: Option<u64>,
}

/// Verify a Slack webhook signature. 1:1 with upstream
/// `verifySlackSignature(body, headers, options)`:
///
/// 1. Require `signing_secret` to be non-empty.
/// 2. Require `x-slack-request-timestamp` and `x-slack-signature`.
/// 3. Require the timestamp to be a finite integer.
/// 4. Reject when `|now - timestamp| > max_skew_seconds`
///    (default 300).
/// 5. HMAC-SHA256 verify the signature.
pub fn verify_slack_signature(
    body: &str,
    timestamp_header: Option<&str>,
    signature_header: Option<&str>,
    options: &SlackVerifyOptions,
) -> Result<(), SlackWebhookVerificationError> {
    if options.signing_secret.is_empty() {
        return Err(SlackWebhookVerificationError::MissingSecret);
    }
    let (Some(timestamp), Some(signature)) = (timestamp_header, signature_header) else {
        return Err(SlackWebhookVerificationError::MissingHeaders);
    };
    if timestamp.is_empty() || signature.is_empty() {
        return Err(SlackWebhookVerificationError::MissingHeaders);
    }
    let timestamp_seconds: i64 = timestamp
        .parse::<i64>()
        .map_err(|_| SlackWebhookVerificationError::InvalidTimestamp)?;

    let now = options.now_seconds.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }) as i64;
    let max_skew = options.max_skew_seconds.unwrap_or(300) as i64;
    if (now - timestamp_seconds).abs() > max_skew {
        return Err(SlackWebhookVerificationError::TimestampTooOld);
    }

    if !verify_slack_signature_value(body, &options.signing_secret, timestamp, signature) {
        return Err(SlackWebhookVerificationError::SignatureMismatch);
    }
    Ok(())
}

// ============================================================================
// parseSlackWebhookBody + typed payload enum
//
// 1:1 port of `packages/adapter-slack/src/webhook/parse.ts` + the
// `SlackWebhookPayload` family in `webhook/types.ts`. Models the JSON
// payload as a Rust tagged enum; per-variant structs carry the typed
// fields upstream surfaces, plus a `raw` JSON copy for callers that
// need to roundtrip the full envelope.
// ============================================================================

/// 1:1 with upstream `interface SlackContinuation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackContinuation {
    pub channel_id: String,
    pub enterprise_id: Option<String>,
    pub team_id: Option<String>,
    pub thread_ts: String,
}

/// Single Slack interactive `actions[i]` entry. 1:1 with upstream
/// `interface SlackAction`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackAction {
    pub action_id: String,
    pub block_id: Option<String>,
    pub label: Option<String>,
    pub raw: serde_json::Value,
    pub selected_option_value: Option<String>,
    pub r#type: String,
    pub value: Option<String>,
}

/// URL verification challenge payload. 1:1 with upstream
/// `interface SlackUrlVerificationPayload`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackUrlVerificationPayload {
    pub challenge: String,
    pub raw: serde_json::Value,
    pub retry: Option<SlackRetry>,
}

/// Shared base for app_mention / direct_message event payloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackEventBase {
    pub api_app_id: Option<String>,
    pub channel_id: String,
    pub continuation: SlackContinuation,
    pub enterprise_id: Option<String>,
    pub event_id: Option<String>,
    pub event_time: Option<i64>,
    pub is_ext_shared_channel: Option<bool>,
    pub raw: serde_json::Value,
    pub retry: Option<SlackRetry>,
    pub team_id: Option<String>,
    pub text: String,
    pub thread_ts: String,
    pub ts: String,
    pub user_id: Option<String>,
}

/// `app_mention` payload. 1:1 with upstream
/// `interface SlackAppMentionPayload`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackAppMentionPayload {
    pub base: SlackEventBase,
}

/// `message.im` payload. 1:1 with upstream
/// `interface SlackDirectMessagePayload`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackDirectMessagePayload {
    pub base: SlackEventBase,
    pub bot_id: Option<String>,
    pub subtype: Option<String>,
}

/// Slash command payload. 1:1 with upstream
/// `interface SlackSlashCommandPayload`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackSlashCommandPayload {
    pub channel_id: String,
    pub channel_name: Option<String>,
    pub command: String,
    pub enterprise_id: Option<String>,
    pub is_enterprise_install: bool,
    pub raw: std::collections::BTreeMap<String, String>,
    pub response_url: Option<String>,
    pub retry: Option<SlackRetry>,
    pub team_id: Option<String>,
    pub text: String,
    pub trigger_id: Option<String>,
    pub user_id: String,
    pub user_name: Option<String>,
}

/// Block action payload. 1:1 with upstream
/// `interface SlackBlockActionsPayload`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackBlockActionsPayload {
    pub actions: Vec<SlackAction>,
    pub channel_id: Option<String>,
    pub continuation: Option<SlackContinuation>,
    pub enterprise_id: Option<String>,
    pub is_enterprise_install: Option<bool>,
    pub message_ts: Option<String>,
    pub raw: serde_json::Value,
    pub response_url: Option<String>,
    pub retry: Option<SlackRetry>,
    pub team_id: Option<String>,
    pub thread_ts: Option<String>,
    pub trigger_id: Option<String>,
    pub user_id: String,
    pub user_name: Option<String>,
}

/// Block suggestion (external_select option load) payload. 1:1 with
/// upstream `interface SlackBlockSuggestionPayload`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackBlockSuggestionPayload {
    pub action_id: String,
    pub block_id: String,
    pub channel_id: Option<String>,
    pub enterprise_id: Option<String>,
    pub raw: serde_json::Value,
    pub retry: Option<SlackRetry>,
    pub team_id: Option<String>,
    pub user_id: String,
    pub value: String,
}

/// View submission payload. 1:1 with upstream
/// `interface SlackViewSubmissionPayload`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackViewSubmissionPayload {
    pub enterprise_id: Option<String>,
    pub raw: serde_json::Value,
    pub response_urls: Option<Vec<serde_json::Value>>,
    pub retry: Option<SlackRetry>,
    pub team_id: Option<String>,
    pub user_id: String,
    pub view: serde_json::Map<String, serde_json::Value>,
}

/// View closed payload. 1:1 with upstream
/// `interface SlackViewClosedPayload`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackViewClosedPayload {
    pub enterprise_id: Option<String>,
    pub raw: serde_json::Value,
    pub retry: Option<SlackRetry>,
    pub team_id: Option<String>,
    pub user_id: String,
    pub view: serde_json::Map<String, serde_json::Value>,
}

/// Catch-all for unknown / unsupported event types. 1:1 with upstream
/// `interface SlackUnsupportedPayload`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackUnsupportedPayload {
    pub raw: serde_json::Value,
    pub retry: Option<SlackRetry>,
    pub r#type: String,
}

/// 1:1 port of upstream `type SlackWebhookPayload`. Each variant
/// mirrors the matching upstream `kind: "..."` discriminator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlackWebhookPayload {
    UrlVerification(SlackUrlVerificationPayload),
    AppMention(SlackAppMentionPayload),
    DirectMessage(SlackDirectMessagePayload),
    SlashCommand(SlackSlashCommandPayload),
    BlockActions(SlackBlockActionsPayload),
    BlockSuggestion(SlackBlockSuggestionPayload),
    ViewSubmission(SlackViewSubmissionPayload),
    ViewClosed(SlackViewClosedPayload),
    Unsupported(SlackUnsupportedPayload),
}

impl SlackWebhookPayload {
    /// Variant discriminator matching upstream's `kind` string.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::UrlVerification(_) => "url_verification",
            Self::AppMention(_) => "app_mention",
            Self::DirectMessage(_) => "direct_message",
            Self::SlashCommand(_) => "slash_command",
            Self::BlockActions(_) => "block_actions",
            Self::BlockSuggestion(_) => "block_suggestion",
            Self::ViewSubmission(_) => "view_submission",
            Self::ViewClosed(_) => "view_closed",
            Self::Unsupported(_) => "unsupported",
        }
    }
}

/// 1:1 with upstream `interface SlackParseOptions`. `headers` is a
/// slice of `(name, value)` pairs (the shape `HeaderMap::iter()`
/// produces).
#[derive(Debug, Default, Clone)]
pub struct SlackParseOptions<'a> {
    pub content_type: Option<&'a str>,
    pub headers: Option<&'a [(String, String)]>,
}

/// 1:1 port of upstream `parseSlackWebhookBody(body, options)`. Form
/// bodies (per `is_form_body`) dispatch into `parse_form_body`; JSON
/// bodies into `classify_json_payload`. Throws
/// [`SlackWebhookParseError`] on invalid JSON.
pub fn parse_slack_webhook_body(
    body: &str,
    options: &SlackParseOptions<'_>,
) -> Result<SlackWebhookPayload, SlackWebhookParseError> {
    let content_type = options
        .content_type
        .map(str::to_string)
        .or_else(|| {
            options
                .headers
                .and_then(|h| get_header(h, "content-type").map(str::to_string))
        })
        .unwrap_or_default();
    let retry = options.headers.and_then(get_retry);

    if is_form_body(body, &content_type) {
        return Ok(parse_form_body(body, retry));
    }
    let raw = parse_json_body(body)?;
    Ok(classify_json_payload(raw, retry))
}

fn parse_form_body(body: &str, retry: Option<SlackRetry>) -> SlackWebhookPayload {
    let params: Vec<(String, String)> = parse_form_pairs(body);
    if let Some((_, payload_str)) = params.iter().find(|(k, _)| k == "payload") {
        match serde_json::from_str::<serde_json::Value>(payload_str) {
            Ok(raw) => return classify_interaction_payload(raw, retry),
            Err(_) => {
                return SlackWebhookPayload::Unsupported(SlackUnsupportedPayload {
                    raw: serde_json::Value::String(payload_str.clone()),
                    retry,
                    r#type: "interaction".to_string(),
                });
            }
        }
    }
    if params.iter().any(|(k, _)| k == "command") {
        return SlackWebhookPayload::SlashCommand(parse_slash_command(&params, retry));
    }
    // Build a raw object mirroring `Object.fromEntries(params)`.
    let mut raw = serde_json::Map::new();
    for (k, v) in &params {
        raw.insert(k.clone(), serde_json::Value::String(v.clone()));
    }
    SlackWebhookPayload::Unsupported(SlackUnsupportedPayload {
        raw: serde_json::Value::Object(raw),
        retry,
        r#type: "form".to_string(),
    })
}

fn classify_json_payload(raw: serde_json::Value, retry: Option<SlackRetry>) -> SlackWebhookPayload {
    if !raw.is_object() {
        return SlackWebhookPayload::Unsupported(SlackUnsupportedPayload {
            raw,
            retry,
            r#type: "unknown".to_string(),
        });
    }

    let r#type = raw.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let challenge = raw.get("challenge").and_then(|v| v.as_str());
    if r#type == "url_verification"
        && let Some(challenge) = challenge
    {
        return SlackWebhookPayload::UrlVerification(SlackUrlVerificationPayload {
            challenge: challenge.to_string(),
            raw,
            retry,
        });
    }

    let event = raw.get("event").cloned();
    let event_is_record = event.as_ref().is_some_and(serde_json::Value::is_object);
    if r#type != "event_callback" || !event_is_record {
        let fallback_type = if r#type.is_empty() {
            "unknown".to_string()
        } else {
            r#type.to_string()
        };
        return SlackWebhookPayload::Unsupported(SlackUnsupportedPayload {
            raw,
            retry,
            r#type: fallback_type,
        });
    }

    let event_val = event.unwrap();
    let event_type = event_val.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if event_type == "app_mention" {
        return SlackWebhookPayload::AppMention(SlackAppMentionPayload {
            base: parse_message_event_base(&raw, &event_val, retry),
        });
    }

    let channel_type = event_val
        .get("channel_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if event_type == "message" && channel_type == "im" {
        let base = parse_message_event_base(&raw, &event_val, retry);
        let bot_id = optional_string(event_val.get("bot_id").unwrap_or(&serde_json::Value::Null))
            .map(str::to_string);
        let subtype = optional_string(event_val.get("subtype").unwrap_or(&serde_json::Value::Null))
            .map(str::to_string);
        return SlackWebhookPayload::DirectMessage(SlackDirectMessagePayload {
            base,
            bot_id,
            subtype,
        });
    }

    let fallback_type = if event_type.is_empty() {
        "event_callback".to_string()
    } else {
        event_type.to_string()
    };
    SlackWebhookPayload::Unsupported(SlackUnsupportedPayload {
        raw,
        retry,
        r#type: fallback_type,
    })
}

fn parse_message_event_base(
    envelope: &serde_json::Value,
    event: &serde_json::Value,
    retry: Option<SlackRetry>,
) -> SlackEventBase {
    let channel_id =
        string_value(event.get("channel").unwrap_or(&serde_json::Value::Null)).to_string();
    let ts = string_value(event.get("ts").unwrap_or(&serde_json::Value::Null)).to_string();
    let raw_thread_ts =
        string_value(event.get("thread_ts").unwrap_or(&serde_json::Value::Null)).to_string();
    let thread_ts = if raw_thread_ts.is_empty() {
        ts.clone()
    } else {
        raw_thread_ts
    };

    let team_id = optional_string(event.get("team_id").unwrap_or(&serde_json::Value::Null))
        .map(str::to_string)
        .or_else(|| {
            optional_string(envelope.get("team_id").unwrap_or(&serde_json::Value::Null))
                .map(str::to_string)
        });
    let enterprise_id = optional_string(
        envelope
            .get("enterprise_id")
            .unwrap_or(&serde_json::Value::Null),
    )
    .map(str::to_string)
    .or_else(|| {
        optional_string(
            envelope
                .get("context_enterprise_id")
                .unwrap_or(&serde_json::Value::Null),
        )
        .map(str::to_string)
    });

    let continuation = SlackContinuation {
        channel_id: channel_id.clone(),
        enterprise_id: enterprise_id.clone(),
        team_id: team_id.clone(),
        thread_ts: thread_ts.clone(),
    };

    let api_app_id = optional_string(
        envelope
            .get("api_app_id")
            .unwrap_or(&serde_json::Value::Null),
    )
    .map(str::to_string);
    let event_id = optional_string(envelope.get("event_id").unwrap_or(&serde_json::Value::Null))
        .map(str::to_string);
    let event_time = envelope.get("event_time").and_then(|v| v.as_i64());
    let is_ext_shared_channel = envelope
        .get("is_ext_shared_channel")
        .and_then(|v| v.as_bool());
    let text = string_value(event.get("text").unwrap_or(&serde_json::Value::Null)).to_string();
    let user_id =
        optional_string(event.get("user").unwrap_or(&serde_json::Value::Null)).map(str::to_string);

    SlackEventBase {
        api_app_id,
        channel_id,
        continuation,
        enterprise_id,
        event_id,
        event_time,
        is_ext_shared_channel,
        raw: event.clone(),
        retry,
        team_id,
        text,
        thread_ts,
        ts,
        user_id,
    }
}

fn classify_interaction_payload(
    raw: serde_json::Value,
    retry: Option<SlackRetry>,
) -> SlackWebhookPayload {
    if !raw.is_object() {
        return SlackWebhookPayload::Unsupported(SlackUnsupportedPayload {
            raw,
            retry,
            r#type: "interaction".to_string(),
        });
    }
    let r#type = raw.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match r#type {
        "block_actions" => SlackWebhookPayload::BlockActions(parse_block_actions(raw, retry)),
        "block_suggestion" => {
            SlackWebhookPayload::BlockSuggestion(parse_block_suggestion(raw, retry))
        }
        "view_submission" => SlackWebhookPayload::ViewSubmission(parse_view_submission(raw, retry)),
        "view_closed" => SlackWebhookPayload::ViewClosed(parse_view_closed(raw, retry)),
        _ => {
            let fallback_type = if r#type.is_empty() {
                "interaction".to_string()
            } else {
                r#type.to_string()
            };
            SlackWebhookPayload::Unsupported(SlackUnsupportedPayload {
                raw,
                retry,
                r#type: fallback_type,
            })
        }
    }
}

fn parse_slash_command(
    params: &[(String, String)],
    retry: Option<SlackRetry>,
) -> SlackSlashCommandPayload {
    let get = |k: &str| {
        params
            .iter()
            .find(|(kk, _)| kk == k)
            .map(|(_, v)| v.clone())
    };
    let enterprise_id = get("enterprise_id").filter(|s| !s.is_empty());
    let team_id = get("team_id").filter(|s| !s.is_empty());
    let raw_map: std::collections::BTreeMap<String, String> =
        params.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    SlackSlashCommandPayload {
        channel_id: get("channel_id").unwrap_or_default(),
        channel_name: get("channel_name").filter(|s| !s.is_empty()),
        command: get("command").unwrap_or_default(),
        enterprise_id,
        is_enterprise_install: get("is_enterprise_install").as_deref() == Some("true"),
        raw: raw_map,
        response_url: get("response_url").filter(|s| !s.is_empty()),
        retry,
        team_id,
        text: get("text").unwrap_or_default(),
        trigger_id: get("trigger_id").filter(|s| !s.is_empty()),
        user_id: get("user_id").unwrap_or_default(),
        user_name: get("user_name").filter(|s| !s.is_empty()),
    }
}

fn parse_block_actions(
    raw: serde_json::Value,
    retry: Option<SlackRetry>,
) -> SlackBlockActionsPayload {
    let channel = record_field(&raw, "channel");
    let container = record_field(&raw, "container");
    let message = record_field(&raw, "message");
    let user = record_field(&raw, "user");
    let team = record_field(&raw, "team");
    let enterprise = record_field(&raw, "enterprise");

    let channel_id = optional_in(channel, "id")
        .or_else(|| optional_in(container, "channel_id"))
        .map(str::to_string);
    let message_ts = optional_in(message, "ts")
        .or_else(|| optional_in(container, "message_ts"))
        .map(str::to_string);
    let thread_ts = optional_in(message, "thread_ts")
        .or_else(|| optional_in(container, "thread_ts"))
        .map(str::to_string)
        .or_else(|| message_ts.clone());
    let team_id = optional_in(team, "id")
        .or_else(|| optional_in(user, "team_id"))
        .map(str::to_string);
    let enterprise_id = optional_in(enterprise, "id")
        .or_else(|| optional_in(team, "enterprise_id"))
        .map(str::to_string);

    let continuation = match (&channel_id, &thread_ts) {
        (Some(c), Some(t)) => Some(SlackContinuation {
            channel_id: c.clone(),
            enterprise_id: enterprise_id.clone(),
            team_id: team_id.clone(),
            thread_ts: t.clone(),
        }),
        _ => None,
    };

    let actions = raw
        .get("actions")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(parse_action).collect())
        .unwrap_or_default();

    let is_enterprise_install = raw.get("is_enterprise_install").and_then(|v| v.as_bool());
    let response_url = raw
        .get("response_url")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let trigger_id = raw
        .get("trigger_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let user_id = optional_in(user, "id").unwrap_or("").to_string();
    let user_name = optional_in(user, "username")
        .or_else(|| optional_in(user, "name"))
        .map(str::to_string);

    SlackBlockActionsPayload {
        actions,
        channel_id,
        continuation,
        enterprise_id,
        is_enterprise_install,
        message_ts,
        raw,
        response_url,
        retry,
        team_id,
        thread_ts,
        trigger_id,
        user_id,
        user_name,
    }
}

fn parse_action(action: &serde_json::Value) -> SlackAction {
    let raw = if action.is_object() {
        action.clone()
    } else {
        serde_json::json!({})
    };
    let selected_option = record_field(&raw, "selected_option");
    let text = record_field(&raw, "text");
    SlackAction {
        action_id: raw
            .get("action_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        block_id: raw
            .get("block_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        label: optional_in(text, "text").map(str::to_string),
        raw: raw.clone(),
        selected_option_value: optional_in(selected_option, "value").map(str::to_string),
        r#type: raw
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        value: raw
            .get("value")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    }
}

fn parse_block_suggestion(
    raw: serde_json::Value,
    retry: Option<SlackRetry>,
) -> SlackBlockSuggestionPayload {
    let channel = record_field(&raw, "channel");
    let team = record_field(&raw, "team");
    let enterprise = record_field(&raw, "enterprise");
    let user = record_field(&raw, "user");
    SlackBlockSuggestionPayload {
        action_id: raw
            .get("action_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        block_id: raw
            .get("block_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        channel_id: optional_in(channel, "id").map(str::to_string),
        enterprise_id: optional_in(enterprise, "id")
            .or_else(|| optional_in(team, "enterprise_id"))
            .map(str::to_string),
        raw: raw.clone(),
        retry,
        team_id: optional_in(team, "id").map(str::to_string),
        user_id: optional_in(user, "id").unwrap_or("").to_string(),
        value: raw
            .get("value")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

fn parse_view_submission(
    raw: serde_json::Value,
    retry: Option<SlackRetry>,
) -> SlackViewSubmissionPayload {
    let team = record_field(&raw, "team");
    let enterprise = record_field(&raw, "enterprise");
    let user = record_field(&raw, "user");
    let view_val = raw
        .get("view")
        .cloned()
        .filter(|v| v.is_object())
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
    let view = view_val.as_object().cloned().unwrap_or_default();
    let response_urls = view
        .get("response_urls")
        .and_then(|v| v.as_array())
        .cloned();
    SlackViewSubmissionPayload {
        enterprise_id: optional_in(enterprise, "id")
            .or_else(|| optional_in(team, "enterprise_id"))
            .map(str::to_string),
        raw: raw.clone(),
        response_urls,
        retry,
        team_id: optional_in(team, "id").map(str::to_string),
        user_id: optional_in(user, "id").unwrap_or("").to_string(),
        view,
    }
}

fn parse_view_closed(raw: serde_json::Value, retry: Option<SlackRetry>) -> SlackViewClosedPayload {
    let team = record_field(&raw, "team");
    let enterprise = record_field(&raw, "enterprise");
    let user = record_field(&raw, "user");
    let view_val = raw
        .get("view")
        .cloned()
        .filter(|v| v.is_object())
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
    let view = view_val.as_object().cloned().unwrap_or_default();
    SlackViewClosedPayload {
        enterprise_id: optional_in(enterprise, "id")
            .or_else(|| optional_in(team, "enterprise_id"))
            .map(str::to_string),
        raw: raw.clone(),
        retry,
        team_id: optional_in(team, "id").map(str::to_string),
        user_id: optional_in(user, "id").unwrap_or("").to_string(),
        view,
    }
}

/// Helper: extract a non-null object field as a reference for the
/// upstream `recordValue(raw.xxx)` pattern.
fn record_field<'a>(raw: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    raw.get(key).filter(|v| v.is_object())
}

/// Helper for `optionalString(obj?.field)` upstream chains.
fn optional_in<'a>(obj: Option<&'a serde_json::Value>, key: &str) -> Option<&'a str> {
    obj.and_then(|o| o.get(key))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
}

/// Minimal URL-encoded `application/x-www-form-urlencoded` pair
/// parser. Matches the byte shape of `URLSearchParams(body)` on the
/// subset Slack actually emits (form fields with `+`-space, `%HH`
/// triples, and raw alphanumerics).
fn parse_form_pairs(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for pair in body.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut it = pair.splitn(2, '=');
        let k = it.next().unwrap_or("");
        let v = it.next().unwrap_or("");
        out.push((url_decode(k), url_decode(v)));
    }
    out
}

fn url_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
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
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- getHeader / getRetry (additive) ----------
    // No standalone upstream tests; the helpers are exercised through
    // parse.test.ts. The Rust suite asserts the case-insensitive
    // lookup behavior and the retry-num parse-or-skip path directly.

    fn h(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn get_header_is_case_insensitive() {
        let hs = h(&[("Content-Type", "application/json")]);
        assert_eq!(get_header(&hs, "content-type"), Some("application/json"));
        assert_eq!(get_header(&hs, "CONTENT-TYPE"), Some("application/json"));
    }

    #[test]
    fn get_header_returns_none_for_missing() {
        let hs = h(&[("Content-Type", "application/json")]);
        assert_eq!(get_header(&hs, "x-slack-signature"), None);
    }

    #[test]
    fn get_retry_parses_num_and_reason() {
        let hs = h(&[
            ("X-Slack-Retry-Num", "2"),
            ("X-Slack-Retry-Reason", "timeout"),
        ]);
        let retry = get_retry(&hs).expect("retry present");
        assert_eq!(retry.num, 2);
        assert_eq!(retry.reason.as_deref(), Some("timeout"));
    }

    #[test]
    fn get_retry_returns_none_when_num_header_missing() {
        let hs = h(&[("X-Slack-Retry-Reason", "timeout")]);
        assert!(get_retry(&hs).is_none());
    }

    #[test]
    fn get_retry_returns_none_when_num_not_finite() {
        let hs = h(&[("X-Slack-Retry-Num", "not-a-number")]);
        assert!(get_retry(&hs).is_none());
    }

    #[test]
    fn get_retry_carries_no_reason_when_reason_header_missing() {
        let hs = h(&[("X-Slack-Retry-Num", "5")]);
        let retry = get_retry(&hs).expect("retry present");
        assert_eq!(retry.num, 5);
        assert_eq!(retry.reason, None);
    }

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
        assert_eq!(optional_string(&serde_json::json!("hello")), Some("hello"));
    }

    #[test]
    fn optional_string_returns_none_for_empty_or_non_string() {
        assert!(optional_string(&serde_json::json!("")).is_none());
        assert!(optional_string(&serde_json::json!(null)).is_none());
        assert!(optional_string(&serde_json::json!(123)).is_none());
        assert!(optional_string(&serde_json::json!({})).is_none());
    }

    // ---------- import boundary (1 upstream case from webhook/boundary.test.ts) ----------

    #[test]
    fn webhook_import_boundary_does_not_pull_in_chat_or_adapter_shared() {
        // 1:1 with upstream `packages/adapter-slack/src/webhook/
        // boundary.test.ts`. Forbidden import strings built with
        // concat! so the test body doesn't match itself.
        let source = include_str!("webhook.rs");
        let forbidden = [
            concat!("use ", "chat_sdk_chat::"),
            concat!("use ", "chat_sdk_adapter_shared::"),
            concat!("use ", "tokio::"),
            concat!("use ", "reqwest::"),
        ];
        for f in forbidden {
            assert!(
                !source.contains(f),
                "webhook.rs must not import {f:?} (edge-runtime portable)"
            );
        }
    }

    // ---------- verify_slack_signature_value ----------

    fn sign(body: &str, secret: &str, timestamp: &str) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(b"v0:");
        mac.update(timestamp.as_bytes());
        mac.update(b":");
        mac.update(body.as_bytes());
        let bytes = mac.finalize().into_bytes();
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        format!("v0={hex}")
    }

    #[test]
    fn verify_slack_signature_value_accepts_correct_signature() {
        let body = "token=abc&team_id=T123";
        let secret = "test-secret";
        let ts = "1700000000";
        let sig = sign(body, secret, ts);
        assert!(verify_slack_signature_value(body, secret, ts, &sig));
    }

    #[test]
    fn verify_slack_signature_value_rejects_wrong_secret() {
        let body = "x";
        let ts = "1700000000";
        let sig = sign(body, "secret-a", ts);
        assert!(!verify_slack_signature_value(body, "secret-b", ts, &sig));
    }

    #[test]
    fn verify_slack_signature_value_rejects_tampered_body() {
        let secret = "s";
        let ts = "1700000000";
        let sig = sign("original", secret, ts);
        assert!(!verify_slack_signature_value("tampered", secret, ts, &sig));
    }

    #[test]
    fn verify_slack_signature_value_rejects_non_v0_prefix() {
        let secret = "s";
        let ts = "1700000000";
        let sig = sign("x", secret, ts);
        let no_prefix = sig.trim_start_matches("v0=").to_string();
        assert!(!verify_slack_signature_value("x", secret, ts, &no_prefix));
    }

    #[test]
    fn verify_slack_signature_value_rejects_non_hex_signature() {
        assert!(!verify_slack_signature_value("x", "s", "0", "v0=zzz"));
    }

    // ---------- verify_slack_signature (high-level) ----------

    #[test]
    fn verify_slack_signature_passes_within_skew() {
        let body = "x";
        let secret = "s";
        let ts = "1700000000";
        let sig = sign(body, secret, ts);
        let opts = SlackVerifyOptions {
            signing_secret: secret.to_string(),
            max_skew_seconds: Some(300),
            now_seconds: Some(1_700_000_010),
        };
        assert_eq!(
            verify_slack_signature(body, Some(ts), Some(&sig), &opts),
            Ok(())
        );
    }

    #[test]
    fn verify_slack_signature_rejects_too_old_timestamp() {
        let body = "x";
        let secret = "s";
        let ts = "1700000000";
        let sig = sign(body, secret, ts);
        let opts = SlackVerifyOptions {
            signing_secret: secret.to_string(),
            max_skew_seconds: Some(60),
            now_seconds: Some(1_700_000_200),
        };
        assert_eq!(
            verify_slack_signature(body, Some(ts), Some(&sig), &opts),
            Err(SlackWebhookVerificationError::TimestampTooOld)
        );
    }

    #[test]
    fn verify_slack_signature_rejects_missing_headers() {
        let opts = SlackVerifyOptions {
            signing_secret: "s".to_string(),
            max_skew_seconds: None,
            now_seconds: None,
        };
        assert_eq!(
            verify_slack_signature("x", None, Some("v0=00"), &opts),
            Err(SlackWebhookVerificationError::MissingHeaders)
        );
        assert_eq!(
            verify_slack_signature("x", Some("1"), None, &opts),
            Err(SlackWebhookVerificationError::MissingHeaders)
        );
    }

    #[test]
    fn verify_slack_signature_rejects_missing_secret() {
        let opts = SlackVerifyOptions {
            signing_secret: String::new(),
            max_skew_seconds: None,
            now_seconds: None,
        };
        assert_eq!(
            verify_slack_signature("x", Some("1"), Some("v0=00"), &opts),
            Err(SlackWebhookVerificationError::MissingSecret)
        );
    }

    #[test]
    fn verify_slack_signature_rejects_non_numeric_timestamp() {
        let opts = SlackVerifyOptions {
            signing_secret: "s".to_string(),
            max_skew_seconds: None,
            now_seconds: Some(0),
        };
        assert_eq!(
            verify_slack_signature("x", Some("not-a-number"), Some("v0=00"), &opts),
            Err(SlackWebhookVerificationError::InvalidTimestamp)
        );
    }

    #[test]
    fn verify_slack_signature_rejects_signature_mismatch() {
        let body = "x";
        let ts = "1700000000";
        let opts = SlackVerifyOptions {
            signing_secret: "s".to_string(),
            max_skew_seconds: Some(300),
            now_seconds: Some(1_700_000_000),
        };
        // Mismatched 64-hex signature should still be rejected.
        let sig: String = format!("v0={}", "0".repeat(64));
        assert_eq!(
            verify_slack_signature(body, Some(ts), Some(&sig), &opts),
            Err(SlackWebhookVerificationError::SignatureMismatch)
        );
    }

    // ============================================================
    // describe("parseSlackWebhookBody") (11 upstream cases)
    // 1:1 with upstream `webhook/index.test.ts > describe("parseSlackWebhookBody")`.
    // ============================================================

    /// Build `payload=<JSON>` form-encoded body — matches upstream's
    /// `new URLSearchParams({ payload: JSON.stringify(raw) }).toString()`.
    fn form_payload(payload: &serde_json::Value) -> String {
        let json = payload.to_string();
        let mut out = String::from("payload=");
        for &b in json.as_bytes() {
            match b {
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
                other => out.push_str(&format!("%{other:02X}")),
            }
        }
        out
    }

    fn url_encode_param(s: &str) -> String {
        let mut out = String::new();
        for &b in s.as_bytes() {
            match b {
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
                other => out.push_str(&format!("%{other:02X}")),
            }
        }
        out
    }

    fn json_opts<'a>() -> SlackParseOptions<'a> {
        SlackParseOptions {
            content_type: Some("application/json"),
            headers: None,
        }
    }

    fn form_opts<'a>() -> SlackParseOptions<'a> {
        SlackParseOptions {
            content_type: Some("application/x-www-form-urlencoded"),
            headers: None,
        }
    }

    #[test]
    fn parse_slack_webhook_body_parses_url_verification_payloads() {
        // 1:1 with upstream `parseSlackWebhookBody > parses url verification payloads`.
        let body = serde_json::json!({
            "challenge": "3eZbrw1aBm2rZgRNFdxV2595E9CY3gmdALWMmHkvFXO7tYXAYM8P",
            "token": "deprecated",
            "type": "url_verification",
        })
        .to_string();
        let payload = parse_slack_webhook_body(&body, &json_opts()).unwrap();
        assert_eq!(payload.kind(), "url_verification");
        match payload {
            SlackWebhookPayload::UrlVerification(p) => {
                assert_eq!(
                    p.challenge,
                    "3eZbrw1aBm2rZgRNFdxV2595E9CY3gmdALWMmHkvFXO7tYXAYM8P"
                );
            }
            _ => panic!("expected url_verification"),
        }
    }

    #[test]
    fn parse_slack_webhook_body_parses_app_mentions_with_continuation() {
        // 1:1 with upstream `parses app mentions with provider-native continuation`.
        let body = serde_json::json!({
            "api_app_id": "A123",
            "event": {
                "channel": "C123",
                "text": "<@U999> hello",
                "thread_ts": "1710000000.000001",
                "ts": "1710000000.000002",
                "type": "app_mention",
                "user": "U123",
            },
            "event_id": "Ev123",
            "event_time": 1_710_000_000,
            "is_ext_shared_channel": true,
            "team_id": "T123",
            "type": "event_callback",
        })
        .to_string();
        let headers = vec![
            ("x-slack-retry-num".to_string(), "2".to_string()),
            (
                "x-slack-retry-reason".to_string(),
                "http_timeout".to_string(),
            ),
        ];
        let opts = SlackParseOptions {
            content_type: Some("application/json"),
            headers: Some(&headers),
        };
        let payload = parse_slack_webhook_body(&body, &opts).unwrap();
        match payload {
            SlackWebhookPayload::AppMention(p) => {
                assert_eq!(p.base.api_app_id.as_deref(), Some("A123"));
                assert_eq!(p.base.channel_id, "C123");
                assert_eq!(p.base.continuation.channel_id, "C123");
                assert_eq!(p.base.continuation.team_id.as_deref(), Some("T123"));
                assert_eq!(p.base.continuation.thread_ts, "1710000000.000001");
                assert_eq!(p.base.event_id.as_deref(), Some("Ev123"));
                assert_eq!(p.base.event_time, Some(1_710_000_000));
                assert_eq!(p.base.is_ext_shared_channel, Some(true));
                assert_eq!(
                    p.base.retry,
                    Some(SlackRetry {
                        num: 2,
                        reason: Some("http_timeout".to_string()),
                    })
                );
                assert_eq!(p.base.text, "<@U999> hello");
                assert_eq!(p.base.thread_ts, "1710000000.000001");
                assert_eq!(p.base.ts, "1710000000.000002");
                assert_eq!(p.base.user_id.as_deref(), Some("U123"));
            }
            _ => panic!("expected app_mention"),
        }
    }

    #[test]
    fn parse_slack_webhook_body_uses_ts_as_thread_ts_when_app_mentions_are_top_level() {
        // 1:1 with upstream `uses ts as threadTs when app mentions are top-level messages`.
        let body = serde_json::json!({
            "event": {
                "channel": "C123",
                "text": "hello",
                "ts": "1710000000.000002",
                "type": "app_mention",
                "user": "U123",
            },
            "team_id": "T123",
            "type": "event_callback",
        })
        .to_string();
        let payload = parse_slack_webhook_body(&body, &json_opts()).unwrap();
        match payload {
            SlackWebhookPayload::AppMention(p) => {
                assert_eq!(p.base.continuation.channel_id, "C123");
                assert_eq!(p.base.continuation.thread_ts, "1710000000.000002");
                assert_eq!(p.base.thread_ts, "1710000000.000002");
            }
            _ => panic!("expected app_mention"),
        }
    }

    #[test]
    fn parse_slack_webhook_body_parses_direct_message_events() {
        // 1:1 with upstream `parses direct message events`.
        let body = serde_json::json!({
            "event": {
                "bot_id": "B123",
                "channel": "D123",
                "channel_type": "im",
                "subtype": "bot_message",
                "text": "hello",
                "ts": "1710000000.000002",
                "type": "message",
                "user": "U123",
            },
            "team_id": "T123",
            "type": "event_callback",
        })
        .to_string();
        // No content-type → sniff falls back to JSON (starts with `{`).
        let opts = SlackParseOptions::default();
        let payload = parse_slack_webhook_body(&body, &opts).unwrap();
        match payload {
            SlackWebhookPayload::DirectMessage(p) => {
                assert_eq!(p.bot_id.as_deref(), Some("B123"));
                assert_eq!(p.base.channel_id, "D123");
                assert_eq!(p.subtype.as_deref(), Some("bot_message"));
            }
            _ => panic!("expected direct_message"),
        }
    }

    #[test]
    fn parse_slack_webhook_body_parses_slash_command_form_posts() {
        // 1:1 with upstream `parses slash command form posts`.
        let pairs = [
            ("channel_id", "C123"),
            ("channel_name", "general"),
            ("command", "/deploy"),
            ("enterprise_id", "E123"),
            ("is_enterprise_install", "true"),
            (
                "response_url",
                "https://hooks.slack.com/commands/T123/1/abc",
            ),
            ("team_id", "T123"),
            ("text", "prod"),
            ("trigger_id", "123.456.abc"),
            ("user_id", "U123"),
            ("user_name", "josh"),
        ];
        let body: String = pairs
            .iter()
            .map(|(k, v)| format!("{}={}", url_encode_param(k), url_encode_param(v)))
            .collect::<Vec<_>>()
            .join("&");
        let payload = parse_slack_webhook_body(&body, &form_opts()).unwrap();
        match payload {
            SlackWebhookPayload::SlashCommand(p) => {
                assert_eq!(p.channel_id, "C123");
                assert_eq!(p.channel_name.as_deref(), Some("general"));
                assert_eq!(p.command, "/deploy");
                assert_eq!(p.enterprise_id.as_deref(), Some("E123"));
                assert!(p.is_enterprise_install);
                assert_eq!(
                    p.response_url.as_deref(),
                    Some("https://hooks.slack.com/commands/T123/1/abc")
                );
                assert_eq!(p.team_id.as_deref(), Some("T123"));
                assert_eq!(p.text, "prod");
                assert_eq!(p.trigger_id.as_deref(), Some("123.456.abc"));
                assert_eq!(p.user_id, "U123");
                assert_eq!(p.user_name.as_deref(), Some("josh"));
                // raw retains every form pair.
                assert_eq!(p.raw.get("channel_id").map(String::as_str), Some("C123"));
                assert_eq!(p.raw.get("command").map(String::as_str), Some("/deploy"));
            }
            _ => panic!("expected slash_command"),
        }
    }

    #[test]
    fn parse_slack_webhook_body_parses_block_action_payloads() {
        // 1:1 with upstream `parses block action payloads`.
        let raw = serde_json::json!({
            "actions": [
                {
                    "action_id": "approve",
                    "block_id": "actions",
                    "selected_option": { "value": "yes" },
                    "text": { "text": "Approve", "type": "plain_text" },
                    "type": "button",
                    "value": "approve-value",
                }
            ],
            "channel": { "id": "C123", "name": "general" },
            "container": {
                "channel_id": "C123",
                "message_ts": "1710000000.000002",
                "thread_ts": "1710000000.000001",
                "type": "message",
            },
            "message": {
                "thread_ts": "1710000000.000001",
                "ts": "1710000000.000002",
            },
            "response_url": "https://hooks.slack.com/actions/T123/1/abc",
            "team": { "enterprise_id": "E123", "id": "T123" },
            "trigger_id": "123.456.abc",
            "type": "block_actions",
            "user": { "id": "U123", "username": "josh" },
        });
        let body = form_payload(&raw);
        let payload = parse_slack_webhook_body(&body, &form_opts()).unwrap();
        match payload {
            SlackWebhookPayload::BlockActions(p) => {
                assert_eq!(p.actions.len(), 1);
                assert_eq!(p.actions[0].action_id, "approve");
                assert_eq!(p.actions[0].block_id.as_deref(), Some("actions"));
                assert_eq!(p.actions[0].label.as_deref(), Some("Approve"));
                assert_eq!(p.actions[0].selected_option_value.as_deref(), Some("yes"));
                assert_eq!(p.actions[0].r#type, "button");
                assert_eq!(p.actions[0].value.as_deref(), Some("approve-value"));
                assert_eq!(p.channel_id.as_deref(), Some("C123"));
                let cont = p.continuation.as_ref().unwrap();
                assert_eq!(cont.channel_id, "C123");
                assert_eq!(cont.enterprise_id.as_deref(), Some("E123"));
                assert_eq!(cont.team_id.as_deref(), Some("T123"));
                assert_eq!(cont.thread_ts, "1710000000.000001");
                assert_eq!(p.message_ts.as_deref(), Some("1710000000.000002"));
                assert_eq!(
                    p.response_url.as_deref(),
                    Some("https://hooks.slack.com/actions/T123/1/abc")
                );
                assert_eq!(p.team_id.as_deref(), Some("T123"));
                assert_eq!(p.thread_ts.as_deref(), Some("1710000000.000001"));
                assert_eq!(p.trigger_id.as_deref(), Some("123.456.abc"));
                assert_eq!(p.user_id, "U123");
            }
            _ => panic!("expected block_actions"),
        }
    }

    #[test]
    fn parse_slack_webhook_body_parses_block_suggestion_payloads() {
        // 1:1 with upstream `parses block suggestion payloads`.
        let raw = serde_json::json!({
            "action_id": "external",
            "block_id": "input",
            "channel": { "id": "C123" },
            "enterprise": { "id": "E123" },
            "team": { "id": "T123" },
            "type": "block_suggestion",
            "user": { "id": "U123" },
            "value": "hel",
        });
        let body = form_payload(&raw);
        let payload = parse_slack_webhook_body(&body, &form_opts()).unwrap();
        match payload {
            SlackWebhookPayload::BlockSuggestion(p) => {
                assert_eq!(p.action_id, "external");
                assert_eq!(p.block_id, "input");
                assert_eq!(p.channel_id.as_deref(), Some("C123"));
                assert_eq!(p.enterprise_id.as_deref(), Some("E123"));
                assert_eq!(p.team_id.as_deref(), Some("T123"));
                assert_eq!(p.user_id, "U123");
                assert_eq!(p.value, "hel");
            }
            _ => panic!("expected block_suggestion"),
        }
    }

    #[test]
    fn parse_slack_webhook_body_parses_view_submissions() {
        // 1:1 with upstream `parses view submissions`.
        let raw = serde_json::json!({
            "team": { "id": "T123" },
            "type": "view_submission",
            "user": { "id": "U123" },
            "view": {
                "callback_id": "feedback",
                "id": "V123",
                "response_urls": [
                    {
                        "action_id": "target",
                        "channel_id": "C123",
                        "response_url": "https://hooks.slack.com/app/1/2/3",
                    }
                ],
            },
        });
        let body = form_payload(&raw);
        let payload = parse_slack_webhook_body(&body, &form_opts()).unwrap();
        match payload {
            SlackWebhookPayload::ViewSubmission(p) => {
                assert_eq!(p.team_id.as_deref(), Some("T123"));
                assert_eq!(p.user_id, "U123");
                let urls = p.response_urls.as_ref().unwrap();
                assert_eq!(urls.len(), 1);
                assert_eq!(urls[0]["action_id"], "target");
                assert_eq!(urls[0]["channel_id"], "C123");
                assert_eq!(urls[0]["response_url"], "https://hooks.slack.com/app/1/2/3");
                assert_eq!(
                    p.view.get("callback_id").and_then(|v| v.as_str()),
                    Some("feedback")
                );
                assert_eq!(p.view.get("id").and_then(|v| v.as_str()), Some("V123"));
            }
            _ => panic!("expected view_submission"),
        }
    }

    #[test]
    fn parse_slack_webhook_body_parses_view_closed_payloads() {
        // 1:1 with upstream `parses view closed payloads`.
        let raw = serde_json::json!({
            "enterprise": { "id": "E123" },
            "team": null,
            "type": "view_closed",
            "user": { "id": "U123" },
            "view": { "id": "V123" },
        });
        let body = form_payload(&raw);
        let payload = parse_slack_webhook_body(&body, &form_opts()).unwrap();
        match payload {
            SlackWebhookPayload::ViewClosed(p) => {
                assert_eq!(p.enterprise_id.as_deref(), Some("E123"));
                assert_eq!(p.user_id, "U123");
                assert_eq!(p.view.get("id").and_then(|v| v.as_str()), Some("V123"));
            }
            _ => panic!("expected view_closed"),
        }
    }

    #[test]
    fn parse_slack_webhook_body_returns_unsupported_for_valid_but_unsupported_payloads() {
        // 1:1 with upstream `returns unsupported for valid but unsupported payloads`.
        let body = serde_json::json!({
            "event": { "type": "reaction_added" },
            "type": "event_callback",
        })
        .to_string();
        // No content-type → JSON sniff.
        let opts = SlackParseOptions::default();
        let payload = parse_slack_webhook_body(&body, &opts).unwrap();
        match payload {
            SlackWebhookPayload::Unsupported(p) => {
                assert_eq!(p.r#type, "reaction_added");
                assert!(p.retry.is_none());
                assert_eq!(
                    p.raw,
                    serde_json::json!({
                        "event": { "type": "reaction_added" },
                        "type": "event_callback",
                    })
                );
            }
            _ => panic!("expected unsupported"),
        }
    }

    #[test]
    fn parse_slack_webhook_body_throws_a_parse_error_for_invalid_json() {
        // 1:1 with upstream `throws a parse error for invalid json`.
        let err = parse_slack_webhook_body("{", &json_opts()).unwrap_err();
        assert_eq!(err.message(), "Slack webhook body is invalid JSON");
    }
}
