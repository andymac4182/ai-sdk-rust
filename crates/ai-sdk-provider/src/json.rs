use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::{Map, Value};

/// A JSON value that can be passed to or returned from model providers.
///
/// This mirrors the AI SDK's `JSONValue` type while using serde's standard JSON
/// representation for idiomatic Rust serialization and deserialization.
pub type JsonValue = Value;

/// A JSON object keyed by string values.
pub type JsonObject = Map<String, JsonValue>;

/// A JSON schema object.
///
/// The upstream AI SDK uses JSON Schema 7 objects for provider-facing schemas.
pub type JsonSchema = JsonObject;

/// A JSON array.
pub type JsonArray = Vec<JsonValue>;

/// Returns whether the supplied value is a valid JSON value.
///
/// This mirrors upstream `isJSONValue`. Rust's [`JsonValue`] type is already
/// JSON by construction, so this helper is primarily useful for API parity and
/// for code that works uniformly with the adjacent array/object guards.
pub fn is_json_value(value: &JsonValue) -> bool {
    let _ = value;
    true
}

/// Returns whether the supplied JSON value is an array.
///
/// Nested values are JSON by construction in Rust, matching upstream's
/// recursive `isJSONArray` validation at the serde boundary.
pub fn is_json_array(value: &JsonValue) -> bool {
    value
        .as_array()
        .is_some_and(|array| array.iter().all(is_json_value))
}

/// Returns whether the supplied JSON value is an object.
///
/// Object keys and nested values are JSON by construction in Rust, matching
/// upstream's recursive `isJSONObject` validation at the serde boundary.
pub fn is_json_object(value: &JsonValue) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.values().all(is_json_value))
}

/// Error returned when a JSON value must not be null.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NullJsonValueError;

impl fmt::Display for NullJsonValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON values cannot be null in this position")
    }
}

impl std::error::Error for NullJsonValueError {}

/// A JSON value that rejects `null`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NonNullJsonValue(JsonValue);

impl NonNullJsonValue {
    /// Creates a non-null JSON value.
    pub fn new(value: JsonValue) -> Result<Self, NullJsonValueError> {
        if value.is_null() {
            return Err(NullJsonValueError);
        }

        Ok(Self(value))
    }

    /// Borrows the inner JSON value.
    pub fn as_value(&self) -> &JsonValue {
        &self.0
    }

    /// Converts this wrapper into the inner JSON value.
    pub fn into_value(self) -> JsonValue {
        self.0
    }
}

impl TryFrom<JsonValue> for NonNullJsonValue {
    type Error = NullJsonValueError;

    fn try_from(value: JsonValue) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<NonNullJsonValue> for JsonValue {
    fn from(value: NonNullJsonValue) -> Self {
        value.into_value()
    }
}

impl Serialize for NonNullJsonValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for NonNullJsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(JsonValue::deserialize(deserializer)?).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        JsonArray, JsonObject, JsonValue, NonNullJsonValue, is_json_array, is_json_object,
        is_json_value,
    };
    use serde_json::json;

    #[test]
    fn aliases_cover_json_object_array_and_value_shapes() {
        let mut object = JsonObject::new();
        object.insert(
            "provider".to_string(),
            JsonValue::String("openai".to_string()),
        );

        let array: JsonArray = vec![JsonValue::Bool(true), JsonValue::Null];

        assert_eq!(JsonValue::Object(object), json!({ "provider": "openai" }));
        assert_eq!(JsonValue::Array(array), json!([true, null]));
    }

    #[test]
    fn json_value_guard_accepts_all_serde_json_values() {
        for value in [
            JsonValue::Null,
            JsonValue::Bool(true),
            json!(42),
            json!("text"),
            json!([null, { "nested": false }]),
            json!({ "provider": { "enabled": true } }),
        ] {
            assert!(is_json_value(&value));
        }
    }

    #[test]
    fn json_array_guard_matches_only_arrays() {
        assert!(is_json_array(&json!([1, "two", { "three": true }])));
        assert!(is_json_array(&json!([])));
        assert!(!is_json_array(&json!({ "not": "array" })));
        assert!(!is_json_array(&JsonValue::Null));
    }

    #[test]
    fn json_object_guard_matches_only_objects() {
        assert!(is_json_object(&json!({ "items": [1, 2, 3] })));
        assert!(is_json_object(&json!({})));
        assert!(!is_json_object(&json!(["not", "object"])));
        assert!(!is_json_object(&JsonValue::Null));
    }

    #[test]
    fn non_null_json_value_round_trips_non_null_values() {
        let value = NonNullJsonValue::new(json!({ "status": "ok" }))
            .expect("object JSON value is non-null");

        let serialized = serde_json::to_value(&value).expect("non-null JSON value serializes");
        assert_eq!(serialized, json!({ "status": "ok" }));

        assert_eq!(
            serde_json::from_value::<NonNullJsonValue>(serialized)
                .expect("non-null JSON value deserializes"),
            value
        );
    }

    #[test]
    fn non_null_json_value_rejects_null_values() {
        assert!(NonNullJsonValue::new(JsonValue::Null).is_err());

        let error = serde_json::from_value::<NonNullJsonValue>(JsonValue::Null)
            .expect_err("null JSON value is rejected");

        assert!(
            error
                .to_string()
                .contains("JSON values cannot be null in this position")
        );
    }
}
