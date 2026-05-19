use std::fmt;

use crate::chat_transport::RequestCredentials;
use crate::headers::Headers;
use crate::json::{JsonObject, JsonValue};
use crate::provider_utils::{ParseJsonResult, normalize_headers, parse_json_event_stream};
use crate::ui_message_stream::UiMessageChunk;

/// Streaming protocol accepted by upstream `callCompletionApi`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompletionStreamProtocol {
    /// UI-message JSON event stream protocol.
    #[default]
    Data,

    /// Plain text streaming protocol.
    Text,
}

/// Extra per-request options for completion transports.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionRequestOptions {
    /// Additional HTTP headers passed to the completion API endpoint.
    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,

    /// Additional JSON body properties sent to the completion API endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<JsonObject>,
}

impl CompletionRequestOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    pub fn with_body(mut self, body: JsonObject) -> Self {
        self.body = Some(body);
        self
    }

    pub fn with_body_property(
        mut self,
        name: impl Into<String>,
        value: impl Into<JsonValue>,
    ) -> Self {
        self.body
            .get_or_insert_with(JsonObject::new)
            .insert(name.into(), value.into());
        self
    }
}

/// Constructor options for the Rust equivalent of upstream completion API calls.
#[derive(Clone, Debug, PartialEq)]
pub struct CompletionTransportOptions {
    pub api: String,
    pub credentials: Option<RequestCredentials>,
    pub headers: Headers,
    pub body: Option<JsonObject>,
    pub stream_protocol: CompletionStreamProtocol,
}

impl Default for CompletionTransportOptions {
    fn default() -> Self {
        Self {
            api: "/api/completion".to_string(),
            credentials: None,
            headers: Headers::new(),
            body: None,
            stream_protocol: CompletionStreamProtocol::Data,
        }
    }
}

impl CompletionTransportOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_api(mut self, api: impl Into<String>) -> Self {
        self.api = api.into();
        self
    }

    pub fn with_credentials(mut self, credentials: RequestCredentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    pub fn with_body(mut self, body: JsonObject) -> Self {
        self.body = Some(body);
        self
    }

    pub fn with_body_property(
        mut self,
        name: impl Into<String>,
        value: impl Into<JsonValue>,
    ) -> Self {
        self.body
            .get_or_insert_with(JsonObject::new)
            .insert(name.into(), value.into());
        self
    }

    pub fn with_stream_protocol(mut self, stream_protocol: CompletionStreamProtocol) -> Self {
        self.stream_protocol = stream_protocol;
        self
    }
}

/// HTTP method used by deterministic completion transport request builders.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum CompletionTransportMethod {
    Post,
}

/// Deterministic HTTP request produced by [`CompletionTransport`].
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionTransportRequest {
    pub method: CompletionTransportMethod,
    pub api: String,

    #[serde(default, skip_serializing_if = "Headers::is_empty")]
    pub headers: Headers,

    pub body: JsonValue,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<RequestCredentials>,
}

/// Errors returned while processing completion response streams.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompletionTransportError {
    InvalidStream(String),
    StreamError(String),
}

