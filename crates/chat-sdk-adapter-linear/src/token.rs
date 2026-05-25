//! Linear OAuth token-encryption helpers.
//!
//! Upstream `packages/adapter-linear/src/index.ts` encrypts the
//! per-installation `accessToken` / `refreshToken` at rest in the
//! state store using AES-256-GCM (configured via the `encryptionKey`
//! constructor option). The `describe("multi-tenant installations >
//! token encryption")` block (index.test.ts L3613-L3731) covers four
//! cases:
//!
//! 1. `encrypts accessToken and refreshToken at rest in the state
//!    store` — full round-trip through state.set + state.get with
//!    an AES-256-GCM envelope `{ iv, data, tag }`.
//! 2. `stores plaintext when no encryptionKey is configured (legacy
//!    behavior)` — same round-trip with no key.
//! 3. `getInstallation tolerates legacy plaintext records when
//!    encryption is enabled (zero-downtime key rotation in)` — same
//!    round-trip with a key but reading a pre-existing plaintext
//!    record.
//! 4. `rejects an encryption key of the wrong length` — pure
//!    validator: a 32-byte AES-256 key must be 64 hex chars.
//!
//! Cases 1-3 require AES-256-GCM crypto; this crate's parity policy
//! is **no new dependencies**, and the workspace doesn't already
//! pull in an AEAD cipher (the only crypto in scope is HMAC-SHA256
//! for webhook signing, which uses the `hmac` + `sha2` crates
//! already wired up). Those three cases are js-only-documented in
//! [`crate::tests`] header per the slice 411 pattern.
//!
//! Case 4 is a pure-Rust hex-length validator — ported below.

/// Error returned by [`validate_encryption_key_hex`] when the key
/// isn't a 32-byte AES-256-GCM key encoded as 64 hex chars. 1:1
/// with upstream's `throw new Error("encryptionKey must be 32 bytes")`
/// (asserted via the `BYTES_32_PATTERN = /32 bytes/` regex in
/// index.test.ts L14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidEncryptionKey {
    /// Human-readable message — contains the literal "32 bytes" so
    /// the upstream pattern test passes.
    pub message: String,
}

impl std::fmt::Display for InvalidEncryptionKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for InvalidEncryptionKey {}

/// Validate that `key_hex` is a 64-char hex string decoding to a
/// 32-byte AES-256-GCM key. 1:1 port of upstream's constructor-time
/// length check on the `encryptionKey` option.
///
/// Returns the decoded 32-byte key on success. Returns
/// [`InvalidEncryptionKey`] when:
/// - the string isn't exactly 64 chars,
/// - the string contains non-hex characters,
/// - or (defensively) the decoded length isn't 32 bytes.
pub fn validate_encryption_key_hex(key_hex: &str) -> Result<[u8; 32], InvalidEncryptionKey> {
    if key_hex.len() != 64 {
        return Err(InvalidEncryptionKey {
            message: format!(
                "encryptionKey must be 32 bytes (64 hex characters), got {} chars",
                key_hex.len()
            ),
        });
    }
    let mut out = [0u8; 32];
    for (i, chunk) in key_hex.as_bytes().chunks(2).enumerate() {
        let hi = hex_nibble(chunk[0]).ok_or_else(|| InvalidEncryptionKey {
            message: "encryptionKey must be 32 bytes (hex-decoding failed)".to_string(),
        })?;
        let lo = hex_nibble(chunk[1]).ok_or_else(|| InvalidEncryptionKey {
            message: "encryptionKey must be 32 bytes (hex-decoding failed)".to_string(),
        })?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(10 + c - b'a'),
        b'A'..=b'F' => Some(10 + c - b'A'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1:1 with upstream index.test.ts:3713 >
    // "multi-tenant installations > token encryption >
    //  rejects an encryption key of the wrong length"
    #[test]
    fn rejects_an_encryption_key_of_the_wrong_length() {
        let err = validate_encryption_key_hex("tooshort").expect_err("8 chars is rejected");
        // Upstream asserts via `/32 bytes/` regex — verify our
        // message contains the literal substring.
        assert!(
            err.to_string().contains("32 bytes"),
            "expected /32 bytes/ in error, got: {err}"
        );
    }

    // ---------- additive Rust-side coverage ----------

    #[test]
    fn accepts_a_32_byte_key_as_64_hex_chars() {
        let key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let bytes = validate_encryption_key_hex(key).unwrap();
        assert_eq!(bytes.len(), 32);
        assert_eq!(bytes[0], 0x01);
        assert_eq!(bytes[31], 0xef);
    }

    #[test]
    fn accepts_uppercase_hex() {
        let key = "0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF";
        let bytes = validate_encryption_key_hex(key).unwrap();
        assert_eq!(bytes[0], 0x01);
        assert_eq!(bytes[1], 0x23);
    }

    #[test]
    fn rejects_non_hex_characters() {
        // 64 chars but contains 'z'
        let bad =
            "z123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string() + "0";
        // Trim back to 64 chars
        let bad = &bad[..64];
        let err = validate_encryption_key_hex(bad).expect_err("non-hex rejected");
        assert!(err.to_string().contains("32 bytes"));
    }

    #[test]
    fn rejects_too_long_key() {
        let key = "0".repeat(128);
        let err = validate_encryption_key_hex(&key).expect_err("too long rejected");
        assert!(err.to_string().contains("32 bytes"));
    }
}
