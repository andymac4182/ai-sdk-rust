use serde_json::Value;

use crate::error::{Result, WorkflowCoreError};

/// Length of every workflow serialization format prefix.
pub const FORMAT_PREFIX_LENGTH: usize = 4;

/// Devalue-compatible wire format prefix used upstream.
pub const DEVALUE_V1: &str = "devl";

/// Encrypted-envelope prefix used by the serialization layer.
pub const ENCRYPTED: &str = "encr";

#[derive(Clone, Debug, PartialEq)]
pub enum WireData {
    Bytes(Vec<u8>),
    Legacy(Value),
}

impl WireData {
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(bytes) => Some(bytes),
            Self::Legacy(_) => None,
        }
    }

    pub fn into_bytes(self) -> Option<Vec<u8>> {
        match self {
            Self::Bytes(bytes) => Some(bytes),
            Self::Legacy(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedFormat {
    pub format: String,
    pub payload: Vec<u8>,
}

/// Runtime equivalent of upstream `isFormatPrefix()`.
pub fn is_format_prefix(value: &str) -> bool {
    value.len() == FORMAT_PREFIX_LENGTH
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

/// Prepend a format prefix to byte payloads, passing legacy/plain values through.
pub fn encode_with_format_prefix(format: &str, payload: WireData) -> Result<WireData> {
    if !is_format_prefix(format) {
        return Err(WorkflowCoreError::InvalidFormatPrefix {
            prefix: format.to_string(),
        });
    }

    let WireData::Bytes(payload) = payload else {
        return Ok(payload);
    };

    let mut encoded = Vec::with_capacity(FORMAT_PREFIX_LENGTH + payload.len());
    encoded.extend_from_slice(format.as_bytes());
    encoded.extend_from_slice(&payload);
    Ok(WireData::Bytes(encoded))
}

/// Read a valid 4-byte prefix without consuming it.
pub fn peek_format_prefix(data: &WireData) -> Option<String> {
    let bytes = data.as_bytes()?;
    if bytes.len() < FORMAT_PREFIX_LENGTH {
        return None;
    }
    let prefix = std::str::from_utf8(&bytes[..FORMAT_PREFIX_LENGTH]).ok()?;
    is_format_prefix(prefix).then(|| prefix.to_string())
}

pub fn is_encrypted(data: &WireData) -> bool {
    peek_format_prefix(data).as_deref() == Some(ENCRYPTED)
}

/// Decode a format-prefixed payload. Legacy JSON values are treated as devl.
pub fn decode_format_prefix(data: WireData) -> Result<DecodedFormat> {
    match data {
        WireData::Legacy(value) => Ok(DecodedFormat {
            format: DEVALUE_V1.to_string(),
            payload: serde_json::to_vec(&value)
                .map_err(|error| WorkflowCoreError::Serialization(error.to_string()))?,
        }),
        WireData::Bytes(bytes) => {
            if bytes.len() < FORMAT_PREFIX_LENGTH {
                return Err(WorkflowCoreError::FormatDataTooShort {
                    expected: FORMAT_PREFIX_LENGTH,
                    actual: bytes.len(),
                });
            }
            let prefix = std::str::from_utf8(&bytes[..FORMAT_PREFIX_LENGTH])
                .unwrap_or_default()
                .to_string();
            if !is_format_prefix(&prefix) {
                return Err(WorkflowCoreError::InvalidFormatPrefix { prefix });
            }
            Ok(DecodedFormat {
                format: prefix,
                payload: bytes[FORMAT_PREFIX_LENGTH..].to_vec(),
            })
        }
    }
}

/// Legacy browser-safe format helper: only known formats are accepted.
pub fn decode_known_format_prefix(data: WireData) -> Result<DecodedFormat> {
    let decoded = decode_format_prefix(data)?;
    if decoded.format == DEVALUE_V1 || decoded.format == ENCRYPTED {
        Ok(decoded)
    } else {
        Err(WorkflowCoreError::UnsupportedSerializationFormat(
            decoded.format,
        ))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn serialization_serialization_test_is_format_prefix_accepts_valid_prefixes() {
        for value in [
            "devl", "cbor", "json", "encr", "abcd", "v2b1", "0000", "9999", "ab12", "a0z9",
        ] {
            assert!(is_format_prefix(value), "{value}");
        }
    }

    #[test]
    fn serialization_serialization_test_is_format_prefix_rejects_invalid_prefixes() {
        for value in [
            "", "a", "ab", "abc", "abcde", "abcdef", "DEVL", "Devl", "devL", "de-l", "de_l",
            "de.l", "de l",
        ] {
            assert!(!is_format_prefix(value), "{value}");
        }
    }

    #[test]
    fn serialization_serialization_test_serialization_format_constants_are_valid() {
        assert_eq!(DEVALUE_V1, "devl");
        assert_eq!(ENCRYPTED, "encr");
        assert!(is_format_prefix(DEVALUE_V1));
        assert!(is_format_prefix(ENCRYPTED));
    }

    #[test]
    fn serialization_serialization_test_encode_with_format_prefix_prepends_prefix() {
        let encoded =
            encode_with_format_prefix(DEVALUE_V1, WireData::Bytes(vec![1, 2, 3])).unwrap();
        assert_eq!(encoded, WireData::Bytes(b"devl\x01\x02\x03".to_vec()));
    }

    #[test]
    fn serialization_serialization_test_encode_with_format_prefix_passes_legacy_values() {
        let value = WireData::Legacy(json!({ "plain": true }));
        assert_eq!(
            encode_with_format_prefix(DEVALUE_V1, value.clone()).unwrap(),
            value
        );
    }

    #[test]
    fn serialization_serialization_test_encode_with_format_prefix_handles_empty_and_large_payloads()
    {
        assert_eq!(
            encode_with_format_prefix(DEVALUE_V1, WireData::Bytes(Vec::new())).unwrap(),
            WireData::Bytes(b"devl".to_vec())
        );
        let payload = vec![0xff; 100_000];
        let encoded = encode_with_format_prefix(DEVALUE_V1, WireData::Bytes(payload)).unwrap();
        assert_eq!(encoded.as_bytes().unwrap().len(), 100_004);
    }

    #[test]
    fn serialization_serialization_test_decode_format_prefix_decodes_bytes_and_legacy_values() {
        let decoded = decode_format_prefix(WireData::Bytes(b"devl\x01\x02\x03".to_vec())).unwrap();
        assert_eq!(decoded.format, DEVALUE_V1);
        assert_eq!(decoded.payload, vec![1, 2, 3]);

        let legacy =
            decode_format_prefix(WireData::Legacy(json!([1, "hello", { "a": 2 }]))).unwrap();
        assert_eq!(legacy.format, DEVALUE_V1);
        assert_eq!(
            String::from_utf8(legacy.payload).unwrap(),
            r#"[1,"hello",{"a":2}]"#
        );
    }

    #[test]
    fn serialization_serialization_test_decode_format_prefix_rejects_short_or_invalid_bytes() {
        assert!(matches!(
            decode_format_prefix(WireData::Bytes(vec![1, 2, 3])),
            Err(WorkflowCoreError::FormatDataTooShort {
                expected: FORMAT_PREFIX_LENGTH,
                actual: 3
            })
        ));
        assert!(matches!(
            decode_format_prefix(WireData::Bytes(vec![0, 0, 0, 0])),
            Err(WorkflowCoreError::InvalidFormatPrefix { .. })
        ));
    }

    #[test]
    fn serialization_serialization_test_peek_format_prefix_and_is_encrypted_match_upstream() {
        let devl = encode_with_format_prefix(DEVALUE_V1, WireData::Bytes(vec![1])).unwrap();
        let encr = encode_with_format_prefix(ENCRYPTED, WireData::Bytes(vec![1])).unwrap();
        let cbor = encode_with_format_prefix("cbor", WireData::Bytes(vec![1])).unwrap();

        assert_eq!(peek_format_prefix(&devl).as_deref(), Some(DEVALUE_V1));
        assert_eq!(peek_format_prefix(&cbor).as_deref(), Some("cbor"));
        assert!(is_encrypted(&encr));
        assert!(!is_encrypted(&devl));
        assert!(!is_encrypted(&WireData::Legacy(json!("hello"))));
        assert_eq!(peek_format_prefix(&WireData::Bytes(vec![1, 2, 3])), None);
        assert_eq!(peek_format_prefix(&WireData::Bytes(vec![0, 0, 0, 0])), None);
    }

    #[test]
    fn serialization_format_test_decode_known_format_prefix_rejects_unknown_format() {
        let cbor = encode_with_format_prefix("cbor", WireData::Bytes(vec![1])).unwrap();
        assert!(matches!(
            decode_known_format_prefix(cbor),
            Err(WorkflowCoreError::UnsupportedSerializationFormat(format)) if format == "cbor"
        ));
    }
}
