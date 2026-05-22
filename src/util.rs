use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::provider_utils::{
    ParseJsonResult, convert_base64_to_bytes, normalize_headers, safe_parse_json,
};

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

/// Error returned when an array chunk size is invalid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SplitArrayError;

impl std::fmt::Display for SplitArrayError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("chunkSize must be greater than 0")
    }
}

impl std::error::Error for SplitArrayError {}

/// Error returned when a high-level AI SDK utility receives an invalid argument.
#[derive(Clone, Debug, PartialEq)]
pub struct InvalidArgumentError {
    parameter: String,
    value: JsonValue,
    message: String,
}

impl InvalidArgumentError {
    /// Creates an invalid argument error with the upstream high-level message prefix.
    pub fn new(
        parameter: impl Into<String>,
        value: impl Into<JsonValue>,
        message: impl Into<String>,
    ) -> Self {
        let parameter = parameter.into();
        let message = format!(
            "Invalid argument for parameter {}: {}",
            parameter,
            message.into()
        );

        Self {
            parameter,
            value: value.into(),
            message,
        }
    }

    /// Returns the invalid parameter name.
    pub fn parameter(&self) -> &str {
        &self.parameter
    }

    /// Returns the invalid value supplied for the parameter.
    pub fn value(&self) -> &JsonValue {
        &self.value
    }

    /// Returns the human-readable error message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Converts this error into the invalid parameter, value, and message.
    pub fn into_parts(self) -> (String, JsonValue, String) {
        (self.parameter, self.value, self.message)
    }
}

impl std::fmt::Display for InvalidArgumentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InvalidArgumentError {}

/// State returned by [`parse_partial_json`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ParsePartialJsonState {
    /// No JSON text was supplied.
    UndefinedInput,

    /// The supplied JSON text parsed without repair.
    SuccessfulParse,

    /// The supplied JSON text parsed after partial JSON repair.
    RepairedParse,

    /// The supplied JSON text could not be parsed, even after repair.
    FailedParse,
}

/// Result returned by [`parse_partial_json`].
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsePartialJsonResult {
    /// Parsed JSON value when parsing succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<JsonValue>,

    /// Parse state for the attempted partial JSON parse.
    state: ParsePartialJsonState,
}

impl ParsePartialJsonResult {
    /// Creates a partial JSON result with the supplied state and value.
    pub fn new(state: ParsePartialJsonState, value: Option<JsonValue>) -> Self {
        Self { value, state }
    }

    /// Creates an undefined-input result.
    pub fn undefined_input() -> Self {
        Self::new(ParsePartialJsonState::UndefinedInput, None)
    }

    /// Creates a successful parse result.
    pub fn successful_parse(value: impl Into<JsonValue>) -> Self {
        Self::new(ParsePartialJsonState::SuccessfulParse, Some(value.into()))
    }

    /// Creates a repaired parse result.
    pub fn repaired_parse(value: impl Into<JsonValue>) -> Self {
        Self::new(ParsePartialJsonState::RepairedParse, Some(value.into()))
    }

    /// Creates a failed parse result.
    pub fn failed_parse() -> Self {
        Self::new(ParsePartialJsonState::FailedParse, None)
    }

    /// Returns the parsed JSON value when parsing succeeded.
    pub fn value(&self) -> Option<&JsonValue> {
        self.value.as_ref()
    }

    /// Returns the parse state.
    pub fn state(&self) -> ParsePartialJsonState {
        self.state
    }

    /// Converts this result into its value and state.
    pub fn into_parts(self) -> (Option<JsonValue>, ParsePartialJsonState) {
        (self.value, self.state)
    }
}

/// Performs a deep-equal comparison of two JSON data values.
///
/// This mirrors upstream `packages/ai` `isDeepEqualData` for Rust's JSON
/// boundary. JavaScript-only cases such as dates, functions, and prototypes do
/// not apply to [`JsonValue`].
pub fn is_deep_equal_data(left: &JsonValue, right: &JsonValue) -> bool {
    left == right
}

/// Applies default headers without overwriting existing values.
///
/// This mirrors upstream `packages/ai` `prepareHeaders` for Rust header maps:
/// input header names are normalized case-insensitively, and default headers
/// only fill keys that are not already present.
pub fn prepare_headers(headers: Option<Headers>, default_headers: Headers) -> Headers {
    let mut response_headers = normalize_headers(
        headers.map(|headers| headers.into_iter().map(|(key, value)| (key, Some(value)))),
    );

    for (key, value) in default_headers {
        response_headers
            .entry(key.to_ascii_lowercase())
            .or_insert(value);
    }

    response_headers
}

/// Deeply merges two JSON object values.
///
/// This mirrors upstream `packages/ai` `mergeObjects` for Rust's JSON boundary:
/// override fields replace base fields, nested objects are merged recursively,
/// arrays and scalar values are replaced, and prototype-pollution keys from
/// override objects are ignored. JavaScript-only `undefined`, `Date`, and
/// `RegExp` values do not apply to [`JsonValue`].
pub fn merge_objects(base: Option<&JsonValue>, overrides: Option<&JsonValue>) -> Option<JsonValue> {
    match (base, overrides) {
        (None, None) => None,
        (Some(base), None) => Some(base.clone()),
        (None, Some(overrides)) => Some(overrides.clone()),
        (Some(JsonValue::Object(base)), Some(JsonValue::Object(overrides))) => {
            Some(JsonValue::Object(merge_json_object_maps(base, overrides)))
        }
        (_, Some(overrides)) => Some(overrides.clone()),
    }
}

fn merge_json_object_maps(base: &JsonObject, overrides: &JsonObject) -> JsonObject {
    let mut result = base.clone();

    for (key, override_value) in overrides {
        if is_prototype_pollution_key(key) {
            continue;
        }

        let merged_value = match (result.get(key), override_value) {
            (Some(JsonValue::Object(base_object)), JsonValue::Object(override_object)) => {
                JsonValue::Object(merge_json_object_maps(base_object, override_object))
            }
            _ => override_value.clone(),
        };

        result.insert(key.clone(), merged_value);
    }

    result
}

fn is_prototype_pollution_key(key: &str) -> bool {
    matches!(key, "__proto__" | "constructor" | "prototype")
}

