//! WhatsApp webhook signature verification.
//!
//! 1:1 port of upstream `WhatsAppAdapter.verifySignature(body,
//! signature)`. Meta signs webhook deliveries with HMAC-SHA256 over
//! the raw request body using the **App Secret**, and ships the
//! result in the `X-Hub-Signature-256` header as `sha256=<hex>`.
//!
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
}
