use crate::chat_transport::RequestCredentials;
use crate::headers::Headers;
use crate::json::JsonValue;
use crate::provider_utils::{ParseJsonResult, normalize_headers, safe_parse_json};
use crate::util::{ParsePartialJsonState, is_deep_equal_data, parse_partial_json};

/// Constructor options for the Rust equivalent of upstream object UI requests.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectTransportOptions {
    pub api: String,
    pub credentials: Option<RequestCredentials>,
    pub headers: Headers,
}

impl ObjectTransportOptions {
    pub fn new(api: impl Into<String>) -> Self {
        Self {
            api: api.into(),
            credentials: None,
            headers: Headers::new(),
        }
    }

    pub fn with_credentials(mut self, credentials: RequestCredentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

/// Per-request options for object UI requests.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectRequestOptions {
    /// Additional HTTP headers passed to the object API endpoint.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,
}

impl ObjectRequestOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

/// HTTP method used by deterministic object transport request builders.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ObjectTransportMethod {
    Post,
}

/// Deterministic HTTP request produced by [`ObjectTransport`].
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectTransportRequest {
    pub method: ObjectTransportMethod,
    pub api: String,

    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,

    pub body: JsonValue,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<RequestCredentials>,
}

/// One changed partial object observed while processing an object text stream.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectStreamUpdate {
    pub object: JsonValue,
    pub state: ParsePartialJsonState,
}

impl ObjectStreamUpdate {
    pub fn new(object: impl Into<JsonValue>, state: ParsePartialJsonState) -> Self {
        Self {
            object: object.into(),
            state,
        }
    }
}

/// Collected result from processing an object text stream.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectStreamResult {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updates: Vec<ObjectStreamUpdate>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object: Option<JsonValue>,
}

impl ObjectStreamResult {
    pub fn new(updates: Vec<ObjectStreamUpdate>, object: Option<JsonValue>) -> Self {
        Self { updates, object }
    }
}

/// Deterministic Rust equivalent of upstream `experimental_useObject` request
/// and response-stream behavior.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectTransport {
    options: ObjectTransportOptions,
}

impl ObjectTransport {
    pub fn new(api: impl Into<String>) -> Self {
        Self::with_options(ObjectTransportOptions::new(api))
    }

