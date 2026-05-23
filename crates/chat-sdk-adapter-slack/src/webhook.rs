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
        assert_eq!(optional_string(&serde_json::json!("hello")), Some("hello"));
    }

    #[test]
    fn optional_string_returns_none_for_empty_or_non_string() {
        assert!(optional_string(&serde_json::json!("")).is_none());
        assert!(optional_string(&serde_json::json!(null)).is_none());
        assert!(optional_string(&serde_json::json!(123)).is_none());
        assert!(optional_string(&serde_json::json!({})).is_none());
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
}
