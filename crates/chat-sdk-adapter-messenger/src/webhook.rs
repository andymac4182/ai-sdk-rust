//! Messenger webhook signature verification.
//!
//! 1:1 port of upstream `MessengerAdapter.verifySignature(request,
//! body)`. Meta signs webhook deliveries with HMAC-SHA256 over the
//! raw request body using the **App Secret**, and ships the result
//! in the `X-Hub-Signature-256` header as `sha256=<hex>`. This
//! differs from WhatsApp's same-named header in the upstream
//! validation flow: Messenger splits the signature on `=` and
//! validates the `sha256` algorithm prefix explicitly, while
//! WhatsApp compares the full `sha256=<hex>` string.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Verify a Messenger webhook signature. 1:1 with upstream
/// `verifySignature(request, body)`:
///
/// 1. Reject if the `x-hub-signature-256` header is `None`/empty.
/// 2. Split on `=`. Reject if algorithm isn't `sha256` or the
///    hash is missing.
/// 3. Hex-decode the supplied hash. Reject if it isn't 32 bytes.
/// 4. Compute `HMAC-SHA256(app_secret, body)` and constant-time
///    compare to the decoded hash via the `subtle` crate.
pub fn verify_messenger_signature(body: &str, app_secret: &str, signature: Option<&str>) -> bool {
    let Some(signature) = signature.filter(|s| !s.is_empty()) else {
        return false;
    };
    let Some((algo, hex_hash)) = signature.split_once('=') else {
        return false;
    };
    if algo != "sha256" || hex_hash.is_empty() {
        return false;
    }
    if hex_hash.len() % 2 != 0 || !hex_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let received: Vec<u8> = (0..hex_hash.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&hex_hash[i..i + 2], 16).ok())
        .collect();
    if received.len() != 32 {
        return false;
    }

    let Ok(mut mac) = HmacSha256::new_from_slice(app_secret.as_bytes()) else {
        return false;
    };
    mac.update(body.as_bytes());
    let computed = mac.finalize().into_bytes();
    computed.as_slice().ct_eq(&received).into()
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
        let body = r#"{"object":"page","entry":[]}"#;
        let secret = "app-secret-123";
        let sig = sign(body, secret);
        assert!(verify_messenger_signature(body, secret, Some(&sig)));
    }

    #[test]
    fn rejects_wrong_secret() {
        let body = "x";
        let sig = sign(body, "wrong");
        assert!(!verify_messenger_signature(body, "right", Some(&sig)));
    }

    #[test]
    fn rejects_tampered_body() {
        let secret = "s";
        let sig = sign("original", secret);
        assert!(!verify_messenger_signature("tampered", secret, Some(&sig)));
    }

    #[test]
    fn rejects_missing_signature() {
        assert!(!verify_messenger_signature("x", "s", None));
    }

    #[test]
    fn rejects_empty_signature() {
        assert!(!verify_messenger_signature("x", "s", Some("")));
    }

    #[test]
    fn rejects_wrong_algorithm() {
        let body = "x";
        let secret = "s";
        // Use a sha256 hex but label it sha1.
        let real = sign(body, secret);
        let hex = real.trim_start_matches("sha256=");
        let fake = format!("sha1={hex}");
        assert!(!verify_messenger_signature(body, secret, Some(&fake)));
    }

    #[test]
    fn rejects_missing_hash() {
        assert!(!verify_messenger_signature("x", "s", Some("sha256=")));
    }

    #[test]
    fn rejects_missing_equals_separator() {
        // No '=' at all.
        let real_sig = sign("x", "s");
        let no_eq = real_sig.replace('=', "");
        assert!(!verify_messenger_signature("x", "s", Some(&no_eq)));
    }

    #[test]
    fn rejects_non_hex_hash() {
        assert!(!verify_messenger_signature(
            "x",
            "s",
            Some("sha256=not-hex!!")
        ));
    }

    #[test]
    fn rejects_truncated_hash() {
        assert!(!verify_messenger_signature("x", "s", Some("sha256=00")));
    }
}
