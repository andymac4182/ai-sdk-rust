//! WhatsApp webhook signature verification + GET verification
//! challenge.
//!
//! 1:1 port of upstream `WhatsAppAdapter.verifySignature(body,
//! signature)` + `WhatsAppAdapter.handleVerificationChallenge(request)`.
//! Meta signs webhook deliveries with HMAC-SHA256 over the raw
//! request body using the **App Secret**, and ships the result in
//! the `X-Hub-Signature-256` header as `sha256=<hex>`.
//!
//! The GET verification challenge is the one-time handshake Meta
//! sends when registering the webhook URL: it includes
//! `hub.mode=subscribe`, `hub.verify_token=<configured-token>`, and
//! `hub.challenge=<random-string>` as query params; on success the
//! adapter echoes the challenge string back with status 200.
//!
//! @see <https://developers.facebook.com/docs/whatsapp/cloud-api/guides/set-up-webhooks>
//! @see <https://developers.facebook.com/docs/graph-api/webhooks/getting-started#verification-requests>

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Verify a WhatsApp webhook signature. 1:1 with upstream
/// `verifySignature(body, signature)`:
///
/// - Returns `false` when `signature` is `None`/empty.
/// - Computes `sha256=<hex>(HMAC-SHA256(app_secret, body))`
///   and constant-time compares to the supplied signature.
/// - Comparison is over the full prefixed string (Upstream
///   uses `Buffer.from(signature)` vs `Buffer.from(expected)`).
pub fn verify_whatsapp_signature(body: &str, app_secret: &str, signature: Option<&str>) -> bool {
    let Some(signature) = signature.filter(|s| !s.is_empty()) else {
        return false;
    };

    let Ok(mut mac) = HmacSha256::new_from_slice(app_secret.as_bytes()) else {
        return false;
    };
    mac.update(body.as_bytes());
    let computed = mac.finalize().into_bytes();
    let hex: String = computed.iter().map(|b| format!("{b:02x}")).collect();
    let expected = format!("sha256={hex}");

    // Upstream's timingSafeEqual throws on length-mismatched
    // buffers and the catch swallows it; both branches return
    // false. Mirror that with an explicit length check before the
    // constant-time compare.
    if expected.len() != signature.len() {
        return false;
    }
    expected.as_bytes().ct_eq(signature.as_bytes()).into()
}

/// Inputs to the WhatsApp webhook GET verification challenge. 1:1
/// with upstream's `URL(request.url).searchParams.get(...)` reads of
/// `hub.mode`, `hub.verify_token`, and `hub.challenge`. Each field
/// is `Option` because Meta only sends them when the request is
/// actually a verification handshake (the absence of any param is
/// treated as failure).
#[derive(Debug, Clone, Default)]
pub struct WhatsappVerificationQuery<'a> {
    /// `hub.mode` query param. Meta sends `"subscribe"` for the
    /// initial handshake.
    pub hub_mode: Option<&'a str>,
    /// `hub.verify_token` query param. Must match the configured
    /// `verify_token` exactly.
    pub hub_verify_token: Option<&'a str>,
    /// `hub.challenge` query param. The random string Meta wants
    /// echoed back as the response body.
    pub hub_challenge: Option<&'a str>,
}

/// Outcome of a WhatsApp webhook verification challenge. 1:1 with
/// upstream's two-branch `handleVerificationChallenge` return:
///
/// - [`Self::Ok`] (HTTP 200) wraps the echoed challenge string —
///   upstream returns `new Response(challenge ?? "")` so an absent
///   `hub.challenge` collapses to an empty body.
/// - [`Self::Forbidden`] (HTTP 403) is returned on any mode/token
///   mismatch (or absent params).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WhatsappVerificationResponse {
    /// Successful handshake — body is the echoed `hub.challenge`
    /// (possibly empty per upstream's `?? ""` fallback).
    Ok(String),
    /// Verification rejected — upstream returns 403 with body
    /// `"Forbidden"`.
    Forbidden,
}

impl WhatsappVerificationResponse {
    /// HTTP status code 1:1 with upstream's `new Response(...,
    /// {status})`. 200 for [`Self::Ok`], 403 for [`Self::Forbidden`].
    pub fn status(&self) -> u16 {
        match self {
            Self::Ok(_) => 200,
            Self::Forbidden => 403,
        }
    }
}

