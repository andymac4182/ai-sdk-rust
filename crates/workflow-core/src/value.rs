use std::collections::BTreeMap;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Result, WorkflowCoreError};

const EMPTY_BYTES_SENTINEL: &str = ".";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowErrorValue {
    pub name: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<Box<WorkflowValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflicting_run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<WorkflowValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
}

impl WorkflowErrorValue {
    pub fn new(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            message: message.into(),
            stack: None,
            cause: None,
            retry_after_ms: None,
            token: None,
            conflicting_run_id: None,
            errors: None,
            context: None,
        }
    }

    pub fn with_stack(mut self, stack: impl Into<String>) -> Self {
        self.stack = Some(stack.into());
        self
    }

    pub fn with_cause(mut self, cause: WorkflowValue) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypedArrayKind {
    Int8Array,
    Int16Array,
    Int32Array,
    BigInt64Array,
    Uint8Array,
    Uint8ClampedArray,
    Uint16Array,
    Uint32Array,
    BigUint64Array,
    Float32Array,
    Float64Array,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepFunctionValue {
    pub step_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub closure_vars: Option<BTreeMap<String, WorkflowValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound_this: Option<Box<WorkflowValue>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound_args: Option<Vec<WorkflowValue>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "$type", content = "value")]
pub enum WorkflowValue {
    Null,
    Undefined,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<WorkflowValue>),
    Object(BTreeMap<String, WorkflowValue>),
    BigInt(String),
    Date(String),
    InvalidDate,
    ArrayBuffer(String),
    TypedArray {
        kind: TypedArrayKind,
        base64: String,
    },
    Map(Vec<(WorkflowValue, WorkflowValue)>),
    Set(Vec<WorkflowValue>),
    Url(String),
    UrlSearchParams(String),
    RegExp {
        source: String,
        flags: String,
    },
    Headers(Vec<(String, String)>),
    Error(WorkflowErrorValue),
    DomException(WorkflowErrorValue),
    Class {
        class_id: String,
    },
    Instance {
        class_id: String,
        data: Box<WorkflowValue>,
    },
    StepFunction(StepFunctionValue),
    WorkflowFunction {
        workflow_id: String,
    },
    StreamRef {
        stream_id: String,
    },
    RunRef {
        run_id: String,
    },
    ExpiredStub {
        expired_at: String,
    },
    Json(Value),
}

impl WorkflowValue {
    pub fn object(fields: impl IntoIterator<Item = (impl Into<String>, WorkflowValue)>) -> Self {
        Self::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    }

    pub fn array_buffer(bytes: &[u8]) -> Self {
        Self::ArrayBuffer(bytes_to_base64(bytes))
    }

    pub fn typed_array(kind: TypedArrayKind, bytes: &[u8]) -> Self {
        Self::TypedArray {
            kind,
            base64: bytes_to_base64(bytes),
        }
    }

    pub fn uint8_array(bytes: &[u8]) -> Self {
        Self::typed_array(TypedArrayKind::Uint8Array, bytes)
    }

    pub fn date(iso: impl Into<String>) -> Self {
        Self::Date(iso.into())
    }

    pub fn url_search_params(value: impl Into<String>) -> Self {
        let value = value.into();
        if value.is_empty() {
            Self::UrlSearchParams(EMPTY_BYTES_SENTINEL.to_string())
        } else {
            Self::UrlSearchParams(value)
        }
    }

    pub fn step_function(step_id: impl Into<String>) -> Self {
        Self::StepFunction(StepFunctionValue {
            step_id: step_id.into(),
            closure_vars: None,
            bound_this: None,
            bound_args: None,
        })
    }

