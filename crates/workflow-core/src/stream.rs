use crate::codec::{SerializationMode, deserialize, serialize};
use crate::encryption::CryptoKey;
use crate::error::{Result, RuntimeDecryptionContext, RuntimeDecryptionError, WorkflowCoreError};
use crate::format::{ENCRYPTED, WireData, peek_format_prefix};
use crate::value::WorkflowValue;

const LENGTH_HEADER_BYTES: usize = 4;

pub fn serialize_frames(values: &[WorkflowValue], key: Option<&CryptoKey>) -> Result<Vec<Vec<u8>>> {
    values
        .iter()
        .map(|value| {
            let payload = serialize(value, SerializationMode::Workflow, key)?
                .into_bytes()
                .ok_or_else(|| {
                    WorkflowCoreError::Serialization("stream payload must be bytes".into())
                })?;
            let mut frame = Vec::with_capacity(LENGTH_HEADER_BYTES + payload.len());
            frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            frame.extend_from_slice(&payload);
            Ok(frame)
        })
        .collect()
}

pub fn deserialize_frames(
    chunks: &[Vec<u8>],
    key: Option<&CryptoKey>,
) -> Result<Vec<WorkflowValue>> {
    let mut buffer = Vec::new();
    for chunk in chunks {
        buffer.extend_from_slice(chunk);
    }

    if !looks_like_framed_stream(&buffer) {
        return deserialize_legacy_newline_chunks(&buffer);
    }

    let mut cursor = 0;
    let mut values = Vec::new();
    while cursor < buffer.len() {
        if buffer.len() - cursor < LENGTH_HEADER_BYTES {
            return Err(WorkflowCoreError::Deserialization(
                "partial stream frame length header".into(),
            ));
        }
        let frame_len = u32::from_be_bytes(
            buffer[cursor..cursor + LENGTH_HEADER_BYTES]
                .try_into()
                .expect("length slice has exactly four bytes"),
        ) as usize;
        cursor += LENGTH_HEADER_BYTES;
        if buffer.len() - cursor < frame_len {
            return Err(WorkflowCoreError::Deserialization(
                "partial stream frame payload".into(),
            ));
        }
        let payload = buffer[cursor..cursor + frame_len].to_vec();
        cursor += frame_len;

        if peek_format_prefix(&WireData::Bytes(payload.clone())).as_deref() == Some(ENCRYPTED)
            && key.is_none()
        {
            return Err(RuntimeDecryptionError::new(
                "Encrypted stream data encountered but no encryption key is available",
                RuntimeDecryptionContext::new("decrypt", payload.len())
                    .with_format_prefix(ENCRYPTED),
            )
            .into());
        }

        values.push(deserialize(
            WireData::Bytes(payload),
            SerializationMode::Workflow,
            key,
        )?);
    }
    Ok(values)
}

fn looks_like_framed_stream(buffer: &[u8]) -> bool {
    if buffer.len() < LENGTH_HEADER_BYTES {
        return false;
    }
    let frame_len = u32::from_be_bytes(
        buffer[..LENGTH_HEADER_BYTES]
            .try_into()
            .expect("length slice has exactly four bytes"),
    ) as usize;
    frame_len + LENGTH_HEADER_BYTES <= buffer.len()
}