impl fmt::Display for CompletionTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStream(message) | Self::StreamError(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl std::error::Error for CompletionTransportError {}

/// Deterministic Rust equivalent of upstream `callCompletionApi` request and
/// response-stream behavior.
#[derive(Clone, Debug, PartialEq)]
pub struct CompletionTransport {
    options: CompletionTransportOptions,
}

impl CompletionTransport {
    pub fn new() -> Self {
        Self::with_options(CompletionTransportOptions::default())
    }

    pub fn with_options(options: CompletionTransportOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &CompletionTransportOptions {
        &self.options
    }

    pub fn build_completion_request(
        &self,
        prompt: impl Into<String>,
        request: Option<&CompletionRequestOptions>,
    ) -> CompletionTransportRequest {
        let request = request.cloned().unwrap_or_default();
        let mut headers = merged_headers(&self.options.headers, &request.headers);
        headers
            .entry("content-type".to_string())
            .or_insert_with(|| "application/json".to_string());

        let mut body = JsonObject::new();
        body.insert("prompt".to_string(), JsonValue::String(prompt.into()));
        if let Some(options_body) = &self.options.body {
            body.extend(options_body.clone());
        }
        if let Some(request_body) = request.body {
            body.extend(request_body);
        }

        CompletionTransportRequest {
            method: CompletionTransportMethod::Post,
            api: self.options.api.clone(),
            headers,
            body: JsonValue::Object(body),
            credentials: self.options.credentials,
        }
    }

    pub fn process_text_response_stream<S>(&self, chunks: impl IntoIterator<Item = S>) -> String
    where
        S: Into<String>,
    {
        process_completion_text_stream(chunks)
    }

    pub fn process_data_response_event_stream<B>(
        &self,
        chunks: impl IntoIterator<Item = B>,
    ) -> Result<String, CompletionTransportError>
    where
        B: AsRef<[u8]>,
    {
        process_completion_data_event_stream(chunks)
    }
}

impl Default for CompletionTransport {
    fn default() -> Self {
        Self::new()
    }
}

/// Processes upstream completion `streamProtocol: "text"` chunks.
pub fn process_completion_text_stream<S>(chunks: impl IntoIterator<Item = S>) -> String
where
    S: Into<String>,
{
    chunks.into_iter().map(Into::into).collect()
}

/// Processes upstream completion `streamProtocol: "data"` SSE chunks.
pub fn process_completion_data_event_stream<B>(
    chunks: impl IntoIterator<Item = B>,
) -> Result<String, CompletionTransportError>
where
    B: AsRef<[u8]>,
{
    let parsed_chunks = parse_json_event_stream(chunks, |value| {
        serde_json::from_value::<UiMessageChunk>(value.clone())
    });
    let mut completion = String::new();

    for parsed_chunk in parsed_chunks {
        match parsed_chunk {
            ParseJsonResult::Success { value, .. } => match value {
                UiMessageChunk::TextDelta { delta, .. } => completion.push_str(&delta),
                UiMessageChunk::Error { error_text } => {
                    return Err(CompletionTransportError::StreamError(error_text));
                }
                _ => {}
            },
            ParseJsonResult::Failure { error, .. } => {
                return Err(CompletionTransportError::InvalidStream(format!(
                    "Completion stream chunk parse failed: {error}"
                )));
            }
        }
    }

    Ok(completion)
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
        CompletionRequestOptions, CompletionStreamProtocol, CompletionTransport,
        CompletionTransportError, CompletionTransportOptions, process_completion_data_event_stream,
        process_completion_text_stream,
    };
    use crate::RequestCredentials;
    use crate::ui_message_stream::UiMessageChunk;

    #[test]
    fn completion_transport_builds_default_request() {
        let transport = CompletionTransport::new();

        let request = transport.build_completion_request("Write a haiku", None);

        assert_eq!(
            serde_json::to_value(request).expect("request serializes"),
            json!({
                "method": "POST",
                "api": "/api/completion",
                "headers": {
                    "content-type": "application/json"
                },
                "body": {
                    "prompt": "Write a haiku"
                }
            })
        );
        assert_eq!(
            transport.options().stream_protocol,
            CompletionStreamProtocol::Data
        );
    }

    #[test]
    fn completion_transport_builds_prepared_request_with_overrides() {
        let transport = CompletionTransport::with_options(
            CompletionTransportOptions::new()
                .with_api("/custom/completion")
                .with_credentials(RequestCredentials::Include)
                .with_header("X-Base", "base")
                .with_body_property("sessionId", json!("session-1"))
                .with_stream_protocol(CompletionStreamProtocol::Text),
        );

        let request_options = CompletionRequestOptions::new()
            .with_header("X-Request", "request")
            .with_body_property("prompt", json!("Override prompt"))
            .with_body_property("extra", json!(true));
        let request = transport.build_completion_request("Original prompt", Some(&request_options));

        assert_eq!(
            serde_json::to_value(request).expect("request serializes"),
            json!({
                "method": "POST",
                "api": "/custom/completion",
                "headers": {
                    "content-type": "application/json",
                    "x-base": "base",
                    "x-request": "request"
                },
                "body": {
                    "prompt": "Override prompt",
                    "sessionId": "session-1",
                    "extra": true
                },
                "credentials": "include"
            })
        );
        assert_eq!(
            transport.options().stream_protocol,
            CompletionStreamProtocol::Text
        );
    }

    #[test]
    fn completion_transport_processes_text_stream() {
        let completion = process_completion_text_stream(["Hel", "lo", " world"]);

        assert_eq!(completion, "Hello world");
    }

    #[test]
    fn completion_transport_processes_data_event_stream() {
        let first = serde_json::to_string(&UiMessageChunk::text_delta("text-1", "Hello"))
            .expect("chunk serializes");
        let ignored =
            serde_json::to_string(&UiMessageChunk::start_step()).expect("chunk serializes");
        let second = serde_json::to_string(&UiMessageChunk::text_delta("text-1", " world"))
            .expect("chunk serializes");
        let stream =
            format!("data: {first}\n\ndata: {ignored}\n\ndata: {second}\n\ndata: [DONE]\n\n");

        let completion = CompletionTransport::new()
            .process_data_response_event_stream([stream.as_bytes()])
            .expect("stream processes");

        assert_eq!(completion, "Hello world");
    }

    #[test]
    fn completion_transport_reports_data_event_error_chunks() {
        let error = serde_json::to_string(&UiMessageChunk::error("Provider failed"))
            .expect("chunk serializes");
        let stream = format!("data: {error}\n\n");

        let error = process_completion_data_event_stream([stream.as_bytes()])
            .expect_err("error chunks fail completion");

        assert_eq!(
            error,
            CompletionTransportError::StreamError("Provider failed".to_string())
        );
    }

    #[test]
    fn completion_transport_reports_invalid_data_event_chunks() {
        let stream = "data: {\"type\":\"text-delta\",\"id\":\"text-1\"}\n\n";

        let error = process_completion_data_event_stream([stream.as_bytes()])
            .expect_err("invalid chunks fail completion");

        assert!(matches!(
            error,
            CompletionTransportError::InvalidStream(message)
                if message.contains("Completion stream chunk parse failed")
        ));
    }
}