    pub fn contains_step_function(&self) -> bool {
        match self {
            Self::StepFunction(_) => true,
            Self::Array(values) | Self::Set(values) => {
                values.iter().any(Self::contains_step_function)
            }
            Self::Object(values) => values.values().any(Self::contains_step_function),
            Self::Map(entries) => entries
                .iter()
                .any(|(key, value)| key.contains_step_function() || value.contains_step_function()),
            Self::Instance { data, .. } => data.contains_step_function(),
            Self::Error(error) | Self::DomException(error) => {
                error
                    .cause
                    .as_deref()
                    .is_some_and(Self::contains_step_function)
                    || error
                        .errors
                        .as_ref()
                        .is_some_and(|errors| errors.iter().any(Self::contains_step_function))
            }
            _ => false,
        }
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>> {
        serde_json::to_vec(self)
            .map_err(|error| WorkflowCoreError::Serialization(error.to_string()))
    }

    pub fn from_json_bytes(bytes: &[u8]) -> Result<Self> {
        serde_json::from_slice(bytes)
            .map_err(|error| WorkflowCoreError::Deserialization(error.to_string()))
    }

    pub fn decode_array_buffer(&self) -> Result<Option<Vec<u8>>> {
        match self {
            Self::ArrayBuffer(base64) | Self::TypedArray { base64, .. } => {
                Ok(Some(base64_to_bytes(base64)?))
            }
            _ => Ok(None),
        }
    }
}

pub fn bytes_to_base64(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        EMPTY_BYTES_SENTINEL.to_string()
    } else {
        STANDARD.encode(bytes)
    }
}