pub fn deserialize_legacy_newline_chunks(buffer: &[u8]) -> Result<Vec<WorkflowValue>> {
    let text = std::str::from_utf8(buffer)
        .map_err(|error| WorkflowCoreError::Deserialization(error.to_string()))?;
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value = serde_json::from_str(line)
                .map_err(|error| WorkflowCoreError::Deserialization(error.to_string()))?;
            Ok(WorkflowValue::Json(value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::encryption::import_key;
    use crate::format::{DEVALUE_V1, ENCRYPTED};

    fn json_value(value: serde_json::Value) -> WorkflowValue {
        WorkflowValue::Json(value)
    }

    #[test]
    fn serialization_test_get_serialize_stream_frames_each_chunk_with_format_prefix_and_length() {
        let chunks = serialize_frames(
            &[
                json_value(json!({"hello":"world"})),
                WorkflowValue::Number(42.0),
            ],
            None,
        )
        .unwrap();
        assert_eq!(chunks.len(), 2);

        for chunk in chunks {
            assert!(chunk.len() > 8);
            let frame_length = u32::from_be_bytes(chunk[..4].try_into().unwrap()) as usize;
            assert_eq!(frame_length, chunk.len() - 4);
            assert_eq!(std::str::from_utf8(&chunk[4..8]).unwrap(), DEVALUE_V1);
        }
    }

    #[test]
    fn serialization_test_get_serialize_stream_frames_parse_and_coalesce() {
        let original = vec![
            json_value(json!({"message":"hello","count":42})),
            json_value(json!([1, 2, 3])),
            WorkflowValue::String("plain string".into()),
            WorkflowValue::Null,
        ];
        let frames = serialize_frames(&original, None).unwrap();
        assert_eq!(deserialize_frames(&frames, None).unwrap(), original);

        let concatenated = vec![frames.concat()];
        assert_eq!(deserialize_frames(&concatenated, None).unwrap(), original);
    }

    #[test]
    fn serialization_test_get_deserialize_stream_handles_arbitrary_frame_splits() {
        let original = vec![json_value(json!({"key":"value"}))];
        let frame = serialize_frames(&original, None).unwrap().remove(0);
        let split = frame.len() / 2;
        let chunks = vec![frame[..split].to_vec(), frame[split..].to_vec()];
        assert_eq!(deserialize_frames(&chunks, None).unwrap(), original);
    }

    #[test]
    fn serialization_test_get_deserialize_stream_legacy_fallback_parses_newline_json() {
        let chunks = vec![br#"{"hello":"world"}"#.to_vec(), b"\n42\n".to_vec()];
        assert_eq!(
            deserialize_frames(&chunks, None).unwrap(),
            vec![json_value(json!({"hello":"world"})), json_value(json!(42))]
        );

        let chunks = vec![
            br#""hello""#.to_vec(),
            b"\n".to_vec(),
            b"\"world\"\n".to_vec(),
        ];
        assert_eq!(
            deserialize_frames(&chunks, None).unwrap(),
            vec![json_value(json!("hello")), json_value(json!("world"))]
        );
    }

    #[test]
    fn serialization_test_stream_encryption_round_trip_frames_with_encr_prefix() {
        let key = import_key(&(0_u8..32).collect::<Vec<_>>()).unwrap();
        let original = vec![
            json_value(json!({"message":"secret","count":42})),
            json_value(json!([1, 2, 3])),
            WorkflowValue::String("plain string".into()),
            WorkflowValue::Null,
            WorkflowValue::Bool(true),
        ];
        let encrypted = serialize_frames(&original, Some(&key)).unwrap();

        let first = &encrypted[0];
        let frame_length = u32::from_be_bytes(first[..4].try_into().unwrap()) as usize;
        assert_eq!(frame_length, first.len() - 4);
        assert_eq!(std::str::from_utf8(&first[4..8]).unwrap(), ENCRYPTED);
        assert_eq!(
            deserialize_frames(&encrypted, Some(&key)).unwrap(),
            original
        );
    }

    #[test]
    fn serialization_test_stream_encryption_handles_concatenated_split_and_large_payloads() {
        let key = import_key(&(0_u8..32).collect::<Vec<_>>()).unwrap();
        let original = vec![
            json_value(json!({"a":1})),
            json_value(json!({"b":2})),
            json_value(json!({"c":3})),
        ];
        let encrypted = serialize_frames(&original, Some(&key)).unwrap();
        assert_eq!(
            deserialize_frames(&[encrypted.concat()], Some(&key)).unwrap(),
            original
        );

        let frame = serialize_frames(&[json_value(json!({"data":"split me"}))], Some(&key))
            .unwrap()
            .remove(0);
        let split = frame.len() / 2;
        assert_eq!(
            deserialize_frames(
                &[frame[..split].to_vec(), frame[split..].to_vec()],
                Some(&key)
            )
            .unwrap(),
            vec![json_value(json!({"data":"split me"}))]
        );

        let large = WorkflowValue::Json(json!((0..1000)
            .map(|index| json!({"index":index,"value":format!("item-{index}"),"nested":{"a":index * 2,"b":index * 3}}))
            .collect::<Vec<_>>()));
        let encrypted = serialize_frames(std::slice::from_ref(&large), Some(&key)).unwrap();
        assert_eq!(
            deserialize_frames(&encrypted, Some(&key)).unwrap(),
            vec![large]
        );
    }

    #[test]
    fn serialization_test_stream_encryption_errors_without_key_or_when_tampered() {
        let key = import_key(&(0_u8..32).collect::<Vec<_>>()).unwrap();
        let encrypted =
            serialize_frames(&[json_value(json!({"secret":true}))], Some(&key)).unwrap();
        assert!(matches!(
            deserialize_frames(&encrypted, None),
            Err(WorkflowCoreError::RuntimeDecryption(RuntimeDecryptionError {
                context: RuntimeDecryptionContext {
                    format_prefix: Some(prefix),
                    ..
                },
                ..
            })) if prefix == ENCRYPTED
        ));

        let mut tampered = encrypted[0].clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xff;
        assert!(matches!(
            deserialize_frames(&[tampered], Some(&key)),
            Err(WorkflowCoreError::RuntimeDecryption(RuntimeDecryptionError {
                context: RuntimeDecryptionContext {
                    format_prefix: Some(prefix),
                    ..
                },
                ..
            })) if prefix == ENCRYPTED
        ));
    }

    #[test]
    fn serialization_test_stream_encryption_does_not_encrypt_without_key() {
        let original = vec![json_value(json!({"hello":"world"}))];
        let frames = serialize_frames(&original, None).unwrap();
        assert_eq!(std::str::from_utf8(&frames[0][4..8]).unwrap(), DEVALUE_V1);
        assert_eq!(deserialize_frames(&frames, None).unwrap(), original);
    }
}
