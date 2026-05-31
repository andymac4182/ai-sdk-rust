use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::codec::{SerializationMode, deserialize};
use crate::encryption::CryptoKey;
use crate::error::{Result, WorkflowCoreError};
use crate::format::{WireData, is_encrypted};
use crate::value::WorkflowValue;

pub const STREAM_ID_PREFIX: &str = "strm_";
pub const STREAM_REF_TYPE: &str = "__workflow_stream_ref__";
pub const RUN_REF_TYPE: &str = "__workflow_run_ref__";
pub const CLASS_INSTANCE_REF_TYPE: &str = "__workflow_class_instance_ref__";

#[derive(Clone, Debug, PartialEq)]
pub enum HydratedData {
    Value(WorkflowValue),
    Encrypted(Vec<u8>),
}

pub fn hydrate_data(data: WireData) -> Result<HydratedData> {
    if is_encrypted(&data) {
        let bytes = data.into_bytes().unwrap_or_default();
        return Ok(HydratedData::Encrypted(bytes));
    }
    deserialize(data, SerializationMode::Workflow, None).map(HydratedData::Value)
}

pub fn hydrate_data_with_key(data: WireData, key: Option<&CryptoKey>) -> Result<HydratedData> {
    if is_encrypted(&data) && key.is_none() {
        let bytes = data.into_bytes().unwrap_or_default();
        return Ok(HydratedData::Encrypted(bytes));
    }
    deserialize(data, SerializationMode::Workflow, key).map(HydratedData::Value)
}

pub fn stream_to_stream_ref(name: Option<&str>) -> WorkflowValue {
    let stream_id = match name {
        Some(name) if name.starts_with(STREAM_ID_PREFIX) => name.to_string(),
        Some(name) => format!("{STREAM_ID_PREFIX}{name}"),
        None => format!("{STREAM_ID_PREFIX}null"),
    };
    WorkflowValue::StreamRef { stream_id }
}

pub fn serialized_step_function_to_string(value: &WorkflowValue) -> String {
    match value {
        WorkflowValue::StepFunction(step) => format!("<step:{}>", step.step_id),
        WorkflowValue::Null => "null".to_string(),
        _ => "<function>".to_string(),
    }
}

pub fn extract_class_name(class_id: &str) -> &str {
    if class_id.is_empty() {
        return "Unknown";
    }
    class_id
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .unwrap_or(class_id)
}

pub fn serialized_instance_to_ref(class_id: &str, data: WorkflowValue) -> WorkflowValue {
    let class_name = extract_class_name(class_id).to_string();
    if class_name == "Run" {
        if let WorkflowValue::Object(fields) = &data {
            if let Some(WorkflowValue::String(run_id)) = fields.get("runId") {
                return WorkflowValue::RunRef {
                    run_id: run_id.clone(),
                };
            }
        }
    }
    WorkflowValue::Json(serde_json::json!({
        "__type": CLASS_INSTANCE_REF_TYPE,
        "className": class_name,
        "classId": class_id,
        "data": data,
    }))
}

pub fn serialized_class_to_string(class_id: &str) -> String {
    format!("<class:{}>", extract_class_name(class_id))
}

pub fn serialized_workflow_function_to_string(workflow_id: &str) -> String {
    format!("<workflow:{workflow_id}>")
}

pub fn is_stream_id(value: &str) -> bool {
    value.starts_with(STREAM_ID_PREFIX)
}

pub fn is_stream_ref(value: &WorkflowValue) -> bool {
    matches!(value, WorkflowValue::StreamRef { stream_id } if is_stream_id(stream_id))
}

pub fn extract_stream_ids(value: &WorkflowValue) -> Vec<String> {
    let mut ids = BTreeSet::new();
    collect_stream_ids(value, &mut ids);
    ids.into_iter().collect()
}

