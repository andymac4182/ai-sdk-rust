use crate::json::{JsonObject, JsonValue};
use crate::provider_utils::{ParseJsonResult, convert_base64_to_bytes, safe_parse_json};

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
        parse_partial_json, split_array,
    };

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
    fn split_array_chunks_values_like_upstream() {
        assert_eq!(
            split_array(&[1, 2, 3, 4, 5], 2).unwrap(),
            vec![vec![1, 2], vec![3, 4], vec![5]]
        );
        assert_eq!(split_array(&[1, 2, 3], 5).unwrap(), vec![vec![1, 2, 3]]);
        assert_eq!(
            split_array(&[1, 2, 3], 1).unwrap(),
            vec![vec![1], vec![2], vec![3]]
        );

        let empty: Vec<Vec<i32>> = split_array(&[], 2).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn split_array_rejects_zero_and_negative_chunk_sizes() {
        assert_eq!(
            split_array(&[1, 2, 3], 0).unwrap_err(),
            super::SplitArrayError
        );
        assert_eq!(
            split_array(&[1, 2, 3], -1).unwrap_err().to_string(),
            "chunkSize must be greater than 0"
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