/// Splits a slice into cloned chunks of the supplied size.
///
/// This mirrors upstream `packages/ai` `splitArray`: chunk sizes must be
/// greater than zero, an empty input returns an empty chunk list, and the final
/// chunk may be shorter than the requested chunk size.
pub fn split_array<T: Clone>(
    array: &[T],
    chunk_size: isize,
) -> Result<Vec<Vec<T>>, SplitArrayError> {
    if chunk_size <= 0 {
        return Err(SplitArrayError);
    }

    Ok(array
        .chunks(chunk_size as usize)
        .map(<[T]>::to_vec)
        .collect())
}

/// Calculates the cosine similarity between two vectors.
///
/// This mirrors upstream `packages/ai` `cosineSimilarity`: vectors must have
/// equal lengths, empty vectors return 0, and any zero-vector input returns 0.
pub fn cosine_similarity(vector1: &[f64], vector2: &[f64]) -> Result<f64, InvalidArgumentError> {
    if vector1.len() != vector2.len() {
        return Err(InvalidArgumentError::new(
            "vector1,vector2",
            serde_json::json!({
                "vector1Length": vector1.len(),
                "vector2Length": vector2.len(),
            }),
            "Vectors must have the same length",
        ));
    }

    if vector1.is_empty() {
        return Ok(0.0);
    }

    let mut magnitude_squared1 = 0.0;
    let mut magnitude_squared2 = 0.0;
    let mut dot_product = 0.0;

    for (value1, value2) in vector1.iter().zip(vector2) {
        magnitude_squared1 += value1 * value1;
        magnitude_squared2 += value2 * value2;
        dot_product += value1 * value2;
    }

    if magnitude_squared1.classify() == std::num::FpCategory::Zero
        || magnitude_squared2.classify() == std::num::FpCategory::Zero
    {
        return Ok(0.0);
    }

    Ok(dot_product / (magnitude_squared1.sqrt() * magnitude_squared2.sqrt()))
}

/// Parses complete or repairable partial JSON text.
///
/// This mirrors upstream `packages/ai` `parsePartialJson`: missing input returns
/// `undefined-input`, valid JSON returns `successful-parse`, and incomplete JSON
/// is repaired once before returning `repaired-parse` or `failed-parse`.
pub fn parse_partial_json(json_text: Option<&str>) -> ParsePartialJsonResult {
    let Some(json_text) = json_text else {
        return ParsePartialJsonResult::undefined_input();
    };

    if let ParseJsonResult::Success { value, .. } = safe_parse_json(json_text) {
        return ParsePartialJsonResult::successful_parse(value);
    }

    let repaired_json = fix_json(json_text);

    if let ParseJsonResult::Success { value, .. } = safe_parse_json(&repaired_json) {
        return ParsePartialJsonResult::repaired_parse(value);
    }

    ParsePartialJsonResult::failed_parse()
}