/// 1:1 port of upstream
/// `WhatsAppAdapter.handleVerificationChallenge(request)`. Matches
/// when `hub_mode == "subscribe"` AND `hub_verify_token` equals the
/// configured `verify_token`; on success echoes `hub_challenge`
/// (collapsing `None` to an empty string per upstream's `challenge
/// ?? ""`). Any other combination — wrong mode, wrong/missing
/// token, missing query params — yields [`WhatsappVerificationResponse::Forbidden`].
pub fn handle_whatsapp_verification_challenge(
    query: &WhatsappVerificationQuery<'_>,
    verify_token: &str,
) -> WhatsappVerificationResponse {
    let mode_ok = query.hub_mode == Some("subscribe");
    let token_ok = query.hub_verify_token == Some(verify_token);
    if mode_ok && token_ok {
        let challenge = query.hub_challenge.unwrap_or("").to_string();
        return WhatsappVerificationResponse::Ok(challenge);
    }
    WhatsappVerificationResponse::Forbidden
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(body: &str, secret: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body.as_bytes());
        let bytes = mac.finalize().into_bytes();
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        format!("sha256={hex}")
    }

    #[test]
    fn verify_whatsapp_signature_accepts_correct_signature() {
        let body = r#"{"object":"whatsapp_business_account","entry":[]}"#;
        let secret = "app-secret-123";
        let sig = sign(body, secret);
        assert!(verify_whatsapp_signature(body, secret, Some(&sig)));
    }

    #[test]
    fn verify_whatsapp_signature_rejects_wrong_secret() {
        let body = "x";
        let secret = "right";
        let sig = sign(body, "wrong");
        assert!(!verify_whatsapp_signature(body, secret, Some(&sig)));
    }

    #[test]
    fn verify_whatsapp_signature_rejects_tampered_body() {
        let secret = "s";
        let sig = sign("original", secret);
        assert!(!verify_whatsapp_signature("tampered", secret, Some(&sig)));
    }

    #[test]
    fn verify_whatsapp_signature_rejects_missing_signature() {
        assert!(!verify_whatsapp_signature("body", "secret", None));
    }

    #[test]
    fn verify_whatsapp_signature_rejects_empty_signature() {
        assert!(!verify_whatsapp_signature("body", "secret", Some("")));
    }

    #[test]
    fn verify_whatsapp_signature_rejects_signature_without_sha256_prefix() {
        let body = "x";
        let secret = "s";
        let sig = sign(body, secret);
        // Strip the "sha256=" prefix.
        let no_prefix = sig.trim_start_matches("sha256=").to_string();
        assert!(!verify_whatsapp_signature(body, secret, Some(&no_prefix)));
    }

    #[test]
    fn verify_whatsapp_signature_rejects_length_mismatch() {
        // Truncated signature shouldn't be valid even if the prefix is right.
        assert!(!verify_whatsapp_signature("x", "s", Some("sha256=00")));
    }

    // ---------- handleWebhook - verification challenge (3 cases) ----------
    // 1:1 with upstream
    // `packages/adapter-whatsapp/src/index.test.ts > describe(
    // "handleWebhook - verification challenge")` (L504-L536). The
    // Rust port factors the inline `new URL(request.url).searchParams.get(...)`
    // reads out into a `WhatsappVerificationQuery` struct so the
    // pure logic can be unit-tested without constructing a full
    // HTTP `Request` value (Rust's HTTP types are framework-specific
    // — axum / hyper / reqwest each have their own `Request`).

    #[test]
    fn handle_webhook_verification_challenge_responds_to_valid_verification_challenge() {
        // 1:1 with upstream "should respond to valid verification challenge"
        // (L505-L515): GET with `hub.mode=subscribe`,
        // `hub.verify_token=test-verify-token`, `hub.challenge=1234567890`
        // -> 200 with body `1234567890`.
        let query = WhatsappVerificationQuery {
            hub_mode: Some("subscribe"),
            hub_verify_token: Some("test-verify-token"),
            hub_challenge: Some("1234567890"),
        };
        let response = handle_whatsapp_verification_challenge(&query, "test-verify-token");
        assert_eq!(response.status(), 200);
        match response {
            WhatsappVerificationResponse::Ok(body) => assert_eq!(body, "1234567890"),
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn handle_webhook_verification_challenge_rejects_invalid_verify_token() {
        // 1:1 with upstream "should reject invalid verify token"
        // (L517-L525): wrong `hub.verify_token` -> 403.
        let query = WhatsappVerificationQuery {
            hub_mode: Some("subscribe"),
            hub_verify_token: Some("wrong-token"),
            hub_challenge: Some("1234567890"),
        };
        let response = handle_whatsapp_verification_challenge(&query, "test-verify-token");
        assert_eq!(response.status(), 403);
        assert_eq!(response, WhatsappVerificationResponse::Forbidden);
    }

    #[test]
    fn handle_webhook_verification_challenge_rejects_wrong_mode() {
        // 1:1 with upstream "should reject wrong mode" (L527-L535):
        // `hub.mode=unsubscribe` -> 403 even when the verify_token
        // matches.
        let query = WhatsappVerificationQuery {
            hub_mode: Some("unsubscribe"),
            hub_verify_token: Some("test-verify-token"),
            hub_challenge: Some("1234567890"),
        };
        let response = handle_whatsapp_verification_challenge(&query, "test-verify-token");
        assert_eq!(response.status(), 403);
        assert_eq!(response, WhatsappVerificationResponse::Forbidden);
    }
}
