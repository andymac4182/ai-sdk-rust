//! Token encryption helpers re-exported from
//! `chat_sdk_adapter_shared::crypto`. 1:1 port of upstream
//! `packages/adapter-slack/src/crypto.ts`, which exists only to
//! re-export the shared primitives so per-adapter callsites
//! historically importing from `./crypto` keep working.
//!
//! The tests in this module port the 14 upstream cases from
//! `packages/adapter-slack/src/crypto.test.ts`. All round-trip /
//! tamper-detection / key-decode behavior is exercised through
//! the re-exported shared helpers.

pub use chat_sdk_adapter_shared::crypto::{
    CryptoError, EncryptedTokenData, decode_key, decrypt_token, encrypt_token,
    is_encrypted_token_data,
};

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use rand::RngCore;

    fn test_key() -> Vec<u8> {
        let mut key = vec![0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        key
    }

    // ---------- encryptToken / decryptToken (4 cases) ----------

    #[test]
    fn encrypt_decrypt_round_trip() {
        let key = test_key();
        let token = "xoxb-test-bot-token-12345";
        let encrypted = encrypt_token(token, &key).unwrap();
        let decrypted = decrypt_token(&encrypted, &key).unwrap();
        assert_eq!(decrypted, token);
    }

    #[test]
    fn encrypt_produces_different_ciphertexts_for_same_input() {
        let key = test_key();
        let token = "xoxb-same-token";
        let a = encrypt_token(token, &key).unwrap();
        let b = encrypt_token(token, &key).unwrap();
        assert_ne!(a.data, b.data);
        assert_ne!(a.iv, b.iv);
    }

    #[test]
    fn decrypt_with_wrong_key_errors() {
        let key = test_key();
        let token = "xoxb-secret";
        let encrypted = encrypt_token(token, &key).unwrap();
        let wrong_key = test_key();
        assert!(decrypt_token(&encrypted, &wrong_key).is_err());
    }

    #[test]
    fn decrypt_with_tampered_ciphertext_errors() {
        let key = test_key();
        let token = "xoxb-secret";
        let mut encrypted = encrypt_token(token, &key).unwrap();
        encrypted.data = STANDARD.encode(b"tampered");
        assert!(decrypt_token(&encrypted, &key).is_err());
    }

    // ---------- decodeKey (5 cases) ----------

    #[test]
    fn decode_key_decodes_a_valid_32_byte_base64_key() {
        let key = test_key();
        let key_b64 = STANDARD.encode(&key);
        let decoded = decode_key(&key_b64).unwrap();
        assert_eq!(decoded.len(), 32);
        assert_eq!(decoded, key);
    }

    #[test]
    fn decode_key_decodes_a_valid_64_char_hex_key() {
        let key = test_key();
        let key_hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        let decoded = decode_key(&key_hex).unwrap();
        assert_eq!(decoded.len(), 32);
        assert_eq!(decoded, key);
    }

    #[test]
    fn decode_key_trims_whitespace() {
        let key = test_key();
        let key_b64 = STANDARD.encode(&key);
        let padded = format!("  {key_b64}  ");
        let decoded = decode_key(&padded).unwrap();
        assert_eq!(decoded.len(), 32);
    }

    #[test]
    fn decode_key_errors_for_non_32_byte_key() {
        // 16 random bytes -> 24-char base64 -> can't be 32 bytes
        let short = STANDARD.encode([0u8; 16]);
        match decode_key(&short) {
            Err(err) => {
                let msg = err.to_string();
                assert!(
                    msg.contains("32 bytes"),
                    "expected 'must decode to exactly 32 bytes' message, got: {msg}"
                );
            }
            Ok(_) => panic!("expected error for short key"),
        }
    }

    #[test]
    fn decode_key_errors_for_empty_string() {
        assert!(decode_key("").is_err());
    }

    // ---------- isEncryptedTokenData (5 cases) ----------

    #[test]
    fn is_encrypted_token_data_returns_true_for_valid_payload() {
        let key = test_key();
        let encrypted = encrypt_token("test", &key).unwrap();
        let value = serde_json::to_value(&encrypted).unwrap();
        assert!(is_encrypted_token_data(&value));
    }

    #[test]
    fn is_encrypted_token_data_returns_false_for_plain_string() {
        assert!(!is_encrypted_token_data(&serde_json::json!("xoxb-token")));
    }

    #[test]
    fn is_encrypted_token_data_returns_false_for_null_and_missing() {
        assert!(!is_encrypted_token_data(&serde_json::Value::Null));
        // No undefined in serde_json; null is the closest analogue.
    }

    #[test]
    fn is_encrypted_token_data_returns_false_for_object_missing_fields() {
        assert!(!is_encrypted_token_data(
            &serde_json::json!({ "iv": "a", "data": "b" })
        ));
        assert!(!is_encrypted_token_data(
            &serde_json::json!({ "iv": "a", "tag": "c" })
        ));
    }

    #[test]
    fn is_encrypted_token_data_returns_false_for_non_string_fields() {
        assert!(!is_encrypted_token_data(
            &serde_json::json!({ "iv": 1, "data": 2, "tag": 3 })
        ));
    }
}