    pub fn with_options(options: ObjectTransportOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &ObjectTransportOptions {
        &self.options
    }

    pub fn build_object_request(
        &self,
        input: impl Into<JsonValue>,
        request: Option<&ObjectRequestOptions>,
    ) -> ObjectTransportRequest {
        let request = request.cloned().unwrap_or_default();
        let mut headers = merged_headers(&self.options.headers, &request.headers);
        headers
            .entry("content-type".to_string())
            .or_insert_with(|| "application/json".to_string());

        ObjectTransportRequest {
            method: ObjectTransportMethod::Post,
            api: self.options.api.clone(),
            headers,
            body: input.into(),
            credentials: self.options.credentials,
        }
    }

    pub fn process_text_response_stream<S>(
        &self,
        chunks: impl IntoIterator<Item = S>,
    ) -> ObjectStreamResult
    where
        S: AsRef<str>,
    {
        process_object_text_stream(chunks)
    }
}

/// Processes upstream object text response chunks with partial JSON repair.
pub fn process_object_text_stream<S>(chunks: impl IntoIterator<Item = S>) -> ObjectStreamResult
where
    S: AsRef<str>,
{
    let mut accumulated_text = String::new();
    let mut latest_object = None::<JsonValue>;
    let mut updates = Vec::new();

    for chunk in chunks {
        accumulated_text.push_str(chunk.as_ref());
        let parse_result = parse_partial_json(Some(&accumulated_text));
        let state = parse_result.state();
        let Some(current_object) = parse_result.value().cloned() else {
            continue;
        };

        if latest_object
            .as_ref()
            .is_some_and(|latest| is_deep_equal_data(latest, &current_object))
        {
            continue;
        }

        latest_object = Some(current_object.clone());
        updates.push(ObjectStreamUpdate::new(current_object, state));
    }

    ObjectStreamResult::new(updates, latest_object)
}

/// Parses one complete JSON value with the same final-object boundary used by
/// upstream object hooks before schema validation.
pub fn parse_object_stream_final_json(text: &str) -> Option<JsonValue> {
    match safe_parse_json(text) {
        ParseJsonResult::Success { value, .. } => Some(value),
        ParseJsonResult::Failure { .. } => None,
    }
}

fn merged_headers(base: &Headers, overrides: &Headers) -> Headers {
    let mut headers = normalize_header_map(base);
    headers.extend(normalize_header_map(overrides));
    headers
}

fn normalize_header_map(headers: &Headers) -> Headers {
    normalize_headers(Some(
        headers
            .iter()
            .map(|(name, value)| (name.clone(), Some(value.clone()))),
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        ObjectRequestOptions, ObjectStreamUpdate, ObjectTransport, ObjectTransportOptions,
        parse_object_stream_final_json, process_object_text_stream,
    };
    use crate::{ParsePartialJsonState, RequestCredentials};

    #[test]
    fn object_transport_builds_post_request_with_input_body() {
        let transport = ObjectTransport::with_options(
            ObjectTransportOptions::new("/api/object")
                .with_credentials(RequestCredentials::SameOrigin)
                .with_header("X-Base", "base"),
        );
        let request_options = ObjectRequestOptions::new().with_header("X-Request", "request");

        let request = transport.build_object_request(
            json!({
                "prompt": "Extract facts",
                "schemaName": "facts"
            }),
            Some(&request_options),
        );

        assert_eq!(
            serde_json::to_value(request).expect("request serializes"),
            json!({
                "method": "POST",
                "api": "/api/object",
                "headers": {
                    "content-type": "application/json",
                    "x-base": "base",
                    "x-request": "request"
                },
                "body": {
                    "prompt": "Extract facts",
                    "schemaName": "facts"
                },
                "credentials": "same-origin"
            })
        );
    }

    #[test]
    fn object_transport_processes_distinct_partial_json_updates() {
        let result =
            process_object_text_stream([r#"{"name":"Ada""#, r#","items":[1"#, r#"]"#, r#"}"#]);

        assert_eq!(
            result.updates,
            vec![
                ObjectStreamUpdate::new(
                    json!({ "name": "Ada" }),
                    ParsePartialJsonState::RepairedParse
                ),
                ObjectStreamUpdate::new(
                    json!({
                        "name": "Ada",
                        "items": [1]
                    }),
                    ParsePartialJsonState::RepairedParse
                ),
            ]
        );
        assert_eq!(
            result.object,
            Some(json!({
                "name": "Ada",
                "items": [1]
            }))
        );
    }

    #[test]
    fn object_transport_skips_duplicate_partial_objects() {
        let result = ObjectTransport::new("/api/object")
            .process_text_response_stream([r#"{"count":1"#, r#"}"#]);

        assert_eq!(
            result.updates,
            vec![ObjectStreamUpdate::new(
                json!({ "count": 1 }),
                ParsePartialJsonState::RepairedParse
            )]
        );
        assert_eq!(result.object, Some(json!({ "count": 1 })));
    }

    #[test]
    fn object_transport_ignores_empty_chunks_until_json_can_be_repaired() {
        let result = process_object_text_stream(["", r#"{"ok":true"#]);

        assert_eq!(
            result.updates,
            vec![ObjectStreamUpdate::new(
                json!({ "ok": true }),
                ParsePartialJsonState::RepairedParse
            )]
        );
    }

    #[test]
    fn object_transport_parses_final_json_for_validation_boundary() {
        assert_eq!(
            parse_object_stream_final_json(r#"{"ok":true}"#),
            Some(json!({ "ok": true }))
        );
        assert_eq!(parse_object_stream_final_json(r#"{"ok":true"#), None);
    }
}
