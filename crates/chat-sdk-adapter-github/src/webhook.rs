//! GitHub webhook signature verification.
//!
//! 1:1 port of upstream `GithubAdapter.verifySignature(body,
//! signature)`. GitHub signs webhook deliveries with HMAC-SHA256
//! over the raw request body using the configured **webhook
//! secret**, and ships the result in the `X-Hub-Signature-256`
//! header as `sha256=<hex>`.
//!
//! @see <https://docs.github.com/en/webhooks/using-webhooks/validating-webhook-deliveries>

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Verify a GitHub webhook signature. 1:1 with upstream
/// `verifySignature(body, signature)`:
///
/// - Returns `false` when `signature` is `None`/empty.
/// - Computes `sha256=<hex>(HMAC-SHA256(webhook_secret, body))`
///   and constant-time compares to the supplied signature over
///   the full prefixed string.
/// - Length-mismatched signatures are rejected before the compare
///   (upstream's `timingSafeEqual` throws on length mismatch and
///   the catch swallows it; both branches return `false`).
pub fn verify_github_signature(body: &str, webhook_secret: &str, signature: Option<&str>) -> bool {
    let Some(signature) = signature.filter(|s| !s.is_empty()) else {
        return false;
    };

    let Ok(mut mac) = HmacSha256::new_from_slice(webhook_secret.as_bytes()) else {
        return false;
    };
    mac.update(body.as_bytes());
    let computed = mac.finalize().into_bytes();
    let hex: String = computed.iter().map(|b| format!("{b:02x}")).collect();
    let expected = format!("sha256={hex}");

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
    fn accepts_correct_signature() {
        let body = r#"{"zen":"Mind your words..."}"#;
        let secret = "webhook-secret";
        let sig = sign(body, secret);
        assert!(verify_github_signature(body, secret, Some(&sig)));
    }

    #[test]
    fn rejects_wrong_secret() {
        let body = "x";
        let sig = sign(body, "wrong");
        assert!(!verify_github_signature(body, "right", Some(&sig)));
    }

    #[test]
    fn rejects_tampered_body() {
        let secret = "s";
        let sig = sign("original", secret);
        assert!(!verify_github_signature("tampered", secret, Some(&sig)));
    }

    #[test]
    fn rejects_missing_signature() {
        assert!(!verify_github_signature("body", "secret", None));
    }

    #[test]
    fn rejects_empty_signature() {
        assert!(!verify_github_signature("body", "secret", Some("")));
    }

    #[test]
    fn rejects_signature_without_sha256_prefix() {
        let body = "x";
        let secret = "s";
        let sig = sign(body, secret);
        let no_prefix = sig.trim_start_matches("sha256=").to_string();
        assert!(!verify_github_signature(body, secret, Some(&no_prefix)));
    }

    #[test]
    fn rejects_length_mismatched_signature() {
        // Right prefix but truncated hex.
        assert!(!verify_github_signature("x", "s", Some("sha256=00")));
    }

    #[test]
    fn rejects_uppercased_hex_signature() {
        // The expected hex is lowercase; a mismatched-case sig fails
        // constant-time comparison (mirrors upstream Buffer compare).
        let body = "x";
        let secret = "s";
        let sig = sign(body, secret)
            .to_uppercase()
            .replace("SHA256=", "sha256=");
        assert!(!verify_github_signature(body, secret, Some(&sig)));
    }
}
