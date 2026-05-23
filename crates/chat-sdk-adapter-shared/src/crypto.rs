//! Token encryption helpers shared across adapters that persist
//! OAuth/bot tokens to a state store.
//!
//! 1:1 port of `packages/adapter-shared/src/crypto.ts`. Uses
//! AES-256-GCM with a randomly generated 12-byte IV per encryption.
//! Adapters call [`decode_key`] once at construction to turn a
//! user-supplied hex/base64 key string into a 32-byte key, then pass
//! it to [`encrypt_token`] / [`decrypt_token`].
//!
//! Upstream has no `crypto.test.ts` file; the colocated tests below
//! are additive Rust-side roundtrip/decode coverage to lock in
//! behavior.

use aes_gcm::{
    Aes256Gcm, Key, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::errors::AdapterError;

const IV_LENGTH: usize = 12;
const KEY_LENGTH: usize = 32;

/// Encrypted token payload. 1:1 port of upstream
/// `interface EncryptedTokenData`.
///
/// Fields are base64-encoded strings, matching upstream's
/// `Buffer.toString("base64")` choice.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EncryptedTokenData {
    /// Base64-encoded ciphertext (no auth tag).
    pub data: String,
    /// Base64-encoded 12-byte initialization vector.
    pub iv: String,
    /// Base64-encoded 16-byte auth tag.
    pub tag: String,
}

/// Errors returned by the crypto helpers. Mapped to
/// [`AdapterError::base`] / [`AdapterError::validation`] for callers
/// that want to surface them through the adapter error channel.
#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    /// Key was not exactly 32 bytes after decode.
    #[error(
        "Encryption key must decode to exactly 32 bytes (received {0}). Use a 64-char hex string or 44-char base64 string."
    )]
    InvalidKeyLength(usize),
    /// Key string could not be decoded as hex or base64.
    #[error("Encryption key could not be decoded as hex or base64: {0}")]
    KeyDecode(String),
    /// AES-GCM encryption or decryption failure.
    #[error("AES-256-GCM operation failed: {0}")]
    Aead(String),
    /// One of the encoded fields in [`EncryptedTokenData`] was not
    /// valid base64.
    #[error("Invalid base64 in encrypted token field `{field}`: {source}")]
    Base64 {
        /// Which field failed to decode (`iv`, `data`, or `tag`).
        field: &'static str,
        /// Underlying base64 error.
        source: base64::DecodeError,
    },
    /// Decrypted bytes were not valid UTF-8.
    #[error("Decrypted bytes were not valid UTF-8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

impl From<CryptoError> for AdapterError {
    fn from(value: CryptoError) -> Self {
        AdapterError::new(value.to_string(), "crypto")
    }
}

/// Encrypt a plaintext token. 1:1 port of upstream
/// `encryptToken(plaintext, key): EncryptedTokenData`.
///
/// Generates a fresh random 12-byte IV per call. `key` must be 32 bytes
/// (typically the output of [`decode_key`]).
pub fn encrypt_token(plaintext: &str, key: &[u8]) -> Result<EncryptedTokenData, CryptoError> {
    if key.len() != KEY_LENGTH {
        return Err(CryptoError::InvalidKeyLength(key.len()));
    }
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));

    let mut iv = [0u8; IV_LENGTH];
    rand::thread_rng().fill_bytes(&mut iv);
    let nonce = Nonce::from_slice(&iv);

    let mut ciphertext_and_tag = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext.as_bytes(),
                aad: &[],
            },
        )
        .map_err(|e| CryptoError::Aead(e.to_string()))?;

    // aes-gcm appends the 16-byte tag to the ciphertext; upstream
    // serializes them separately. Split here to match the upstream
    // wire shape exactly.
    let tag_start = ciphertext_and_tag
        .len()
        .checked_sub(16)
        .ok_or_else(|| CryptoError::Aead("aes-gcm output too short".to_string()))?;
    let tag = ciphertext_and_tag.split_off(tag_start);
    let ciphertext = ciphertext_and_tag;

    let b64 = base64::engine::general_purpose::STANDARD;
    Ok(EncryptedTokenData {
        iv: b64.encode(iv),
        data: b64.encode(&ciphertext),
        tag: b64.encode(&tag),
    })
}