/// Finds where `searched_text` is already present or could begin at the end of `text`.
///
/// This mirrors upstream `packages/ai` `getPotentialStartIndex`: a complete
/// match returns its first byte offset, otherwise the function returns the byte
/// offset for the largest suffix of `text` that matches a prefix of
/// `searched_text`. Empty search text and no match return `None`.
pub fn get_potential_start_index(text: &str, searched_text: &str) -> Option<usize> {
    if searched_text.is_empty() {
        return None;
    }

    if let Some(index) = text.find(searched_text) {
        return Some(index);
    }

    text.char_indices()
        .rev()
        .find_map(|(index, _)| searched_text.starts_with(&text[index..]).then_some(index))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PartialJsonState {
    Root,
    Finish,
    InsideString,
    InsideStringEscape,
    InsideLiteral,
    InsideNumber,
    InsideObjectStart,
    InsideObjectKey,
    InsideObjectAfterKey,
    InsideObjectBeforeValue,
    InsideObjectAfterValue,
    InsideObjectAfterComma,
    InsideArrayStart,
    InsideArrayAfterValue,
    InsideArrayAfterComma,
}

/// Repairs a partial JSON prefix into the closest complete JSON text.
///
/// This mirrors upstream `packages/ai` `fixJson`. It is intended for incomplete
/// JSON prefixes produced during streaming; invalid complete JSON is still left
/// to the normal JSON parser.
pub fn fix_json(input: &str) -> String {
    use PartialJsonState::{
        Finish, InsideArrayAfterComma, InsideArrayAfterValue, InsideArrayStart, InsideLiteral,
        InsideNumber, InsideObjectAfterComma, InsideObjectAfterKey, InsideObjectAfterValue,
        InsideObjectBeforeValue, InsideObjectKey, InsideObjectStart, InsideString,
        InsideStringEscape, Root,
    };

    let mut stack = vec![Root];
    let mut last_valid_end = 0usize;
    let mut has_valid_char = false;
    let mut literal_start = None::<usize>;

    fn mark_valid(last_valid_end: &mut usize, has_valid_char: &mut bool, index: usize, char: char) {
        *last_valid_end = index + char.len_utf8();
        *has_valid_char = true;
    }

    fn process_value_start(
        char: char,
        index: usize,
        swap_state: PartialJsonState,
        stack: &mut Vec<PartialJsonState>,
        last_valid_end: &mut usize,
        has_valid_char: &mut bool,
        literal_start: &mut Option<usize>,
    ) {
        use PartialJsonState::{
            InsideArrayStart, InsideLiteral, InsideNumber, InsideObjectStart, InsideString,
        };

        match char {
            '"' => {
                mark_valid(last_valid_end, has_valid_char, index, char);
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideString);
            }
            'f' | 't' | 'n' => {
                mark_valid(last_valid_end, has_valid_char, index, char);
                *literal_start = Some(index);
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideLiteral);
            }
            '-' => {
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideNumber);
            }
            '0'..='9' => {
                mark_valid(last_valid_end, has_valid_char, index, char);
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideNumber);
            }
            '{' => {
                mark_valid(last_valid_end, has_valid_char, index, char);
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideObjectStart);
            }
            '[' => {
                mark_valid(last_valid_end, has_valid_char, index, char);
                stack.pop();
                stack.push(swap_state);
                stack.push(InsideArrayStart);
            }
            _ => {}
        }
    }

    fn process_after_object_value(
        char: char,
        index: usize,
        stack: &mut Vec<PartialJsonState>,
        last_valid_end: &mut usize,
        has_valid_char: &mut bool,
    ) {
        match char {
            ',' => {
                stack.pop();
                stack.push(PartialJsonState::InsideObjectAfterComma);
            }
            '}' => {
                mark_valid(last_valid_end, has_valid_char, index, char);
                stack.pop();
            }
            _ => {}
        }
    }

    fn process_after_array_value(
        char: char,
        index: usize,
        stack: &mut Vec<PartialJsonState>,
        last_valid_end: &mut usize,
        has_valid_char: &mut bool,
    ) {
        match char {
            ',' => {
                stack.pop();
                stack.push(PartialJsonState::InsideArrayAfterComma);
            }
            ']' => {
                mark_valid(last_valid_end, has_valid_char, index, char);
                stack.pop();
            }
            _ => {}
        }
    }

    for (index, char) in input.char_indices() {
        let current_state = *stack.last().unwrap_or(&Finish);

        match current_state {
            Root => process_value_start(
                char,
                index,
                Finish,
                &mut stack,
                &mut last_valid_end,
                &mut has_valid_char,
                &mut literal_start,
            ),
            InsideObjectStart => match char {
                '"' => {
                    stack.pop();
                    stack.push(InsideObjectKey);
                }
                '}' => {
                    mark_valid(&mut last_valid_end, &mut has_valid_char, index, char);
                    stack.pop();
                }
                _ => {}
            },
            InsideObjectAfterComma => {
                if char == '"' {
                    stack.pop();
                    stack.push(InsideObjectKey);
                }
            }
            InsideObjectKey => {
                if char == '"' {
                    stack.pop();
                    stack.push(InsideObjectAfterKey);
                }
            }
            InsideObjectAfterKey => {
                if char == ':' {
                    stack.pop();
                    stack.push(InsideObjectBeforeValue);
                }
            }
            InsideObjectBeforeValue => process_value_start(
                char,
                index,
                InsideObjectAfterValue,
                &mut stack,
                &mut last_valid_end,
                &mut has_valid_char,
                &mut literal_start,
            ),
            InsideObjectAfterValue => process_after_object_value(
                char,
                index,
                &mut stack,
                &mut last_valid_end,
                &mut has_valid_char,
            ),
            InsideString => match char {
                '"' => {
                    stack.pop();
                    mark_valid(&mut last_valid_end, &mut has_valid_char, index, char);
                }
                '\\' => stack.push(InsideStringEscape),
                _ => mark_valid(&mut last_valid_end, &mut has_valid_char, index, char),
            },
            InsideArrayStart => match char {
                ']' => {
                    mark_valid(&mut last_valid_end, &mut has_valid_char, index, char);
                    stack.pop();
                }
                _ => {
                    mark_valid(&mut last_valid_end, &mut has_valid_char, index, char);
                    process_value_start(
                        char,
                        index,
                        InsideArrayAfterValue,
                        &mut stack,
                        &mut last_valid_end,
                        &mut has_valid_char,
                        &mut literal_start,
                    );
                }
            },
            InsideArrayAfterValue => match char {
                ',' => {
                    stack.pop();
                    stack.push(InsideArrayAfterComma);
                }
                ']' => {
                    mark_valid(&mut last_valid_end, &mut has_valid_char, index, char);
                    stack.pop();
                }
                _ => mark_valid(&mut last_valid_end, &mut has_valid_char, index, char),
            },
            InsideArrayAfterComma => process_value_start(
                char,
                index,
                InsideArrayAfterValue,
                &mut stack,
                &mut last_valid_end,
                &mut has_valid_char,
                &mut literal_start,
            ),
            InsideStringEscape => {
                stack.pop();
                mark_valid(&mut last_valid_end, &mut has_valid_char, index, char);
            }
            InsideNumber => match char {
                '0'..='9' => mark_valid(&mut last_valid_end, &mut has_valid_char, index, char),
                'e' | 'E' | '-' | '.' => {}
                ',' => {
                    stack.pop();

                    if stack.last() == Some(&InsideArrayAfterValue) {
                        process_after_array_value(
                            char,
                            index,
                            &mut stack,
                            &mut last_valid_end,
                            &mut has_valid_char,
                        );
                    }

                    if stack.last() == Some(&InsideObjectAfterValue) {
                        process_after_object_value(
                            char,
                            index,
                            &mut stack,
                            &mut last_valid_end,
                            &mut has_valid_char,
                        );
                    }
                }
                '}' => {
                    stack.pop();

                    if stack.last() == Some(&InsideObjectAfterValue) {
                        process_after_object_value(
                            char,
                            index,
                            &mut stack,
                            &mut last_valid_end,
                            &mut has_valid_char,
                        );
                    }
                }
                ']' => {
                    stack.pop();

                    if stack.last() == Some(&InsideArrayAfterValue) {
                        process_after_array_value(
                            char,
                            index,
                            &mut stack,
                            &mut last_valid_end,
                            &mut has_valid_char,
                        );
                    }
                }
                _ => {
                    stack.pop();
                }
            },
            InsideLiteral => {
                let Some(start) = literal_start else {
                    continue;
                };
                let partial_literal = &input[start..index + char.len_utf8()];

                if !"false".starts_with(partial_literal)
                    && !"true".starts_with(partial_literal)
                    && !"null".starts_with(partial_literal)
                {
                    stack.pop();

                    if stack.last() == Some(&InsideObjectAfterValue) {
                        process_after_object_value(
                            char,
                            index,
                            &mut stack,
                            &mut last_valid_end,
                            &mut has_valid_char,
                        );
                    } else if stack.last() == Some(&InsideArrayAfterValue) {
                        process_after_array_value(
                            char,
                            index,
                            &mut stack,
                            &mut last_valid_end,
                            &mut has_valid_char,
                        );
                    }
                } else {
                    mark_valid(&mut last_valid_end, &mut has_valid_char, index, char);
                }
            }
            Finish => {}
        }
    }

    let mut result = if has_valid_char {
        input[..last_valid_end].to_owned()
    } else {
        String::new()
    };

    for state in stack.iter().rev() {
        match state {
            InsideString => result.push('"'),
            InsideObjectKey
            | InsideObjectAfterKey
            | InsideObjectAfterComma
            | InsideObjectStart
            | InsideObjectBeforeValue
            | InsideObjectAfterValue => result.push('}'),
            InsideArrayStart | InsideArrayAfterComma | InsideArrayAfterValue => result.push(']'),
            InsideLiteral => {
                let Some(start) = literal_start else {
                    continue;
                };
                let partial_literal = &input[start..];

                if "true".starts_with(partial_literal) {
                    result.push_str(&"true"[partial_literal.len()..]);
                } else if "false".starts_with(partial_literal) {
                    result.push_str(&"false"[partial_literal.len()..]);
                } else if "null".starts_with(partial_literal) {
                    result.push_str(&"null"[partial_literal.len()..]);
                }
            }
            _ => {}
        }
    }

    result
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

    use super::{
        DataUrlTextError, InvalidArgumentError, cosine_similarity, fix_json,
        get_potential_start_index, get_text_from_data_url, is_deep_equal_data, merge_objects,
        parse_partial_json, prepare_headers, split_array,
    };
    use crate::headers::Headers;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-12,
            "expected {actual} to be close to {expected}"
        );
    }

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
    fn is_deep_equal_data_should_check_if_two_primitives_are_equal() {
        assert!(is_deep_equal_data(&json!(1), &json!(1)));
        assert!(!is_deep_equal_data(&json!(1), &json!(2)));
    }

    #[test]
    fn is_deep_equal_data_should_return_false_for_different_types() {
        assert!(!is_deep_equal_data(&json!({ "a": 1 }), &json!(1)));
    }

    #[test]
    fn is_deep_equal_data_should_return_false_for_null_values_compared_with_objects() {
        assert!(!is_deep_equal_data(&json!({ "a": 1 }), &json!(null)));
    }

    #[test]
    fn is_deep_equal_data_should_identify_two_equal_objects() {
        assert!(is_deep_equal_data(
            &json!({ "a": 1, "b": 2 }),
            &json!({ "a": 1, "b": 2 })
        ));
    }

    #[test]
    fn is_deep_equal_data_should_identify_two_objects_with_different_values() {
        assert!(!is_deep_equal_data(
            &json!({ "a": 1, "b": 2 }),
            &json!({ "a": 1, "b": 3 })
        ));
    }

    #[test]
    fn is_deep_equal_data_should_identify_two_objects_with_different_number_of_keys() {
        assert!(!is_deep_equal_data(
            &json!({ "a": 1, "b": 2 }),
            &json!({ "a": 1, "b": 2, "c": 3 })
        ));
    }

    #[test]
    fn is_deep_equal_data_should_handle_nested_objects() {
        assert!(is_deep_equal_data(
            &json!({ "a": { "c": 1 }, "b": 2 }),
            &json!({ "a": { "c": 1 }, "b": 2 })
        ));
    }

    #[test]
    fn is_deep_equal_data_should_detect_inequality_in_nested_objects() {
        assert!(!is_deep_equal_data(
            &json!({ "a": { "c": 1 }, "b": 2 }),
            &json!({ "a": { "c": 2 }, "b": 2 })
        ));
    }

    #[test]
    fn is_deep_equal_data_should_compare_arrays_correctly() {
        assert!(is_deep_equal_data(&json!([1, 2, 3]), &json!([1, 2, 3])));
        assert!(!is_deep_equal_data(&json!([1, 2, 3]), &json!([1, 2, 4])));
    }

    #[test]
    fn is_deep_equal_data_should_return_false_for_null_comparison_with_object() {
        assert!(!is_deep_equal_data(&json!({ "a": 1 }), &json!(null)));
    }

    #[test]
    fn is_deep_equal_data_should_distinguish_between_array_and_object_with_same_enumerable_properties()
     {
        assert!(!is_deep_equal_data(
            &json!({ "0": "one", "1": "two", "length": 2 }),
            &json!(["one", "two"])
        ));
    }

    #[test]
    fn prepare_headers_should_set_content_type_header_if_not_present() {
        let headers = prepare_headers(Some(Headers::new()), content_type_default_headers());

        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    #[test]
    fn prepare_headers_should_not_overwrite_existing_content_type_header() {
        let headers = prepare_headers(
            Some(Headers::from([(
                "Content-Type".to_string(),
                "text/html".to_string(),
            )])),
            content_type_default_headers(),
        );

        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("text/html")
        );
    }

    #[test]
    fn prepare_headers_should_handle_undefined_init() {
        let headers = prepare_headers(None, content_type_default_headers());

        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    #[test]
    fn prepare_headers_should_handle_init_headers_as_headers_object() {
        let headers = prepare_headers(
            Some(Headers::from([("init".to_string(), "foo".to_string())])),
            content_type_default_headers(),
        );

        assert_eq!(headers.get("init").map(String::as_str), Some("foo"));
        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    #[test]
    fn prepare_headers_should_handle_response_object_headers() {
        let headers = prepare_headers(
            Some(Headers::from([
                ("init".to_string(), "foo".to_string()),
                ("extra".to_string(), "bar".to_string()),
            ])),
            content_type_default_headers(),
        );

        assert_eq!(headers.get("init").map(String::as_str), Some("foo"));
        assert_eq!(headers.get("extra").map(String::as_str), Some("bar"));
        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    fn content_type_default_headers() -> Headers {
        Headers::from([("content-type".to_string(), "application/json".to_string())])
    }

    #[test]
    fn merge_objects_should_merge_two_flat_objects() {
        let target = json!({ "a": 1, "b": 2 });
        let source = json!({ "b": 3, "c": 4 });

        assert_eq!(
            merge_objects(Some(&target), Some(&source)),
            Some(json!({ "a": 1, "b": 3, "c": 4 }))
        );
        assert_eq!(target, json!({ "a": 1, "b": 2 }));
        assert_eq!(source, json!({ "b": 3, "c": 4 }));
    }

    #[test]
    fn merge_objects_should_deeply_merge_nested_objects() {
        let target = json!({ "a": 1, "b": { "c": 2, "d": 3 } });
        let source = json!({ "b": { "c": 4, "e": 5 } });

        assert_eq!(
            merge_objects(Some(&target), Some(&source)),
            Some(json!({ "a": 1, "b": { "c": 4, "d": 3, "e": 5 } }))
        );
    }

    #[test]
    fn merge_objects_should_replace_arrays_instead_of_merging_them() {
        let target = json!({ "a": [1, 2, 3], "b": 2 });
        let source = json!({ "a": [4, 5] });

        assert_eq!(
            merge_objects(Some(&target), Some(&source)),
            Some(json!({ "a": [4, 5], "b": 2 }))
        );
    }

    #[test]
    fn merge_objects_should_handle_null_values() {
        let target = json!({ "a": 1, "b": null });
        let source = json!({ "a": null, "b": 2 });

        assert_eq!(
            merge_objects(Some(&target), Some(&source)),
            Some(json!({ "a": null, "b": 2 }))
        );
    }

    #[test]
    fn merge_objects_should_handle_complex_nested_structures() {
        let target = json!({
            "a": 1,
            "b": {
                "c": [1, 2, 3],
                "d": {
                    "e": 4,
                    "f": 5
                }
            }
        });
        let source = json!({
            "b": {
                "c": [4, 5],
                "d": {
                    "f": 6,
                    "g": 7
                }
            },
            "h": 8
        });

        assert_eq!(
            merge_objects(Some(&target), Some(&source)),
            Some(json!({
                "a": 1,
                "b": {
                    "c": [4, 5],
                    "d": {
                        "e": 4,
                        "f": 6,
                        "g": 7
                    }
                },
                "h": 8
            }))
        );
    }

    #[test]
    fn merge_objects_should_handle_empty_objects() {
        let empty = json!({});
        let object = json!({ "a": 1 });

        assert_eq!(
            merge_objects(Some(&empty), Some(&object)),
            Some(json!({ "a": 1 }))
        );
        assert_eq!(
            merge_objects(Some(&object), Some(&empty)),
            Some(json!({ "a": 1 }))
        );
    }

    #[test]
    fn merge_objects_should_handle_undefined_inputs() {
        let target = json!({ "a": 1 });
        let source = json!({ "b": 2 });

        assert_eq!(merge_objects(None, None), None);
        assert_eq!(merge_objects(Some(&target), None), Some(json!({ "a": 1 })));
        assert_eq!(merge_objects(None, Some(&source)), Some(json!({ "b": 2 })));
    }

    #[test]
    fn merge_objects_should_not_pollute_object_prototype_via_proto() {
        let malicious = json!({ "__proto__": { "polluted": true } });

        assert_eq!(
            merge_objects(Some(&json!({})), Some(&malicious)),
            Some(json!({}))
        );
    }

    #[test]
    fn merge_objects_should_ignore_proto_constructor_and_prototype_keys() {
        let malicious = json!({
            "__proto__": { "a": 1 },
            "constructor": { "prototype": { "b": 2 } },
            "prototype": { "c": 3 },
            "safe": "value"
        });

        assert_eq!(
            merge_objects(Some(&json!({ "existing": "ok" })), Some(&malicious)),
            Some(json!({ "existing": "ok", "safe": "value" }))
        );
    }

    #[test]
    fn merge_objects_should_ignore_dangerous_keys_nested_in_mergeable_objects() {
        let base = json!({ "metadata": { "user": "alice" } });
        let malicious = json!({
            "metadata": {
                "__proto__": { "polluted": true },
                "role": "admin"
            }
        });

        assert_eq!(
            merge_objects(Some(&base), Some(&malicious)),
            Some(json!({ "metadata": { "user": "alice", "role": "admin" } }))
        );
    }

    #[test]
    fn merge_objects_deeply_merges_json_objects() {
        let base = json!({
            "a": 1,
            "b": {
                "c": [1, 2, 3],
                "d": {
                    "e": 4,
                    "f": 5,
                },
            },
        });
        let overrides = json!({
            "b": {
                "c": [4, 5],
                "d": {
                    "f": 6,
                    "g": 7,
                },
            },
            "h": 8,
        });

        assert_eq!(
            merge_objects(Some(&base), Some(&overrides)),
            Some(json!({
                "a": 1,
                "b": {
                    "c": [4, 5],
                    "d": {
                        "e": 4,
                        "f": 6,
                        "g": 7,
                    },
                },
                "h": 8,
            }))
        );
        assert_eq!(base["b"]["c"], json!([1, 2, 3]));
        assert_eq!(overrides["b"]["c"], json!([4, 5]));
    }

    #[test]
    fn merge_objects_handles_missing_inputs_and_replacements() {
        let base = json!({ "a": 1, "b": null, "c": [1, 2, 3] });
        let overrides = json!({ "a": null, "b": 2, "c": [4, 5] });

        assert_eq!(merge_objects(None, None), None);
        assert_eq!(merge_objects(Some(&base), None), Some(base.clone()));
        assert_eq!(
            merge_objects(None, Some(&overrides)),
            Some(overrides.clone())
        );
        assert_eq!(
            merge_objects(Some(&base), Some(&overrides)),
            Some(json!({ "a": null, "b": 2, "c": [4, 5] }))
        );
    }

    #[test]
    fn merge_objects_ignores_dangerous_override_keys() {
        let base = json!({
            "existing": "ok",
            "metadata": { "user": "alice" },
        });
        let malicious = serde_json::from_str::<serde_json::Value>(
            r#"{
                "__proto__": { "a": 1 },
                "constructor": { "prototype": { "b": 2 } },
                "prototype": { "c": 3 },
                "safe": "value",
                "metadata": {
                    "__proto__": { "polluted": true },
                    "role": "admin"
                }
            }"#,
        )
        .unwrap();

        assert_eq!(
            merge_objects(Some(&base), Some(&malicious)),
            Some(json!({
                "existing": "ok",
                "safe": "value",
                "metadata": {
                    "user": "alice",
                    "role": "admin",
                },
            }))
        );
    }

    #[test]
    fn split_array_should_split_an_array_into_chunks_of_the_specified_size() {
        assert_eq!(
            split_array(&[1, 2, 3, 4, 5], 2).unwrap(),
            vec![vec![1, 2], vec![3, 4], vec![5]]
        );
    }

    #[test]
    fn split_array_should_return_empty_array_when_input_array_is_empty() {
        let empty: Vec<Vec<i32>> = split_array(&[], 2).unwrap();

        assert!(empty.is_empty());
    }

    #[test]
    fn split_array_should_return_original_array_when_chunk_size_is_greater_than_array_length() {
        assert_eq!(split_array(&[1, 2, 3], 5).unwrap(), vec![vec![1, 2, 3]]);
    }

    #[test]
    fn split_array_should_return_original_array_when_chunk_size_is_equal_to_array_length() {
        assert_eq!(split_array(&[1, 2, 3], 3).unwrap(), vec![vec![1, 2, 3]]);
    }

    #[test]
    fn split_array_should_handle_chunk_size_of_one_correctly() {
        assert_eq!(
            split_array(&[1, 2, 3], 1).unwrap(),
            vec![vec![1], vec![2], vec![3]]
        );
    }

    #[test]
    fn split_array_should_throw_error_for_chunk_size_of_zero() {
        assert_eq!(
            split_array(&[1, 2, 3], 0).unwrap_err(),
            super::SplitArrayError
        );
    }

    #[test]
    fn split_array_should_throw_error_for_negative_chunk_size() {
        assert_eq!(
            split_array(&[1, 2, 3], -1).unwrap_err().to_string(),
            "chunkSize must be greater than 0"
        );
    }

    #[test]
    fn split_array_should_handle_non_integer_chunk_size_by_flooring_the_size() {
        let size = 2.5_f64.floor() as isize;

        assert_eq!(
            split_array(&[1, 2, 3, 4, 5], size).unwrap(),
            vec![vec![1, 2], vec![3, 4], vec![5]]
        );
    }

    #[test]
    fn get_potential_start_index_matches_complete_or_partial_prefixes() {
        assert_eq!(get_potential_start_index("1234567890", ""), None);
        assert_eq!(get_potential_start_index("1234567890", "a"), None);
        assert_eq!(
            get_potential_start_index("1234567890", "1234567890"),
            Some(0)
        );
        assert_eq!(get_potential_start_index("1234567890", "0123"), Some(9));
        assert_eq!(get_potential_start_index("1234567890", "90123"), Some(8));
        assert_eq!(get_potential_start_index("1234567890", "890123"), Some(7));
    }

    #[test]
    fn fix_json_repairs_incomplete_literals_numbers_and_strings() {
        assert_eq!(fix_json(""), "");
        assert_eq!(fix_json("nul"), "null");
        assert_eq!(fix_json("t"), "true");
        assert_eq!(fix_json("fals"), "false");
        assert_eq!(fix_json("12."), "12");
        assert_eq!(fix_json("2.5e-"), "2.5");
        assert_eq!(fix_json("2.5E3"), "2.5E3");
        assert_eq!(fix_json("-"), "");
        assert_eq!(fix_json(r#""abc"#), r#""abc""#);
        assert_eq!(fix_json(r#""value with \"#), r#""value with ""#);
    }

    #[test]
    fn fix_json_repairs_incomplete_arrays_and_objects() {
        assert_eq!(fix_json("["), "[]");
        assert_eq!(fix_json("[[1], [2"), "[[1], [2]]");
        assert_eq!(fix_json("[[false], [nu"), "[[false], [null]]");
        assert_eq!(fix_json("[1, "), "[1]");
        assert_eq!(fix_json(r#"{"key":"#), "{}");
        assert_eq!(fix_json(r#"{"k1": 1, "k2"#), r#"{"k1": 1}"#);
        assert_eq!(fix_json(r#"{"key": "value"  "#), r#"{"key": "value"}"#);
        assert_eq!(
            fix_json(r#"{"a": {"b": ["c", {"d": "e","#),
            r#"{"a": {"b": ["c", {"d": "e"}]}}"#
        );
        assert_eq!(
            fix_json(r#"{"type":"div","children":[{"type":"Card","props":{}"#),
            r#"{"type":"div","children":[{"type":"Card","props":{}}]}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_empty_input() {
        assert_eq!(fix_json(""), "");
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_null() {
        assert_eq!(fix_json("nul"), "null");
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_true() {
        assert_eq!(fix_json("t"), "true");
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_false() {
        assert_eq!(fix_json("fals"), "false");
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_numbers() {
        assert_eq!(fix_json("12."), "12");
    }

    #[test]
    fn fix_json_upstream_handles_numbers_with_dot() {
        assert_eq!(fix_json("12.2"), "12.2");
    }

    #[test]
    fn fix_json_upstream_handles_negative_numbers() {
        assert_eq!(fix_json("-12"), "-12");
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_negative_numbers() {
        assert_eq!(fix_json("-"), "");
    }

    #[test]
    fn fix_json_upstream_handles_e_notation_numbers() {
        assert_eq!(fix_json("2.5e"), "2.5");
        assert_eq!(fix_json("2.5e-"), "2.5");
        assert_eq!(fix_json("2.5e3"), "2.5e3");
        assert_eq!(fix_json("-2.5e3"), "-2.5e3");
    }

    #[test]
    fn fix_json_upstream_handles_uppercase_e_notation_numbers() {
        assert_eq!(fix_json("2.5E"), "2.5");
        assert_eq!(fix_json("2.5E-"), "2.5");
        assert_eq!(fix_json("2.5E3"), "2.5E3");
        assert_eq!(fix_json("-2.5E3"), "-2.5E3");
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_exponent_numbers() {
        assert_eq!(fix_json("12.e"), "12");
        assert_eq!(fix_json("12.34e"), "12.34");
        assert_eq!(fix_json("5e"), "5");
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_strings() {
        assert_eq!(fix_json(r#""abc"#), r#""abc""#);
    }

    #[test]
    fn fix_json_upstream_handles_escape_sequences() {
        assert_eq!(
            fix_json(r#""value with \"quoted\" text and \\ escape"#),
            r#""value with \"quoted\" text and \\ escape""#
        );
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_escape_sequences() {
        assert_eq!(fix_json(r#""value with \"#), r#""value with ""#);
    }

    #[test]
    fn fix_json_upstream_handles_unicode_characters() {
        assert_eq!(
            fix_json(r#""value with unicode <""#),
            r#""value with unicode <""#
        );
    }

    #[test]
    fn fix_json_upstream_handles_incomplete_array() {
        assert_eq!(fix_json("["), "[]");
    }

    #[test]
    fn fix_json_upstream_handles_closing_bracket_after_number_in_array() {
        assert_eq!(fix_json("[[1], [2"), "[[1], [2]]");
    }

    #[test]
    fn fix_json_upstream_handles_closing_bracket_after_string_in_array() {
        assert_eq!(fix_json(r#"[["1"], ["2"#), r#"[["1"], ["2"]]"#);
    }

    #[test]
    fn fix_json_upstream_handles_closing_bracket_after_literal_in_array() {
        assert_eq!(fix_json("[[false], [nu"), "[[false], [null]]");
    }

    #[test]
    fn fix_json_upstream_handles_closing_bracket_after_array_in_array() {
        assert_eq!(fix_json("[[[]], [[]"), "[[[]], [[]]]");
    }

    #[test]
    fn fix_json_upstream_handles_closing_bracket_after_object_in_array() {
        assert_eq!(fix_json("[[{}], [{"), "[[{}], [{}]]");
    }

    #[test]
    fn fix_json_upstream_handles_trailing_comma() {
        assert_eq!(fix_json("[1, "), "[1]");
    }

    #[test]
    fn fix_json_upstream_handles_closing_array() {
        assert_eq!(fix_json("[[], 123"), "[[], 123]");
    }

    #[test]
    fn fix_json_upstream_handles_keys_without_values() {
        assert_eq!(fix_json(r#"{"key":"#), "{}");
    }

    #[test]
    fn fix_json_upstream_handles_closing_brace_after_number_in_object() {
        assert_eq!(
            fix_json(r#"{"a": {"b": 1}, "c": {"d": 2"#),
            r#"{"a": {"b": 1}, "c": {"d": 2}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_closing_brace_after_string_in_object() {
        assert_eq!(
            fix_json(r#"{"a": {"b": "1"}, "c": {"d": 2"#),
            r#"{"a": {"b": "1"}, "c": {"d": 2}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_closing_brace_after_literal_in_object() {
        assert_eq!(
            fix_json(r#"{"a": {"b": false}, "c": {"d": 2"#),
            r#"{"a": {"b": false}, "c": {"d": 2}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_closing_brace_after_array_in_object() {
        assert_eq!(
            fix_json(r#"{"a": {"b": []}, "c": {"d": 2"#),
            r#"{"a": {"b": []}, "c": {"d": 2}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_closing_brace_after_object_in_object() {
        assert_eq!(
            fix_json(r#"{"a": {"b": {}}, "c": {"d": 2"#),
            r#"{"a": {"b": {}}, "c": {"d": 2}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_partial_keys_first_key() {
        assert_eq!(fix_json(r#"{"ke"#), "{}");
    }

    #[test]
    fn fix_json_upstream_handles_partial_keys_second_key() {
        assert_eq!(fix_json(r#"{"k1": 1, "k2"#), r#"{"k1": 1}"#);
    }

    #[test]
    fn fix_json_upstream_handles_partial_keys_with_colon_second_key() {
        assert_eq!(fix_json(r#"{"k1": 1, "k2":"#), r#"{"k1": 1}"#);
    }

    #[test]
    fn fix_json_upstream_handles_trailing_whitespace() {
        assert_eq!(fix_json(r#"{"key": "value"  "#), r#"{"key": "value"}"#);
    }

    #[test]
    fn fix_json_upstream_handles_closing_after_empty_object() {
        assert_eq!(fix_json(r#"{"a": {"b": {}"#), r#"{"a": {"b": {}}}"#);
    }

    #[test]
    fn fix_json_upstream_handles_nested_arrays_with_numbers() {
        assert_eq!(fix_json("[1, [2, 3, ["), "[1, [2, 3, []]]");
    }

    #[test]
    fn fix_json_upstream_handles_nested_arrays_with_literals() {
        assert_eq!(fix_json("[false, [true, ["), "[false, [true, []]]");
    }

    #[test]
    fn fix_json_upstream_handles_nested_objects() {
        assert_eq!(fix_json(r#"{"key": {"subKey":"#), r#"{"key": {}}"#);
    }

    #[test]
    fn fix_json_upstream_handles_nested_objects_with_numbers() {
        assert_eq!(
            fix_json(r#"{"key": 123, "key2": {"subKey":"#),
            r#"{"key": 123, "key2": {}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_nested_objects_with_literals() {
        assert_eq!(
            fix_json(r#"{"key": null, "key2": {"subKey":"#),
            r#"{"key": null, "key2": {}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_arrays_within_objects() {
        assert_eq!(fix_json(r#"{"key": [1, 2, {"#), r#"{"key": [1, 2, {}]}"#);
    }

    #[test]
    fn fix_json_upstream_handles_objects_within_arrays() {
        assert_eq!(
            fix_json(r#"[1, 2, {"key": "value","#),
            r#"[1, 2, {"key": "value"}]"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_nested_arrays_and_objects() {
        assert_eq!(
            fix_json(r#"{"a": {"b": ["c", {"d": "e","#),
            r#"{"a": {"b": ["c", {"d": "e"}]}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_deeply_nested_objects() {
        assert_eq!(
            fix_json(r#"{"a": {"b": {"c": {"d":"#),
            r#"{"a": {"b": {"c": {}}}}"#
        );
    }

    #[test]
    fn fix_json_upstream_handles_potential_nested_arrays_or_objects() {
        assert_eq!(fix_json(r#"{"a": 1, "b": ["#), r#"{"a": 1, "b": []}"#);
        assert_eq!(fix_json(r#"{"a": 1, "b": {"#), r#"{"a": 1, "b": {}}"#);
        assert_eq!(fix_json(r#"{"a": 1, "b": ""#), r#"{"a": 1, "b": ""}"#);
    }

    #[test]
    fn fix_json_upstream_handles_complex_nesting_1() {
        assert_eq!(
            fix_json(
                [
                    "{",
                    r#"  "a": ["#,
                    "    {",
                    r#"      "a1": "v1","#,
                    r#"      "a2": "v2","#,
                    r#"      "a3": "v3""#,
                    "    }",
                    "  ],",
                    r#"  "b": ["#,
                    "    {",
                    r#"      "b1": "n"#,
                ]
                .join("\n")
                .as_str()
            ),
            [
                "{",
                r#"  "a": ["#,
                "    {",
                r#"      "a1": "v1","#,
                r#"      "a2": "v2","#,
                r#"      "a3": "v3""#,
                "    }",
                "  ],",
                r#"  "b": ["#,
                "    {",
                r#"      "b1": "n"}]}"#,
            ]
            .join("\n")
        );
    }

    #[test]
    fn fix_json_upstream_handles_empty_objects_inside_nested_objects_and_arrays() {
        assert_eq!(
            fix_json(r#"{"type":"div","children":[{"type":"Card","props":{}"#),
            r#"{"type":"div","children":[{"type":"Card","props":{}}]}"#
        );
    }

    #[test]
    fn invalid_argument_error_retains_parameter_value_and_message() {
        let error = InvalidArgumentError::new("messages", json!(null), "messages are required");

        assert_eq!(error.parameter(), "messages");
        assert_eq!(error.value(), &json!(null));
        assert_eq!(
            error.message(),
            "Invalid argument for parameter messages: messages are required"
        );
        assert_eq!(error.to_string(), error.message());

        let (parameter, value, message) = error.into_parts();
        assert_eq!(parameter, "messages");
        assert_eq!(value, json!(null));
        assert_eq!(
            message,
            "Invalid argument for parameter messages: messages are required"
        );
    }

    #[test]
    fn cosine_similarity_calculates_similarity() {
        let result = cosine_similarity(&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0]).unwrap();

        assert_close(result, 0.974_631_846_197_076_2);
    }

    #[test]
    fn cosine_similarity_calculates_negative_similarity() {
        let result = cosine_similarity(&[1.0, 0.0], &[-1.0, 0.0]).unwrap();

        assert_close(result, -1.0);
    }

    #[test]
    fn cosine_similarity_rejects_mismatched_lengths() {
        let error = cosine_similarity(&[1.0, 2.0, 3.0], &[4.0, 5.0]).unwrap_err();

        assert_eq!(error.parameter(), "vector1,vector2");
        assert_eq!(
            error.value(),
            &json!({ "vector1Length": 3, "vector2Length": 2 })
        );
        assert_eq!(
            error.to_string(),
            "Invalid argument for parameter vector1,vector2: Vectors must have the same length"
        );
    }

    #[test]
    fn cosine_similarity_returns_zero_for_empty_or_zero_vectors() {
        assert_eq!(cosine_similarity(&[], &[]).unwrap(), 0.0);
        assert_eq!(
            cosine_similarity(&[0.0, 1.0, 2.0], &[0.0, 0.0, 0.0]).unwrap(),
            0.0
        );
        assert_eq!(
            cosine_similarity(&[0.0, 0.0, 0.0], &[0.0, 1.0, 2.0]).unwrap(),
            0.0
        );
    }

    #[test]
    fn cosine_similarity_handles_very_small_magnitudes() {
        let result = cosine_similarity(&[1e-10, 0.0, 0.0], &[2e-10, 0.0, 0.0]).unwrap();
        let negative = cosine_similarity(&[1e-10, 0.0, 0.0], &[-1e-10, 0.0, 0.0]).unwrap();

        assert_close(result, 1.0);
        assert_close(negative, -1.0);
    }

    #[test]
    fn parse_partial_json_returns_undefined_input_for_missing_text() {
        let result = parse_partial_json(None);

        assert_eq!(result.value(), None);
        assert_eq!(result.state(), super::ParsePartialJsonState::UndefinedInput);
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({ "state": "undefined-input" })
        );
    }

    #[test]
    fn parse_partial_json_returns_successful_parse_for_valid_json() {
        let result = parse_partial_json(Some(r#"{"foo":"bar","items":[1,true,null]}"#));

        assert_eq!(
            result.value(),
            Some(&json!({
                "foo": "bar",
                "items": [1, true, null],
            }))
        );
        assert_eq!(
            result.state(),
            super::ParsePartialJsonState::SuccessfulParse
        );
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({
                "value": {
                    "foo": "bar",
                    "items": [1, true, null],
                },
                "state": "successful-parse",
            })
        );
    }

    #[test]
    fn parse_partial_json_repairs_incomplete_literals_strings_arrays_and_objects() {
        assert_eq!(
            parse_partial_json(Some("nul")),
            super::ParsePartialJsonResult::repaired_parse(json!(null))
        );
        assert_eq!(
            parse_partial_json(Some(r#""abc"#)),
            super::ParsePartialJsonResult::repaired_parse(json!("abc"))
        );
        assert_eq!(
            parse_partial_json(Some("[[false], [nu")),
            super::ParsePartialJsonResult::repaired_parse(json!([[false], [null]]))
        );
        assert_eq!(
            parse_partial_json(Some(r#"{"k1": 1, "k2":"#)),
            super::ParsePartialJsonResult::repaired_parse(json!({ "k1": 1 }))
        );
    }

    #[test]
    fn parse_partial_json_trims_incomplete_numbers_before_repair_parse() {
        assert_eq!(
            parse_partial_json(Some("12.")),
            super::ParsePartialJsonResult::repaired_parse(json!(12))
        );
        assert_eq!(
            parse_partial_json(Some("2.5e-")),
            super::ParsePartialJsonResult::repaired_parse(json!(2.5))
        );
        assert_eq!(
            parse_partial_json(Some("-")),
            super::ParsePartialJsonResult::failed_parse()
        );
    }

    #[test]
    fn parse_partial_json_returns_failed_parse_when_repair_cannot_make_valid_json() {
        let result = parse_partial_json(Some("not json"));

        assert_eq!(result.value(), None);
        assert_eq!(result.state(), super::ParsePartialJsonState::FailedParse);
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({ "state": "failed-parse" })
        );
    }

    #[test]
    fn parse_partial_json_deserializes_state_shape() {
        let result: super::ParsePartialJsonResult = serde_json::from_value(json!({
            "value": { "partial": true },
            "state": "repaired-parse",
        }))
        .unwrap();

        assert_eq!(result.value(), Some(&json!({ "partial": true })));
        assert_eq!(result.state(), super::ParsePartialJsonState::RepairedParse);
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
