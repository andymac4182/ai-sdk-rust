use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};

use crate::error::{Result, RuntimeDecryptionContext, RuntimeDecryptionError, WorkflowCoreError};
use crate::format::{
    DecodedFormat, ENCRYPTED, WireData, decode_format_prefix, encode_with_format_prefix,
    is_encrypted,
};

const NONCE_LENGTH: usize = 12;
const TAG_LENGTH: usize = 16;
const KEY_LENGTH: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyUsage {
    Encrypt,
    Decrypt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CryptoKey {
    raw: [u8; KEY_LENGTH],
    can_encrypt: bool,
    can_decrypt: bool,
}

impl CryptoKey {
    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new_from_slice(&self.raw).expect("AES-256 key length is validated")
    }
}

pub fn import_key(raw: &[u8]) -> Result<CryptoKey> {
    import_key_with_usages(raw, &[KeyUsage::Encrypt, KeyUsage::Decrypt])
}

pub fn import_key_with_usages(raw: &[u8], usages: &[KeyUsage]) -> Result<CryptoKey> {
    if raw.len() != KEY_LENGTH {
        return Err(WorkflowCoreError::InvalidEncryptionKeyLength(raw.len()));
    }

    let mut key = [0; KEY_LENGTH];
    key.copy_from_slice(raw);
    Ok(CryptoKey {
        raw: key,
        can_encrypt: usages.contains(&KeyUsage::Encrypt),
        can_decrypt: usages.contains(&KeyUsage::Decrypt),
    })
}

/// Encrypt bytes as `[nonce (12 bytes)][ciphertext + 16-byte GCM tag]`.
pub fn encrypt(key: &CryptoKey, data: &[u8]) -> Result<Vec<u8>> {
    if !key.can_encrypt {
        return Err(RuntimeDecryptionError::new(
            "AES-256-GCM encryption failed: key is not allowed to encrypt",
            RuntimeDecryptionContext::new("encrypt", data.len()),
        )
        .into());
    }

    let mut nonce = [0; NONCE_LENGTH];
    OsRng.fill_bytes(&mut nonce);
    encrypt_with_nonce(key, data, nonce)
}

pub fn encrypt_with_nonce(
    key: &CryptoKey,
    data: &[u8],
    nonce: [u8; NONCE_LENGTH],
) -> Result<Vec<u8>> {
    if !key.can_encrypt {
        return Err(RuntimeDecryptionError::new(
            "AES-256-GCM encryption failed: key is not allowed to encrypt",
            RuntimeDecryptionContext::new("encrypt", data.len()),
        )
        .into());
    }

    let ciphertext = key
        .cipher()
        .encrypt(Nonce::from_slice(&nonce), data)
        .map_err(|_| {
            RuntimeDecryptionError::new(
                "AES-256-GCM encryption failed",
                RuntimeDecryptionContext::new("encrypt", data.len()),
            )
        })?;
    let mut envelope = Vec::with_capacity(NONCE_LENGTH + ciphertext.len());
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&ciphertext);
    Ok(envelope)
}

pub fn decrypt(key: &CryptoKey, data: &[u8]) -> Result<Vec<u8>> {
    if !key.can_decrypt {
        return Err(RuntimeDecryptionError::new(
            "AES-256-GCM decryption failed: key is not allowed to decrypt",
            RuntimeDecryptionContext::new("decrypt", data.len()),
        )
        .into());
    }

    let min_length = NONCE_LENGTH + TAG_LENGTH;
    if data.len() < min_length {
        return Err(RuntimeDecryptionError::new(
            format!(
                "Encrypted data too short: expected at least {min_length} bytes, got {}",
                data.len()
            ),
            RuntimeDecryptionContext::new("decrypt", data.len()),
        )
        .into());
    }

    let (nonce, ciphertext) = data.split_at(NONCE_LENGTH);
    key.cipher()
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|_| {
            RuntimeDecryptionError::new(
                "AES-256-GCM decryption failed: authentication tag mismatch",
                RuntimeDecryptionContext::new("decrypt", data.len()),
            )
            .into()
        })
}

/// Serialization-layer encryption wrapper. Adds the outer `encr` prefix.
pub fn maybe_encrypt(data: WireData, key: Option<&CryptoKey>) -> Result<WireData> {
    let Some(key) = key else {
        return Ok(data);
    };
    let WireData::Bytes(bytes) = data else {
        return Ok(data);
    };
    let encrypted = encrypt(key, &bytes)?;
    encode_with_format_prefix(ENCRYPTED, WireData::Bytes(encrypted))
}

