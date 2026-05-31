use serde_json::Value;

use crate::encryption::{CryptoKey, maybe_decrypt, maybe_encrypt};
use crate::error::{Result, WorkflowCoreError};
use crate::format::{DEVALUE_V1, WireData, decode_format_prefix, encode_with_format_prefix};
use crate::value::WorkflowValue;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SerializationMode {
    Workflow,
    Step,
    Client,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DevalueCodec;

impl DevalueCodec {
    pub const FORMAT_PREFIX: &'static str = DEVALUE_V1;

    pub fn serialize_payload(
        self,
        value: &WorkflowValue,
        mode: SerializationMode,
    ) -> Result<Vec<u8>> {
        if mode != SerializationMode::Workflow && value.contains_step_function() {
            return Err(WorkflowCoreError::StepFunctionOutsideWorkflowContext);
        }
        value.to_json_bytes()
    }

    pub fn deserialize_payload(
        self,
        payload: &[u8],
        mode: SerializationMode,
    ) -> Result<WorkflowValue> {
        let value = WorkflowValue::from_json_bytes(payload)?;
        if mode == SerializationMode::Client && value.contains_step_function() {
            return Err(WorkflowCoreError::StepFunctionInClientContext);
        }
        Ok(value)
    }

    pub fn deserialize_legacy(self, data: Value, mode: SerializationMode) -> Result<WorkflowValue> {
        let value = serde_json::from_value::<WorkflowValue>(data.clone())
            .unwrap_or(WorkflowValue::Json(data));
        if mode == SerializationMode::Client && value.contains_step_function() {
            return Err(WorkflowCoreError::StepFunctionInClientContext);
        }
        Ok(value)
    }
}

pub const DEVALUE_CODEC: DevalueCodec = DevalueCodec;

pub fn serialize(
    value: &WorkflowValue,
    mode: SerializationMode,
    key: Option<&CryptoKey>,
) -> Result<WireData> {
    let payload = DEVALUE_CODEC.serialize_payload(value, mode)?;
    let prefixed = encode_with_format_prefix(DEVALUE_V1, WireData::Bytes(payload))?;
    maybe_encrypt(prefixed, key)
}

pub fn deserialize(
    data: WireData,
    mode: SerializationMode,
    key: Option<&CryptoKey>,
) -> Result<WorkflowValue> {
    let decrypted = maybe_decrypt(data, key)?;
    match decrypted {
        WireData::Legacy(value) => DEVALUE_CODEC.deserialize_legacy(value, mode),
        WireData::Bytes(bytes) => {
            let decoded = decode_format_prefix(WireData::Bytes(bytes))?;
            if decoded.format != DEVALUE_V1 {
                return Err(WorkflowCoreError::UnsupportedSerializationFormat(
                    decoded.format,
                ));
            }
            DEVALUE_CODEC.deserialize_payload(&decoded.payload, mode)
        }
    }
}

pub fn workflow_serialize(value: &WorkflowValue) -> Result<WireData> {
    serialize(value, SerializationMode::Workflow, None)
}

pub fn workflow_deserialize(data: WireData) -> Result<WorkflowValue> {
    deserialize(data, SerializationMode::Workflow, None)
}

pub fn step_serialize(value: &WorkflowValue, key: Option<&CryptoKey>) -> Result<WireData> {
    serialize(value, SerializationMode::Step, key)
}

pub fn step_deserialize(data: WireData, key: Option<&CryptoKey>) -> Result<WorkflowValue> {
    deserialize(data, SerializationMode::Step, key)
}

pub fn client_serialize(value: &WorkflowValue, key: Option<&CryptoKey>) -> Result<WireData> {
    serialize(value, SerializationMode::Client, key)
}

pub fn client_deserialize(data: WireData, key: Option<&CryptoKey>) -> Result<WorkflowValue> {
    deserialize(data, SerializationMode::Client, key)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;

    use super::*;
    use crate::encryption::import_key;
    use crate::format::{ENCRYPTED, WireData, peek_format_prefix};
    use crate::value::{TypedArrayKind, WorkflowErrorValue};

    fn complex_value() -> WorkflowValue {
        WorkflowValue::object([
            (
                "items",
                WorkflowValue::Array(vec![
                    WorkflowValue::Number(1.0),
                    WorkflowValue::String("two".into()),
                ]),
            ),
            (
                "map",
                WorkflowValue::Map(vec![(
                    WorkflowValue::String("a".into()),
                    WorkflowValue::Number(1.0),
                )]),
            ),
            (
                "set",
                WorkflowValue::Set(vec![WorkflowValue::Number(1.0), WorkflowValue::Number(2.0)]),
            ),
            ("date", WorkflowValue::date("2025-06-15T00:00:00.000Z")),
            (
                "bytes",
                WorkflowValue::typed_array(TypedArrayKind::Uint8Array, &[1, 2, 3]),
            ),
        ])
    }

    #[test]
    fn serialization_serialization_test_devalue_codec_round_trips_primitives_in_all_modes() {
        let values = [
            WorkflowValue::Number(42.0),
            WorkflowValue::String("hello".into()),
            WorkflowValue::Bool(true),
            WorkflowValue::Null,
            WorkflowValue::Number(0.0),
            WorkflowValue::Number(-1.0),
            WorkflowValue::String(String::new()),
            WorkflowValue::Bool(false),
        ];

        for mode in [
            SerializationMode::Workflow,
            SerializationMode::Step,
            SerializationMode::Client,
        ] {
            for value in &values {
                let serialized = DEVALUE_CODEC.serialize_payload(value, mode).unwrap();
                let deserialized = DEVALUE_CODEC
                    .deserialize_payload(&serialized, mode)
                    .unwrap();
                assert_eq!(deserialized, *value);
            }
        }
    }

    #[test]
    fn serialization_serialization_test_devalue_codec_round_trips_common_special_values() {
        let values = [
            WorkflowValue::date("2025-01-01T00:00:00.000Z"),
            WorkflowValue::Map(vec![
                (
                    WorkflowValue::String("key1".into()),
                    WorkflowValue::String("val1".into()),
                ),
                (
                    WorkflowValue::String("key2".into()),
                    WorkflowValue::String("val2".into()),
                ),
            ]),
            WorkflowValue::Set(vec![
                WorkflowValue::String("a".into()),
                WorkflowValue::String("b".into()),
                WorkflowValue::String("c".into()),
            ]),
            complex_value(),
        ];

        for mode in [
            SerializationMode::Workflow,
            SerializationMode::Step,
            SerializationMode::Client,
        ] {
            for value in &values {
                let serialized = DEVALUE_CODEC.serialize_payload(value, mode).unwrap();
                assert_eq!(
                    DEVALUE_CODEC
                        .deserialize_payload(&serialized, mode)
                        .unwrap(),
                    *value
                );
            }
        }
    }

    #[test]
    fn serialization_serialization_test_devalue_codec_supports_legacy_and_bytes_output() {
        assert_eq!(DEVALUE_CODEC.format_prefix(), DEVALUE_V1);
        let serialized = DEVALUE_CODEC
            .serialize_payload(&WorkflowValue::Number(42.0), SerializationMode::Workflow)
            .unwrap();
        assert!(!serialized.is_empty());

        let legacy = serde_json::to_value(WorkflowValue::object([(
            "test",
            WorkflowValue::String("legacy".into()),
        )]))
        .unwrap();
        assert_eq!(
            DEVALUE_CODEC
                .deserialize_legacy(legacy, SerializationMode::Workflow)
                .unwrap(),
            WorkflowValue::object([("test", WorkflowValue::String("legacy".into()))])
        );
    }

    #[test]
    fn serialization_serialization_test_devalue_codec_step_function_mode_boundaries() {
        let step_fn = WorkflowValue::step_function("test-step");
        let serialized = DEVALUE_CODEC
            .serialize_payload(&step_fn, SerializationMode::Workflow)
            .unwrap();
        assert!(matches!(
            DEVALUE_CODEC.deserialize_payload(&serialized, SerializationMode::Client),
            Err(WorkflowCoreError::StepFunctionInClientContext)
        ));
        assert!(matches!(
            DEVALUE_CODEC.serialize_payload(&step_fn, SerializationMode::Step),
            Err(WorkflowCoreError::StepFunctionOutsideWorkflowContext)
        ));
    }

    #[test]
    fn serialization_serialization_test_workflow_serialize_deserialize_round_trips_values() {
        for value in [
            WorkflowValue::Number(42.0),
            WorkflowValue::String("hello".into()),
            WorkflowValue::Bool(true),
            WorkflowValue::Null,
            WorkflowValue::Array(vec![WorkflowValue::Number(1.0), WorkflowValue::Number(2.0)]),
            WorkflowValue::object([("a", WorkflowValue::Number(1.0))]),
            WorkflowValue::date("2025-06-15T12:00:00.000Z"),
            WorkflowValue::Error(WorkflowErrorValue::new("TypeError", "test error")),
            complex_value(),
            WorkflowValue::BigInt("9007199254740993".into()),
            WorkflowValue::uint8_array(&[1, 2, 3, 4, 5]),
            WorkflowValue::Url("https://example.com/path?q=1".into()),
            WorkflowValue::RegExp {
                source: "foo.*bar".into(),
                flags: "gi".into(),
            },
            WorkflowValue::Headers(vec![("x-custom".into(), "value".into())]),
        ] {
            let serialized = workflow_serialize(&value).unwrap();
            assert_eq!(peek_format_prefix(&serialized).as_deref(), Some(DEVALUE_V1));
            assert_eq!(workflow_deserialize(serialized).unwrap(), value);
        }
    }

    #[test]
    fn serialization_serialization_test_workflow_deserialize_rejects_unsupported_format_prefix() {
        assert!(matches!(
            workflow_deserialize(WireData::Bytes(b"cbor{}".to_vec())),
            Err(WorkflowCoreError::UnsupportedSerializationFormat(format)) if format == "cbor"
        ));
    }

    #[test]
    fn serialization_serialization_test_workflow_deserialize_legacy_non_binary_data() {
        let legacy = serde_json::to_value(WorkflowValue::object([(
            "hello",
            WorkflowValue::String("world".into()),
        )]))
        .unwrap();
        assert_eq!(
            workflow_deserialize(WireData::Legacy(legacy)).unwrap(),
            WorkflowValue::object([("hello", WorkflowValue::String("world".into()))])
        );
    }

    #[test]
    fn serialization_serialization_test_step_and_client_modes_round_trip_with_encryption() {
        let key = import_key(&[0x42; 32]).unwrap();
        let value = complex_value();

        let encrypted = step_serialize(&value, Some(&key)).unwrap();
        assert_eq!(peek_format_prefix(&encrypted).as_deref(), Some(ENCRYPTED));
        assert_eq!(step_deserialize(encrypted, Some(&key)).unwrap(), value);

        let encrypted = client_serialize(&value, Some(&key)).unwrap();
        assert_eq!(peek_format_prefix(&encrypted).as_deref(), Some(ENCRYPTED));
        assert_eq!(client_deserialize(encrypted, Some(&key)).unwrap(), value);
    }

    #[test]
    fn serialization_serialization_test_step_and_client_modes_reject_encrypted_data_without_key() {
        let key = import_key(&[0x42; 32]).unwrap();
        let value = WorkflowValue::object([("test", WorkflowValue::Bool(true))]);
        let encrypted = step_serialize(&value, Some(&key)).unwrap();

        assert!(matches!(
            step_deserialize(encrypted.clone(), None),
            Err(WorkflowCoreError::RuntimeDecryption(_))
        ));
        assert!(matches!(
            client_deserialize(encrypted, None),
            Err(WorkflowCoreError::RuntimeDecryption(_))
        ));
    }

    #[test]
    fn serialization_serialization_test_cross_mode_serialization_is_compatible() {
        let key = import_key(&[0x42; 32]).unwrap();
        let value = complex_value();

        let workflow_data = workflow_serialize(&value).unwrap();
        assert_eq!(
            step_deserialize(workflow_data.clone(), None).unwrap(),
            value
        );
        assert_eq!(client_deserialize(workflow_data, None).unwrap(), value);

        let step_data = step_serialize(&value, None).unwrap();
        assert_eq!(workflow_deserialize(step_data.clone()).unwrap(), value);
        assert_eq!(client_deserialize(step_data, None).unwrap(), value);

        let encrypted = step_serialize(&value, Some(&key)).unwrap();
        assert_eq!(client_deserialize(encrypted, Some(&key)).unwrap(), value);
    }

    #[test]
    fn serialization_serialization_test_edge_cases_cover_undefined_deep_mixed_empty_and_bigint_values()
     {
        let mut nested = WorkflowValue::object([("depth", WorkflowValue::Number(0.0))]);
        for depth in 1..=50 {
            nested = WorkflowValue::object([
                ("depth", WorkflowValue::Number(depth as f64)),
                ("child", nested),
            ]);
        }

        let mixed = WorkflowValue::Array(vec![
            WorkflowValue::Number(42.0),
            WorkflowValue::String("string".into()),
            WorkflowValue::Bool(true),
            WorkflowValue::Null,
            WorkflowValue::date("2025-01-01T00:00:00.000Z"),
            WorkflowValue::Map(vec![(
                WorkflowValue::String("k".into()),
                WorkflowValue::String("v".into()),
            )]),
            WorkflowValue::Set(vec![WorkflowValue::Number(1.0)]),
            WorkflowValue::RegExp {
                source: "test".into(),
                flags: "g".into(),
            },
            WorkflowValue::Url("https://example.com/".into()),
        ]);

        let value = WorkflowValue::object([
            ("a", WorkflowValue::Number(1.0)),
            ("b", WorkflowValue::Undefined),
            ("nested", nested),
            ("mixed", mixed),
            ("emptyMap", WorkflowValue::Map(Vec::new())),
            ("emptySet", WorkflowValue::Set(Vec::new())),
            ("big0", WorkflowValue::BigInt("0".into())),
            ("bigNegative", WorkflowValue::BigInt("-1".into())),
            ("emptyBytes", WorkflowValue::uint8_array(&[])),
        ]);

        assert_eq!(
            workflow_deserialize(workflow_serialize(&value).unwrap()).unwrap(),
            value
        );
    }

    trait CodecPrefix {
        fn format_prefix(self) -> &'static str;
    }

    impl CodecPrefix for DevalueCodec {
        fn format_prefix(self) -> &'static str {
            DevalueCodec::FORMAT_PREFIX
        }
    }

    #[test]
    fn serialization_test_format_prefix_system_handles_all_dehydrate_hydrate_pairs() {
        let key = import_key(&[0x42; 32]).unwrap();
        let values = [
            WorkflowValue::String("workflow args".into()),
            WorkflowValue::String("step args".into()),
            WorkflowValue::String("step return".into()),
            WorkflowValue::Error(WorkflowErrorValue::new("FatalError", "boom")),
        ];

        for value in values {
            let plain = serialize(&value, SerializationMode::Workflow, None).unwrap();
            assert_eq!(peek_format_prefix(&plain).as_deref(), Some(DEVALUE_V1));
            assert_eq!(
                deserialize(plain, SerializationMode::Workflow, None).unwrap(),
                value
            );

            let encrypted = serialize(&value, SerializationMode::Workflow, Some(&key)).unwrap();
            assert_eq!(peek_format_prefix(&encrypted).as_deref(), Some(ENCRYPTED));
            assert_eq!(
                deserialize(encrypted, SerializationMode::Workflow, Some(&key)).unwrap(),
                value
            );
        }
    }

    #[test]
    fn serialization_test_snapshot_like_wire_json_is_stable_for_portable_values() {
        let value = WorkflowValue::object([
            ("date", WorkflowValue::date("2025-01-01T00:00:00.000Z")),
            ("bytes", WorkflowValue::uint8_array(&[1, 2, 3])),
        ]);
        let decoded = decode_format_prefix(workflow_serialize(&value).unwrap()).unwrap();
        let json: Value = serde_json::from_slice(&decoded.payload).unwrap();

        assert_eq!(
            json,
            json!({
                "$type": "Object",
                "value": {
                    "bytes": {
                        "$type": "TypedArray",
                        "value": { "kind": "Uint8Array", "base64": "AQID" }
                    },
                    "date": {
                        "$type": "Date",
                        "value": "2025-01-01T00:00:00.000Z"
                    }
                }
            })
        );
    }

    #[test]
    fn serialization_test_decode_format_prefix_legacy_compatibility_handles_plain_values() {
        assert_eq!(
            workflow_deserialize(WireData::Legacy(json!({"legacy": true}))).unwrap(),
            WorkflowValue::Json(json!({"legacy": true}))
        );
        assert_eq!(
            workflow_deserialize(WireData::Legacy(json!([1, 2]))).unwrap(),
            WorkflowValue::Json(json!([1, 2]))
        );
        assert_eq!(
            workflow_deserialize(WireData::Legacy(Value::Null)).unwrap(),
            WorkflowValue::Json(Value::Null)
        );
    }

    #[test]
    fn serialization_serialization_test_step_function_reducer_preserves_closure_and_binding_payloads()
     {
        let mut closure_vars = BTreeMap::new();
        closure_vars.insert("x".into(), WorkflowValue::Number(1.0));
        closure_vars.insert("y".into(), WorkflowValue::String("hello".into()));
        let step_fn = WorkflowValue::StepFunction(crate::value::StepFunctionValue {
            step_id: "step//test//withClosure".into(),
            closure_vars: Some(closure_vars),
            bound_this: Some(Box::new(WorkflowValue::object([(
                "kind",
                WorkflowValue::String("workflow-side".into()),
            )]))),
            bound_args: Some(vec![WorkflowValue::Number(7.0)]),
        });

        let serialized = DEVALUE_CODEC
            .serialize_payload(&step_fn, SerializationMode::Workflow)
            .unwrap();
        assert_eq!(
            DEVALUE_CODEC
                .deserialize_payload(&serialized, SerializationMode::Workflow)
                .unwrap(),
            step_fn
        );
    }
}