pub fn base64_to_bytes(value: &str) -> Result<Vec<u8>> {
    let value = if value == EMPTY_BYTES_SENTINEL {
        ""
    } else {
        value
    };
    STANDARD
        .decode(value)
        .map_err(|error| WorkflowCoreError::InvalidBase64(error.to_string()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn serialization_serialization_test_common_reducers_reduce_arraybuffer_and_zero_length() {
        assert_eq!(
            WorkflowValue::array_buffer(&[1, 2, 3]),
            WorkflowValue::ArrayBuffer("AQID".into())
        );
        assert_eq!(
            WorkflowValue::array_buffer(&[]),
            WorkflowValue::ArrayBuffer(".".into())
        );
        assert_eq!(
            WorkflowValue::array_buffer(&[1, 2, 3])
                .decode_array_buffer()
                .unwrap(),
            Some(vec![1, 2, 3])
        );
    }

    #[test]
    fn serialization_serialization_test_common_reducers_reduce_scalar_special_values() {
        assert_eq!(
            WorkflowValue::BigInt("42".into()),
            WorkflowValue::BigInt("42".into())
        );
        assert_eq!(
            WorkflowValue::date("2025-06-15T12:00:00.000Z"),
            WorkflowValue::Date("2025-06-15T12:00:00.000Z".into())
        );
        assert_eq!(WorkflowValue::InvalidDate, WorkflowValue::InvalidDate);
        assert_eq!(
            WorkflowValue::url_search_params(""),
            WorkflowValue::UrlSearchParams(".".into())
        );
    }

    #[test]
    fn serialization_serialization_test_common_reducers_reduce_maps_sets_headers_urls_and_regex() {
        let map = WorkflowValue::Map(vec![
            (
                WorkflowValue::String("a".into()),
                WorkflowValue::Number(1.0),
            ),
            (
                WorkflowValue::String("b".into()),
                WorkflowValue::Number(2.0),
            ),
        ]);
        let set = WorkflowValue::Set(vec![
            WorkflowValue::Number(1.0),
            WorkflowValue::Number(2.0),
            WorkflowValue::Number(3.0),
        ]);
        let headers = WorkflowValue::Headers(vec![("content-type".into(), "text/plain".into())]);
        let regex = WorkflowValue::RegExp {
            source: "foo".into(),
            flags: "gi".into(),
        };
        let url = WorkflowValue::Url("https://example.com/path".into());

        assert!(matches!(map, WorkflowValue::Map(entries) if entries.len() == 2));
        assert!(matches!(set, WorkflowValue::Set(values) if values.len() == 3));
        assert_eq!(
            headers,
            WorkflowValue::Headers(vec![("content-type".into(), "text/plain".into())])
        );
        assert_eq!(
            regex,
            WorkflowValue::RegExp {
                source: "foo".into(),
                flags: "gi".into()
            }
        );
        assert_eq!(url, WorkflowValue::Url("https://example.com/path".into()));
    }

    #[test]
    fn serialization_serialization_test_common_reducers_reduce_typed_arrays() {
        let cases = [
            (TypedArrayKind::Int8Array, vec![1_u8, 254]),
            (TypedArrayKind::Int16Array, 1000_i16.to_le_bytes().to_vec()),
            (
                TypedArrayKind::Int32Array,
                100000_i32.to_le_bytes().to_vec(),
            ),
            (TypedArrayKind::Float32Array, 1.5_f32.to_le_bytes().to_vec()),
            (
                TypedArrayKind::Float64Array,
                1.123456789_f64.to_le_bytes().to_vec(),
            ),
            (TypedArrayKind::Uint8ClampedArray, vec![255, 0]),
            (
                TypedArrayKind::Uint16Array,
                65535_u16.to_le_bytes().to_vec(),
            ),
            (
                TypedArrayKind::Uint32Array,
                4294967295_u32.to_le_bytes().to_vec(),
            ),
        ];

        for (kind, bytes) in cases {
            let value = WorkflowValue::typed_array(kind, &bytes);
            assert_eq!(value.decode_array_buffer().unwrap(), Some(bytes));
        }
    }

    #[test]
    fn serialization_serialization_test_common_revivers_round_trip_portable_special_values() {
        let value = WorkflowValue::object([
            ("date", WorkflowValue::date("2025-01-01T00:00:00.000Z")),
            (
                "map",
                WorkflowValue::Map(vec![(
                    WorkflowValue::String("k".into()),
                    WorkflowValue::Number(42.0),
                )]),
            ),
            ("set", WorkflowValue::Set(vec![WorkflowValue::Number(1.0)])),
            ("url", WorkflowValue::Url("https://example.com/".into())),
            (
                "regex",
                WorkflowValue::RegExp {
                    source: "test\\d+".into(),
                    flags: "i".into(),
                },
            ),
            ("bytes", WorkflowValue::uint8_array(&[1, 2, 3])),
        ]);

        let bytes = value.to_json_bytes().unwrap();
        assert_eq!(WorkflowValue::from_json_bytes(&bytes).unwrap(), value);
    }

    #[test]
    fn serialization_serialization_test_common_revivers_round_trip_error_payloads() {
        let error = WorkflowValue::Error(
            WorkflowErrorValue::new("RangeError", "out of range")
                .with_stack("RangeError: out of range"),
        );
        let aggregate = WorkflowValue::Error(WorkflowErrorValue {
            name: "AggregateError".into(),
            message: "many".into(),
            stack: None,
            cause: None,
            retry_after_ms: None,
            token: None,
            conflicting_run_id: None,
            errors: Some(vec![WorkflowValue::String("not an error".into())]),
            context: None,
        });
        let fatal = WorkflowValue::Error(
            WorkflowErrorValue::new("FatalError", "fatal")
                .with_cause(WorkflowValue::String("because".into())),
        );

        for value in [error, aggregate, fatal] {
            assert_eq!(
                WorkflowValue::from_json_bytes(&value.to_json_bytes().unwrap()).unwrap(),
                value
            );
        }
    }

    #[test]
    fn serialization_serialization_test_class_and_function_reference_payloads_are_data_only() {
        let class_ref = WorkflowValue::Class {
            class_id: "test-class-id".into(),
        };
        let instance_ref = WorkflowValue::Instance {
            class_id: "serializable-test".into(),
            data: Box::new(WorkflowValue::object([("v", WorkflowValue::Number(42.0))])),
        };
        let workflow_fn = WorkflowValue::WorkflowFunction {
            workflow_id: "workflow-1".into(),
        };
        let step_fn = WorkflowValue::step_function("step//test//myStep");

        assert_eq!(
            serde_json::to_value(&class_ref).unwrap(),
            json!({"$type":"Class","value":{"class_id":"test-class-id"}})
        );
        assert!(!class_ref.contains_step_function());
        assert!(!instance_ref.contains_step_function());
        assert!(!workflow_fn.contains_step_function());
        assert!(step_fn.contains_step_function());
    }
}
