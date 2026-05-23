//! Discord interaction signature verification (Ed25519).
//!
//! 1:1 port of the `verifyKey` call from the
//! `discord-interactions` npm package that upstream's
//! `DiscordAdapter` uses to validate incoming interaction
//! webhooks. Discord signs each request with Ed25519 over the
//! concatenation of the `X-Signature-Timestamp` header and the
//! raw request body. The signed result is hex-encoded in the
//! `X-Signature-Ed25519` header, and the application's public
//! key is itself a 64-char hex string.
//!
//! @see <https://discord.com/developers/docs/interactions/receiving-and-responding#security-and-authorization>

use ed25519_dalek::{Signature, Verifier, VerifyingKey};

/// Verify a Discord interaction signature. 1:1 with upstream
/// `verifyKey(body, signature, timestamp, publicKey)`:
///
/// 1. Decode `signature` as 64 bytes of hex.
/// 2. Decode `public_key` as 32 bytes of hex.
/// 3. Construct the Ed25519 verifying key.
/// 4. Verify Ed25519 over the bytes `timestamp || body`.
///
/// Returns `false` for any decoding/verification failure (the
/// upstream impl swallows errors the same way).
pub fn verify_discord_signature(
    body: &[u8],
    signature_hex: &str,
    timestamp: &str,
    public_key_hex: &str,
) -> bool {
    let Some(signature_bytes) = decode_hex_exact(signature_hex, 64) else {
        return false;
    };
    let Some(public_key_bytes) = decode_hex_exact(public_key_hex, 32) else {
        return false;
    };
    let Ok(public_key_arr): Result<[u8; 32], _> = public_key_bytes.try_into() else {
        return false;
    };
    let Ok(verifying_key) = VerifyingKey::from_bytes(&public_key_arr) else {
        return false;
    };
    let Ok(signature_arr): Result<[u8; 64], _> = signature_bytes.try_into() else {
        return false;
    };
    let signature = Signature::from_bytes(&signature_arr);

    // Discord signs `timestamp || body` (timestamp as UTF-8 + raw body
    // bytes).
    let mut signed = Vec::with_capacity(timestamp.len() + body.len());
    signed.extend_from_slice(timestamp.as_bytes());
    signed.extend_from_slice(body);

    verifying_key.verify(&signed, &signature).is_ok()
}

fn decode_hex_exact(hex: &str, byte_len: usize) -> Option<Vec<u8>> {
    if hex.len() != byte_len * 2 {
        return None;
    }
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = Vec::with_capacity(byte_len);
    for i in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&hex[i..i + 2], 16).ok()?;
        out.push(byte);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signing_keypair_from_seed(seed: [u8; 32]) -> (SigningKey, String) {
        let sk = SigningKey::from_bytes(&seed);
        let public_hex: String = sk
            .verifying_key()
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        (sk, public_hex)
    }

    fn sign_request(sk: &SigningKey, timestamp: &str, body: &[u8]) -> String {
        let mut signed = Vec::with_capacity(timestamp.len() + body.len());
        signed.extend_from_slice(timestamp.as_bytes());
        signed.extend_from_slice(body);
        let signature = sk.sign(&signed);
        signature
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn accepts_correct_signature() {
        let (sk, public_hex) = signing_keypair_from_seed([1u8; 32]);
        let body = b"{\"type\":1}";
        let ts = "1700000000";
        let sig_hex = sign_request(&sk, ts, body);
        assert!(verify_discord_signature(body, &sig_hex, ts, &public_hex));
    }

    #[test]
    fn rejects_tampered_body() {
        let (sk, public_hex) = signing_keypair_from_seed([2u8; 32]);
        let ts = "1700000000";
        let sig_hex = sign_request(&sk, ts, b"original");
        assert!(!verify_discord_signature(
            b"tampered",
            &sig_hex,
            ts,
            &public_hex
        ));
    }

    #[test]
    fn rejects_tampered_timestamp() {
        let (sk, public_hex) = signing_keypair_from_seed([3u8; 32]);
        let body = b"x";
        let sig_hex = sign_request(&sk, "1700000000", body);
        assert!(!verify_discord_signature(
            body,
            &sig_hex,
            "1700000999",
            &public_hex
        ));
    }

    #[test]
    fn rejects_wrong_public_key() {
        let (sk, _) = signing_keypair_from_seed([4u8; 32]);
        let (_, other_public_hex) = signing_keypair_from_seed([5u8; 32]);
        let ts = "1700000000";
        let body = b"x";
        let sig_hex = sign_request(&sk, ts, body);
        assert!(!verify_discord_signature(
            body,
            &sig_hex,
            ts,
            &other_public_hex
        ));
    }

    #[test]
    fn rejects_non_hex_signature() {
        let (_, public_hex) = signing_keypair_from_seed([6u8; 32]);
        let bad_sig = "z".repeat(128);
        assert!(!verify_discord_signature(
            b"x",
            &bad_sig,
            "1700000000",
            &public_hex
        ));
    }

    #[test]
    fn rejects_wrong_length_signature() {
        let (_, public_hex) = signing_keypair_from_seed([7u8; 32]);
        // 32 hex chars = 16 bytes, not 64.
        let short = "0".repeat(32);
        assert!(!verify_discord_signature(
            b"x",
            &short,
            "1700000000",
            &public_hex
        ));
    }

    #[test]
    fn rejects_non_hex_public_key() {
        let bad_public = "z".repeat(64);
        assert!(!verify_discord_signature(
            b"x",
            &"0".repeat(128),
            "0",
            &bad_public
        ));
    }

    #[test]
    fn rejects_wrong_length_public_key() {
        // 64 hex chars but we need 64 bytes (128 chars).
        let short_pub = "0".repeat(60);
        assert!(!verify_discord_signature(
            b"x",
            &"0".repeat(128),
            "0",
            &short_pub
        ));
    }

    // ---------- helper ----------

    #[test]
    fn decode_hex_exact_returns_none_for_odd_length() {
        assert!(decode_hex_exact("abc", 2).is_none());
    }

    #[test]
    fn decode_hex_exact_returns_none_for_non_hex() {
        assert!(decode_hex_exact("zz", 1).is_none());
    }

    #[test]
    fn decode_hex_exact_decodes_lowercase_and_uppercase() {
        assert_eq!(decode_hex_exact("0a", 1), Some(vec![0x0a]));
        assert_eq!(decode_hex_exact("FF", 1), Some(vec![0xff]));
    }
}