fn collect_stream_ids(value: &WorkflowValue, ids: &mut BTreeSet<String>) {
    match value {
        WorkflowValue::String(value) if is_stream_id(value) => {
            ids.insert(value.clone());
        }
        WorkflowValue::StreamRef { stream_id } if is_stream_id(stream_id) => {
            ids.insert(stream_id.clone());
        }
        WorkflowValue::Array(values) | WorkflowValue::Set(values) => {
            for value in values {
                collect_stream_ids(value, ids);
            }
        }
        WorkflowValue::Object(fields) => {
            for value in fields.values() {
                collect_stream_ids(value, ids);
            }
        }
        WorkflowValue::Map(entries) => {
            for (key, value) in entries {
                collect_stream_ids(key, ids);
                collect_stream_ids(value, ids);
            }
        }
        WorkflowValue::Instance { data, .. } => collect_stream_ids(data, ids),
        _ => {}
    }
}

pub fn truncate_id(id: &str, max_length: usize) -> String {
    if id.len() <= max_length {
        id.to_string()
    } else {
        format!("{}...", &id[..max_length])
    }
}

pub fn truncate_id_default(id: &str) -> String {
    truncate_id(id, 12)
}

pub fn is_expired_stub(value: &WorkflowValue) -> bool {
    match value {
        WorkflowValue::ExpiredStub { .. } => true,
        WorkflowValue::Object(fields) => is_expired_object(fields),
        WorkflowValue::Array(values) if values.len() == 1 => is_expired_stub(&values[0]),
        _ => false,
    }
}

