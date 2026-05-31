use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// Opaque serialized workflow data.
///
/// Spec version 2 and later use binary devalue payloads. Legacy spec version 1
/// stored JSON-like data directly, so the contract keeps both shapes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SerializedData {
    Binary(Vec<u8>),
    Legacy(JsonValue),
}

impl From<Vec<u8>> for SerializedData {
    fn from(value: Vec<u8>) -> Self {
        Self::Binary(value)
    }
}

impl From<JsonValue> for SerializedData {
    fn from(value: JsonValue) -> Self {
        Self::Legacy(value)
    }
}
