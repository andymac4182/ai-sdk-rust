use crate::json::JsonValue;

/// Performs a deep-equal comparison of two JSON data values.
///
/// This mirrors upstream `packages/ai` `isDeepEqualData` for Rust's JSON
/// boundary. JavaScript-only cases such as dates, functions, and prototypes do
/// not apply to [`JsonValue`].
pub fn is_deep_equal_data(left: &JsonValue, right: &JsonValue) -> bool {
    left == right
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::is_deep_equal_data;

    #[test]
    fn is_deep_equal_data_compares_primitives() {
        assert!(is_deep_equal_data(&json!(1), &json!(1)));
        assert!(!is_deep_equal_data(&json!(1), &json!(2)));
        assert!(!is_deep_equal_data(&json!(null), &json!({ "a": 1 })));
    }

    #[test]
    fn is_deep_equal_data_compares_nested_objects() {
        assert!(is_deep_equal_data(
            &json!({ "a": { "c": 1 }, "b": [true, null] }),
            &json!({ "b": [true, null], "a": { "c": 1 } })
        ));

        assert!(!is_deep_equal_data(
            &json!({ "a": { "c": 1 }, "b": 2 }),
            &json!({ "a": { "c": 2 }, "b": 2 })
        ));
    }

    #[test]
    fn is_deep_equal_data_compares_arrays_by_order_and_length() {
        assert!(is_deep_equal_data(
            &json!([1, { "a": "b" }, 3]),
            &json!([1, { "a": "b" }, 3])
        ));
        assert!(!is_deep_equal_data(&json!([1, 2, 3]), &json!([1, 3, 2])));
        assert!(!is_deep_equal_data(&json!([1, 2]), &json!([1, 2, 3])));
    }

    #[test]
    fn is_deep_equal_data_distinguishes_objects_from_arrays() {
        assert!(!is_deep_equal_data(
            &json!({ "0": "one", "1": "two", "length": 2 }),
            &json!(["one", "two"])
        ));
    }
}