/// Serialization-layer decryption wrapper. It enriches low-level AES errors
/// with the real outer `encr` envelope marker.
pub fn maybe_decrypt(data: WireData, key: Option<&CryptoKey>) -> Result<WireData> {
    let WireData::Bytes(bytes) = data else {
        return Ok(data);
    };
    let wrapped = WireData::Bytes(bytes);
    if !is_encrypted(&wrapped) {
        return Ok(wrapped);
    }
    let Some(key) = key else {
        return Err(RuntimeDecryptionError::new(
            "Encrypted data encountered but no encryption key is available. Encryption is not configured or no key was provided for this run.",
            RuntimeDecryptionContext::new("decrypt", wrapped.as_bytes().map_or(0, <[u8]>::len))
                .with_format_prefix(ENCRYPTED),
        )
        .into());
    };

    let DecodedFormat { payload, .. } = decode_format_prefix(wrapped)?;
    decrypt(key, &payload)
        .map(WireData::Bytes)
        .map_err(|error| match error {
            WorkflowCoreError::RuntimeDecryption(mut runtime) => {
                runtime.context.format_prefix = Some(ENCRYPTED.to_string());
                WorkflowCoreError::RuntimeDecryption(runtime)
            }
            other => other,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{DEVALUE_V1, peek_format_prefix};

    const RAW_KEY: [u8; KEY_LENGTH] = [7; KEY_LENGTH];
    const OTHER_RAW_KEY: [u8; KEY_LENGTH] = [8; KEY_LENGTH];

    #[test]
    fn encryption_test_encrypt_decrypt_returns_original_plaintext() {
        let key = import_key(&RAW_KEY).unwrap();
        let plaintext = b"hello, workflow";
        let ciphertext = encrypt(&key, plaintext).unwrap();

        assert_eq!(
            ciphertext.len(),
            plaintext.len() + NONCE_LENGTH + TAG_LENGTH
        );
        assert_eq!(decrypt(&key, &ciphertext).unwrap(), plaintext);
    }

    #[test]
    fn encryption_test_import_key_rejects_keys_that_are_not_exactly_32_bytes() {
        assert!(matches!(
            import_key(&[7; 16]),
            Err(WorkflowCoreError::InvalidEncryptionKeyLength(16))
        ));
    }

    #[test]
    fn encryption_test_decrypt_failure_cases_use_runtime_decryption_error() {
        let key = import_key(&RAW_KEY).unwrap();
        let too_short = [0; 10];
        let error = decrypt(&key, &too_short).unwrap_err();
        assert!(matches!(
            error,
            WorkflowCoreError::RuntimeDecryption(RuntimeDecryptionError {
                context: RuntimeDecryptionContext {
                    operation: "decrypt",
                    byte_length: 10,
                    ..
                },
                ..
            })
        ));

        let mut tampered = encrypt(&key, b"hello, workflow").unwrap();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xff;
        let error = decrypt(&key, &tampered).unwrap_err();
        assert!(matches!(
            error,
            WorkflowCoreError::RuntimeDecryption(RuntimeDecryptionError {
                context: RuntimeDecryptionContext {
                    operation: "decrypt",
                    format_prefix: None,
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn encryption_test_decrypt_failure_cases_wrong_key_is_runtime_decryption_error() {
        let writer_key = import_key(&RAW_KEY).unwrap();
        let reader_key = import_key(&OTHER_RAW_KEY).unwrap();
        let ciphertext = encrypt(&writer_key, b"secret").unwrap();
        assert!(matches!(
            decrypt(&reader_key, &ciphertext),
            Err(WorkflowCoreError::RuntimeDecryption(_))
        ));
    }

    #[test]
    fn encryption_test_encrypt_failure_cases_wrap_underlying_crypto_call() {
        let decrypt_only = import_key_with_usages(&RAW_KEY, &[KeyUsage::Decrypt]).unwrap();
        let error = encrypt(&decrypt_only, b"nope").unwrap_err();
        assert!(matches!(
            error,
            WorkflowCoreError::RuntimeDecryption(RuntimeDecryptionError {
                context: RuntimeDecryptionContext {
                    operation: "encrypt",
                    byte_length: 4,
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn serialization_serialization_test_encrypt_decrypt_layer_matches_encr_contract() {
        let key = import_key(&[0x42; KEY_LENGTH]).unwrap();
        let data =
            crate::format::encode_with_format_prefix(DEVALUE_V1, WireData::Bytes(vec![1, 2, 3]))
                .unwrap();

        assert_eq!(maybe_encrypt(data.clone(), None).unwrap(), data);
        assert_eq!(
            maybe_encrypt(
                WireData::Legacy(serde_json::json!("string data")),
                Some(&key)
            )
            .unwrap(),
            WireData::Legacy(serde_json::json!("string data"))
        );

        let encrypted = maybe_encrypt(data.clone(), Some(&key)).unwrap();
        assert_eq!(peek_format_prefix(&encrypted).as_deref(), Some(ENCRYPTED));
        assert_eq!(maybe_decrypt(encrypted, Some(&key)).unwrap(), data);
    }

    #[test]
    fn serialization_serialization_test_decrypt_layer_requires_key_and_attaches_encr_prefix() {
        let key = import_key(&[0x42; KEY_LENGTH]).unwrap();
        let data =
            crate::format::encode_with_format_prefix(DEVALUE_V1, WireData::Bytes(vec![1, 2, 3]))
                .unwrap();
        let encrypted = maybe_encrypt(data, Some(&key)).unwrap();

        let error = maybe_decrypt(encrypted.clone(), None).unwrap_err();
        assert!(matches!(
            error,
            WorkflowCoreError::RuntimeDecryption(RuntimeDecryptionError {
                context: RuntimeDecryptionContext {
                    operation: "decrypt",
                    format_prefix: Some(prefix),
                    ..
                },
                ..
            }) if prefix == ENCRYPTED
        ));

        let mut tampered = encrypted.into_bytes().unwrap();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xff;
        let error = maybe_decrypt(WireData::Bytes(tampered), Some(&key)).unwrap_err();
        assert!(matches!(
            error,
            WorkflowCoreError::RuntimeDecryption(RuntimeDecryptionError {
                context: RuntimeDecryptionContext {
                    operation: "decrypt",
                    format_prefix: Some(prefix),
                    ..
                },
                ..
            }) if prefix == ENCRYPTED
        ));
    }
}