/// Decrypt a token previously produced by [`encrypt_token`]. 1:1 port
/// of upstream `decryptToken(encrypted, key): string`.
pub fn decrypt_token(encrypted: &EncryptedTokenData, key: &[u8]) -> Result<String, CryptoError> {
    if key.len() != KEY_LENGTH {
        return Err(CryptoError::InvalidKeyLength(key.len()));
    }
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
    let b64 = base64::engine::general_purpose::STANDARD;
    let iv = b64.decode(&encrypted.iv).map_err(|e| CryptoError::Base64 {
        field: "iv",
        source: e,
    })?;
    let ciphertext = b64
        .decode(&encrypted.data)
        .map_err(|e| CryptoError::Base64 {
            field: "data",
            source: e,
        })?;
    let tag = b64
        .decode(&encrypted.tag)
        .map_err(|e| CryptoError::Base64 {
            field: "tag",
            source: e,
        })?;
    let nonce = Nonce::from_slice(&iv);
    let mut combined = ciphertext;
    combined.extend_from_slice(&tag);
    let plaintext = cipher
        .decrypt(
            nonce,
            Payload {
                msg: &combined,
                aad: &[],
            },
        )
        .map_err(|e| CryptoError::Aead(e.to_string()))?;
    Ok(String::from_utf8(plaintext)?)
}

/// Duck-typed shape check. 1:1 port of upstream
/// `isEncryptedTokenData(value): value is EncryptedTokenData`. The
/// typed Rust port already gives compile-time guarantees for owned
/// values; this helper exists for the JSON dispatch path where adapters
/// receive `serde_json::Value` from a state-store row.
pub fn is_encrypted_token_data(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(obj) => {
            obj.get("iv").is_some_and(serde_json::Value::is_string)
                && obj.get("data").is_some_and(serde_json::Value::is_string)
                && obj.get("tag").is_some_and(serde_json::Value::is_string)
        }
        _ => false,
    }
}

