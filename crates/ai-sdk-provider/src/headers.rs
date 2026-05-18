use std::collections::BTreeMap;

/// HTTP-style headers attached to provider calls or responses.
///
/// This mirrors the AI SDK's shared v4 `Record<string, string>` header shape
/// while using an ordered map for deterministic Rust behavior.
pub type Headers = BTreeMap<String, String>;

#[cfg(test)]
mod tests {
    use super::Headers;
    use serde_json::json;

    #[test]
    fn headers_serialize_as_string_map() {
        let headers: Headers = serde_json::from_value(json!({
            "x-request-id": "req_123",
            "content-type": "application/json"
        }))
        .expect("headers deserialize");

        assert_eq!(
            serde_json::to_value(headers).expect("headers serialize"),
            json!({
                "content-type": "application/json",
                "x-request-id": "req_123"
            })
        );
    }
}
