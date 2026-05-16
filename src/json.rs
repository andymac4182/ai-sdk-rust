use serde_json::{Map, Value};

/// A JSON value that can be passed to or returned from model providers.
///
/// This mirrors the AI SDK's `JSONValue` type while using serde's standard JSON
/// representation for idiomatic Rust serialization and deserialization.
pub type JsonValue = Value;

/// A JSON object keyed by string values.
pub type JsonObject = Map<String, JsonValue>;

/// A JSON array.
pub type JsonArray = Vec<JsonValue>;

#[cfg(test)]
mod tests {
    use super::{JsonArray, JsonObject, JsonValue};
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
}