/// Decode a user-supplied key string into a 32-byte key. 1:1 port of
/// upstream `decodeKey(rawKey): Buffer`.
///
/// Accepts either:
/// - A 64-character hex string (32 bytes encoded).
/// - A base64 string that decodes to exactly 32 bytes (typically 44
///   chars including `=` padding).
///
/// Returns [`CryptoError::InvalidKeyLength`] if the decoded byte length
/// is not exactly 32.
pub fn decode_key(raw_key: &str) -> Result<Vec<u8>, CryptoError> {
    let trimmed = raw_key.trim();
    let is_hex = trimmed.len() == 64 && trimmed.bytes().all(|b| b.is_ascii_hexdigit());
    let bytes = if is_hex {
        hex::decode(trimmed).map_err(|e| CryptoError::KeyDecode(e.to_string()))?
    } else {
        base64::engine::general_purpose::STANDARD
            .decode(trimmed)
            .map_err(|e| CryptoError::KeyDecode(e.to_string()))?
    };
    if bytes.len() != KEY_LENGTH {
        return Err(CryptoError::InvalidKeyLength(bytes.len()));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    //! Additive coverage for `packages/adapter-shared/src/crypto.ts`.
    //! Upstream ships no `crypto.test.ts`; these tests lock in the
    //! roundtrip + key-decode + shape-check behavior that adapters
    //! depend on.

    use super::*;
    use serde_json::json;

    fn fixed_key() -> Vec<u8> {
        // 32-byte deterministic key for roundtrip tests.
        (0..32u8).collect()
    }

    #[test]
    fn encrypt_decrypt_round_trips_a_plaintext_token() {
        let key = fixed_key();
        let cipher = encrypt_token("hello world", &key).unwrap();
        let plain = decrypt_token(&cipher, &key).unwrap();
        assert_eq!(plain, "hello world");
    }

    #[test]
    fn encrypt_produces_a_new_iv_per_call() {
        let key = fixed_key();
        let a = encrypt_token("same plaintext", &key).unwrap();
        let b = encrypt_token("same plaintext", &key).unwrap();
        assert_ne!(a.iv, b.iv, "IV must be randomized per encryption");
        assert_ne!(a.data, b.data, "ciphertext must differ under fresh IV");
    }

    #[test]
    fn encrypt_rejects_a_wrong_length_key() {
        let err = encrypt_token("text", &[0u8; 16]).unwrap_err();
        assert!(matches!(err, CryptoError::InvalidKeyLength(16)));
    }

    #[test]
    fn decrypt_fails_when_auth_tag_is_tampered_with() {
        let key = fixed_key();
        let mut cipher = encrypt_token("hello", &key).unwrap();
        // Flip the first base64 character of the tag.
        let mut bytes = cipher.tag.into_bytes();
        bytes[0] = if bytes[0] == b'A' { b'B' } else { b'A' };
        cipher.tag = String::from_utf8(bytes).unwrap();
        let err = decrypt_token(&cipher, &key).unwrap_err();
        assert!(matches!(err, CryptoError::Aead(_)));
    }

    #[test]
    fn decode_key_accepts_a_64_char_hex_string() {
        let hex = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
        let key = decode_key(hex).unwrap();
        assert_eq!(key.len(), 32);
        assert_eq!(key[0], 0x00);
        assert_eq!(key[31], 0xff);
    }

    #[test]
    fn decode_key_accepts_a_base64_string_decoding_to_32_bytes() {
        let bytes: Vec<u8> = (0..32u8).collect();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let key = decode_key(&b64).unwrap();
        assert_eq!(key, bytes);
    }

    #[test]
    fn decode_key_trims_surrounding_whitespace_before_decoding() {
        let hex = "  00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff  ";
        let key = decode_key(hex).unwrap();
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn decode_key_rejects_a_wrong_length_decoded_string() {
        // 8-byte base64 -> wrong length.
        let short_b64 = base64::engine::general_purpose::STANDARD.encode([1u8; 8]);
        let err = decode_key(&short_b64).unwrap_err();
        assert!(matches!(err, CryptoError::InvalidKeyLength(8)));
    }

    #[test]
    fn decode_key_rejects_a_non_hex_non_base64_string() {
        let err = decode_key("not-hex-or-base64!!!").unwrap_err();
        assert!(matches!(err, CryptoError::KeyDecode(_)));
    }

    #[test]
    fn is_encrypted_token_data_accepts_a_well_formed_object() {
        let v = json!({"iv": "abc", "data": "xyz", "tag": "ttt"});
        assert!(is_encrypted_token_data(&v));
    }

    #[test]
    fn is_encrypted_token_data_rejects_objects_missing_any_field() {
        assert!(!is_encrypted_token_data(&json!({"iv": "a", "data": "b"})));
        assert!(!is_encrypted_token_data(&json!({"data": "b", "tag": "c"})));
        assert!(!is_encrypted_token_data(&json!({"iv": "a", "tag": "c"})));
    }

    #[test]
    fn is_encrypted_token_data_rejects_objects_with_non_string_fields() {
        assert!(!is_encrypted_token_data(
            &json!({"iv": 1, "data": "b", "tag": "c"})
        ));
    }

    #[test]
    fn is_encrypted_token_data_rejects_non_object_values() {
        assert!(!is_encrypted_token_data(&json!(null)));
        assert!(!is_encrypted_token_data(&json!("hello")));
        assert!(!is_encrypted_token_data(&json!(42)));
        assert!(!is_encrypted_token_data(&json!([])));
    }

    #[test]
    fn encrypted_token_data_round_trips_through_serde_json() {
        let key = fixed_key();
        let cipher = encrypt_token("payload", &key).unwrap();
        let json = serde_json::to_string(&cipher).unwrap();
        let back: EncryptedTokenData = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cipher);
    }

    #[test]
    fn crypto_error_converts_to_adapter_error_via_from() {
        let err: AdapterError = CryptoError::InvalidKeyLength(16).into();
        assert_eq!(err.adapter(), "crypto");
    }
}
