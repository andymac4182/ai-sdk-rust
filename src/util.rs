use crate::json::JsonValue;
use crate::provider_utils::convert_base64_to_bytes;

/// Error returned when text cannot be extracted from a data URL.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataUrlTextError {
    /// The URL did not include the media-type header and comma-separated data payload.
    InvalidFormat,

    /// The data payload was not valid base64.
    Decode,
}

impl std::fmt::Display for DataUrlTextError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidFormat => formatter.write_str("Invalid data URL format"),
            Self::Decode => formatter.write_str("Error decoding data URL"),
        }
    }
}

impl std::error::Error for DataUrlTextError {}

/// Performs a deep-equal comparison of two JSON data values.
///
/// This mirrors upstream `packages/ai` `isDeepEqualData` for Rust's JSON
/// boundary. JavaScript-only cases such as dates, functions, and prototypes do
/// not apply to [`JsonValue`].
pub fn is_deep_equal_data(left: &JsonValue, right: &JsonValue) -> bool {
    left == right
}

/// Converts a base64 data URL into its text payload.
///
/// This mirrors upstream `packages/ai` `getTextFromDataUrl`: the URL is split
/// on the first comma-delimited header and payload segments, the media type must
/// be present in the header, and the payload is decoded with `atob`-style byte
/// to Unicode scalar mapping.
pub fn get_text_from_data_url(data_url: &str) -> Result<String, DataUrlTextError> {
    let mut parts = data_url.split(',');
    let header = parts.next().unwrap_or_default();
    let base64_content = parts.next().ok_or(DataUrlTextError::InvalidFormat)?;

    let header_prefix = header.split(';').next().unwrap_or_default();
    let _media_type = header_prefix
        .split(':')
        .nth(1)
        .ok_or(DataUrlTextError::InvalidFormat)?;

    let bytes = convert_base64_to_bytes(base64_content).map_err(|_| DataUrlTextError::Decode)?;

    Ok(bytes.into_iter().map(char::from).collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{DataUrlTextError, get_text_from_data_url, is_deep_equal_data};

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

    #[test]
    fn get_text_from_data_url_decodes_base64_text() {
        let text = get_text_from_data_url("data:text/plain;base64,SGVsbG8h").unwrap();

        assert_eq!(text, "Hello!");
    }

    #[test]
    fn get_text_from_data_url_accepts_empty_payloads() {
        let text = get_text_from_data_url("data:text/plain;base64,").unwrap();

        assert_eq!(text, "");
    }

    #[test]
    fn get_text_from_data_url_uses_atob_style_byte_mapping() {
        let text = get_text_from_data_url("data:text/plain;base64,/w==").unwrap();

        assert_eq!(text, "ÿ");
    }

    #[test]
    fn get_text_from_data_url_rejects_missing_payload_or_media_type() {
        assert_eq!(
            get_text_from_data_url("data:text/plain;base64").unwrap_err(),
            DataUrlTextError::InvalidFormat
        );
        assert_eq!(
            get_text_from_data_url("text/plain;base64,SGVsbG8=").unwrap_err(),
            DataUrlTextError::InvalidFormat
        );
        assert_eq!(
            DataUrlTextError::InvalidFormat.to_string(),
            "Invalid data URL format"
        );
    }

    #[test]
    fn get_text_from_data_url_rejects_invalid_base64_payloads() {
        let error = get_text_from_data_url("data:text/plain;base64,%").unwrap_err();

        assert_eq!(error, DataUrlTextError::Decode);
        assert_eq!(error.to_string(), "Error decoding data URL");
    }
}