fn is_expired_object(fields: &BTreeMap<String, WorkflowValue>) -> bool {
    matches!(
        fields.get("expiredAt"),
        Some(WorkflowValue::String(_)) if fields.len() == 1
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct IoResource {
    pub input: Option<WireData>,
    pub output: Option<WireData>,
    pub error: Option<WireData>,
    pub metadata: Option<WireData>,
    pub payload: Option<WireData>,
    pub result: Option<WireData>,
    pub execution_context: Option<Value>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HydratedIoResource {
    pub input: Option<HydratedData>,
    pub output: Option<HydratedData>,
    pub error: Option<HydratedData>,
    pub metadata: Option<HydratedData>,
    pub payload: Option<HydratedData>,
    pub result: Option<HydratedData>,
    pub workflow_core_version: Option<String>,
}

pub fn hydrate_resource_io(resource: Option<IoResource>) -> Result<Option<HydratedIoResource>> {
    let Some(resource) = resource else {
        return Ok(None);
    };

    let workflow_core_version = resource
        .execution_context
        .as_ref()
        .and_then(|context| context.get("workflowCoreVersion"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    Ok(Some(HydratedIoResource {
        input: hydrate_optional(resource.input)?,
        output: hydrate_optional(resource.output)?,
        error: hydrate_optional(resource.error)?,
        metadata: hydrate_optional(resource.metadata)?,
        payload: hydrate_optional(resource.payload)?,
        result: hydrate_optional(resource.result)?,
        workflow_core_version,
    }))
}

fn hydrate_optional(data: Option<WireData>) -> Result<Option<HydratedData>> {
    data.map(hydrate_data).transpose()
}

pub fn leave_unhydrated_on_parse_error(data: WireData) -> HydratedData {
    hydrate_data(data.clone()).unwrap_or_else(|_| {
        HydratedData::Value(WorkflowValue::Json(match data {
            WireData::Bytes(bytes) => Value::Array(
                bytes
                    .into_iter()
                    .map(|byte| Value::Number(byte.into()))
                    .collect(),
            ),
            WireData::Legacy(value) => value,
        }))
    })
}

pub fn unsupported_format_error(format: impl Into<String>) -> WorkflowCoreError {
    WorkflowCoreError::UnsupportedSerializationFormat(format.into())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::codec::workflow_serialize;
    use crate::encryption::import_key;
    use crate::format::{ENCRYPTED, encode_with_format_prefix, peek_format_prefix};

    fn serialized(value: WorkflowValue) -> WireData {
        workflow_serialize(&value).unwrap()
    }

    #[test]
    fn serialization_format_test_hydrate_data_parses_prefixed_and_legacy_values() {
        let value = WorkflowValue::object([("hello", WorkflowValue::String("world".into()))]);
        assert_eq!(
            hydrate_data(serialized(value.clone())).unwrap(),
            HydratedData::Value(value)
        );
        assert_eq!(
            hydrate_data(WireData::Legacy(json!({"plain": true}))).unwrap(),
            HydratedData::Value(WorkflowValue::Json(json!({"plain": true})))
        );
    }

    #[test]
    fn serialization_format_test_hydrate_data_passes_plain_values_and_rejects_unknown_prefix() {
        assert_eq!(
            hydrate_data(WireData::Legacy(json!("plain"))).unwrap(),
            HydratedData::Value(WorkflowValue::Json(json!("plain")))
        );
        assert!(matches!(
            hydrate_data(WireData::Bytes(b"cbor{}".to_vec())),
            Err(WorkflowCoreError::UnsupportedSerializationFormat(format)) if format == "cbor"
        ));
    }

    #[test]
    fn serialization_format_test_hydrate_resource_io_hydrates_step_run_event_and_hook_fields() {
        let input = serialized(WorkflowValue::String("input".into()));
        let output = serialized(WorkflowValue::String("output".into()));
        let error = serialized(WorkflowValue::Error(crate::value::WorkflowErrorValue::new(
            "Error", "boom",
        )));
        let metadata = serialized(WorkflowValue::object([(
            "hook",
            WorkflowValue::String("metadata".into()),
        )]));
        let payload = serialized(WorkflowValue::object([(
            "message",
            WorkflowValue::String("payload".into()),
        )]));
        let result = serialized(WorkflowValue::String("result".into()));

        let hydrated = hydrate_resource_io(Some(IoResource {
            input: Some(input),
            output: Some(output),
            error: Some(error),
            metadata: Some(metadata),
            payload: Some(payload),
            result: Some(result),
            execution_context: Some(json!({"workflowCoreVersion":"5.0.0-beta.10","secret":true})),
        }))
        .unwrap()
        .unwrap();

        assert_eq!(
            hydrated.input,
            Some(HydratedData::Value(WorkflowValue::String("input".into())))
        );
        assert_eq!(
            hydrated.output,
            Some(HydratedData::Value(WorkflowValue::String("output".into())))
        );
        assert!(matches!(
            hydrated.error,
            Some(HydratedData::Value(WorkflowValue::Error(_)))
        ));
        assert!(matches!(
            hydrated.metadata,
            Some(HydratedData::Value(WorkflowValue::Object(_)))
        ));
        assert!(matches!(
            hydrated.payload,
            Some(HydratedData::Value(WorkflowValue::Object(_)))
        ));
        assert_eq!(
            hydrated.result,
            Some(HydratedData::Value(WorkflowValue::String("result".into())))
        );
        assert_eq!(
            hydrated.workflow_core_version.as_deref(),
            Some("5.0.0-beta.10")
        );
    }

    #[test]
    fn serialization_format_test_hydrate_resource_io_handles_null_and_parse_errors_gracefully() {
        assert_eq!(hydrate_resource_io(None).unwrap(), None);
        assert!(matches!(
            leave_unhydrated_on_parse_error(WireData::Bytes(b"devl{".to_vec())),
            HydratedData::Value(WorkflowValue::Json(Value::Array(_)))
        ));
    }

    #[test]
    fn serialization_format_test_observability_revivers_convert_stream_step_class_and_workflow_refs()
     {
        assert_eq!(
            stream_to_stream_ref(Some("abc")),
            WorkflowValue::StreamRef {
                stream_id: "strm_abc".into()
            }
        );
        assert_eq!(
            stream_to_stream_ref(Some("strm_existing")),
            WorkflowValue::StreamRef {
                stream_id: "strm_existing".into()
            }
        );
        assert_eq!(
            serialized_step_function_to_string(&WorkflowValue::step_function("step//x")),
            "<step:step//x>"
        );
        assert_eq!(
            serialized_class_to_string("@workflow/example//Widget"),
            "<class:Widget>"
        );
        assert_eq!(
            serialized_workflow_function_to_string("workflow-1"),
            "<workflow:workflow-1>"
        );
        assert!(matches!(
            serialized_instance_to_ref("@workflow/example//Widget", WorkflowValue::Null),
            WorkflowValue::Json(_)
        ));
    }

    #[test]
    fn serialization_format_test_stream_ref_stream_id_extract_and_truncate_helpers() {
        assert!(is_stream_id("strm_123"));
        assert!(!is_stream_id("step_123"));
        assert!(is_stream_ref(&WorkflowValue::StreamRef {
            stream_id: "strm_123".into()
        }));

        let ids = extract_stream_ids(&WorkflowValue::object([
            ("a", WorkflowValue::String("strm_a".into())),
            (
                "nested",
                WorkflowValue::Array(vec![
                    WorkflowValue::String("strm_b".into()),
                    WorkflowValue::String("strm_a".into()),
                ]),
            ),
        ]));
        assert_eq!(ids, vec!["strm_a".to_string(), "strm_b".to_string()]);

        assert_eq!(truncate_id("short", 12), "short");
        assert_eq!(truncate_id("abcdefghijklmnop", 8), "abcdefgh...");
        assert_eq!(truncate_id_default("abcdefghijklmnop"), "abcdefghijkl...");
    }

    #[test]
    fn serialization_format_test_encrypted_data_handling_passes_through_or_decrypts() {
        let key = import_key(&[0x42; 32]).unwrap();
        let value = serialized(WorkflowValue::String("secret".into()));
        let encrypted = crate::encryption::maybe_encrypt(value.clone(), Some(&key)).unwrap();

        assert_eq!(peek_format_prefix(&encrypted).as_deref(), Some(ENCRYPTED));
        assert!(matches!(
            hydrate_data_with_key(encrypted.clone(), None).unwrap(),
            HydratedData::Encrypted(_)
        ));
        assert_eq!(
            hydrate_data_with_key(encrypted, Some(&key)).unwrap(),
            HydratedData::Value(WorkflowValue::String("secret".into()))
        );
        assert_eq!(
            hydrate_data_with_key(value, Some(&key)).unwrap(),
            HydratedData::Value(WorkflowValue::String("secret".into()))
        );
    }

    #[test]
    fn serialization_format_test_is_encrypted_data_detects_only_encr_prefixed_bytes() {
        let encr = encode_with_format_prefix(ENCRYPTED, WireData::Bytes(vec![1])).unwrap();
        let devl = serialized(WorkflowValue::String("plain".into()));

        assert!(crate::format::is_encrypted(&encr));
        assert!(!crate::format::is_encrypted(&devl));
        assert!(!crate::format::is_encrypted(&WireData::Legacy(Value::Null)));
        assert!(!crate::format::is_encrypted(&WireData::Bytes(vec![
            1, 2, 3
        ])));
    }

    #[test]
    fn serialization_format_test_is_expired_stub_matches_exact_server_shapes() {
        assert!(is_expired_stub(&WorkflowValue::ExpiredStub {
            expired_at: "2026-01-01T00:00:00.000Z".into()
        }));
        assert!(is_expired_stub(&WorkflowValue::object([(
            "expiredAt",
            WorkflowValue::String("2026-01-01T00:00:00.000Z".into()),
        )])));
        assert!(is_expired_stub(&WorkflowValue::Array(vec![
            WorkflowValue::object([(
                "expiredAt",
                WorkflowValue::String("2026-01-01T00:00:00.000Z".into()),
            )])
        ])));
        assert!(!is_expired_stub(&WorkflowValue::object([
            ("expiredAt", WorkflowValue::String("2026".into())),
            ("extra", WorkflowValue::Bool(true)),
        ])));
        assert!(!is_expired_stub(&WorkflowValue::object([(
            "expiredAt",
            WorkflowValue::Number(1.0),
        )])));
        assert!(!is_expired_stub(&WorkflowValue::Null));
        assert!(!is_expired_stub(&WorkflowValue::object([(
            "different",
            WorkflowValue::String("2026".into()),
        )])));
        assert!(!is_expired_stub(&WorkflowValue::Array(vec![
            WorkflowValue::object([("expiredAt", WorkflowValue::String("2026".into()))]),
            WorkflowValue::object([("expiredAt", WorkflowValue::String("2027".into()))]),
        ])));
    }
}
