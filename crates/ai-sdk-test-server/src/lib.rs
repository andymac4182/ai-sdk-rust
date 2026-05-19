//! Portable test-server contracts for the Rust port of upstream
//! `@ai-sdk/test-server`.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use serde_json::{Value as JsonValue, json};

/// The test-server crate version compiled into the library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Headers attached to test requests and responses.
pub type TestHeaders = BTreeMap<String, String>;

/// A configured response for a test URL.
#[derive(Clone, Debug, PartialEq)]
pub enum UrlResponse {
    /// JSON response with a default `200` status and `content-type: application/json`.
    JsonValue {
        headers: TestHeaders,
        body: JsonValue,
    },
    /// Text event-stream response represented as ordered string chunks.
    StreamChunks {
        headers: TestHeaders,
        chunks: Vec<String>,
    },
    /// Binary response body.
    Binary { headers: TestHeaders, body: Vec<u8> },
    /// Empty response with configurable status.
    Empty { headers: TestHeaders, status: u16 },
    /// Error text response with configurable status and body.
    Error {
        headers: TestHeaders,
        status: u16,
        body: String,
    },
    /// Stream response controlled by test code.
    ControlledStream {
        headers: TestHeaders,
        controller: TestResponseController,
    },
}

impl UrlResponse {
    /// Creates a JSON response.
    pub fn json_value(body: JsonValue) -> Self {
        Self::JsonValue {
            headers: TestHeaders::new(),
            body,
        }
    }

    /// Creates a stream response from ordered chunks.
    pub fn stream_chunks(chunks: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::StreamChunks {
            headers: TestHeaders::new(),
            chunks: chunks.into_iter().map(Into::into).collect(),
        }
    }

    /// Creates a binary response.
    pub fn binary(body: impl Into<Vec<u8>>) -> Self {
        Self::Binary {
            headers: TestHeaders::new(),
            body: body.into(),
        }
    }

    /// Creates an empty response.
    pub fn empty(status: u16) -> Self {
        Self::Empty {
            headers: TestHeaders::new(),
            status,
        }
    }

    /// Creates an error response.
    pub fn error(status: u16, body: impl Into<String>) -> Self {
        Self::Error {
            headers: TestHeaders::new(),
            status,
            body: body.into(),
        }
    }

    /// Creates a controlled stream response.
    pub fn controlled_stream(controller: TestResponseController) -> Self {
        Self::ControlledStream {
            headers: TestHeaders::new(),
            controller,
        }
    }

    /// Adds a response header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers_mut().insert(name.into(), value.into());
        self
    }

    fn headers_mut(&mut self) -> &mut TestHeaders {
        match self {
            Self::JsonValue { headers, .. }
            | Self::StreamChunks { headers, .. }
            | Self::Binary { headers, .. }
            | Self::Empty { headers, .. }
            | Self::Error { headers, .. }
            | Self::ControlledStream { headers, .. } => headers,
        }
    }
}

/// Response source for a URL handler.
#[derive(Clone)]
pub enum UrlResponseParameter {
    /// One response reused for every call.
    Static(Option<UrlResponse>),
    /// Per-call responses selected by zero-based call number.
    Sequence(Vec<Option<UrlResponse>>),
    /// Dynamic response selected by zero-based call number.
    Dynamic(Arc<dyn Fn(usize) -> Option<UrlResponse> + Send + Sync>),
}

impl UrlResponseParameter {
    /// Creates a missing response, which renders as a 404.
    pub fn missing() -> Self {
        Self::Static(None)
    }

    /// Creates a dynamic response selector.
    pub fn dynamic(
        response: impl Fn(usize) -> Option<UrlResponse> + Send + Sync + 'static,
    ) -> Self {
        Self::Dynamic(Arc::new(response))
    }

    fn response_for(&self, call_number: usize) -> Option<UrlResponse> {
        match self {
            Self::Static(response) => response.clone(),
            Self::Sequence(responses) => responses.get(call_number).cloned().flatten(),
            Self::Dynamic(response) => response(call_number),
        }
    }
}

impl fmt::Debug for UrlResponseParameter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Static(response) => formatter.debug_tuple("Static").field(response).finish(),
            Self::Sequence(responses) => {
                formatter.debug_tuple("Sequence").field(responses).finish()
            }
            Self::Dynamic(_) => formatter.write_str("Dynamic(<response selector>)"),
        }
    }
}

impl From<UrlResponse> for UrlResponseParameter {
    fn from(response: UrlResponse) -> Self {
        Self::Static(Some(response))
    }
}

impl From<Option<UrlResponse>> for UrlResponseParameter {
    fn from(response: Option<UrlResponse>) -> Self {
        Self::Static(response)
    }
}

impl From<Vec<UrlResponse>> for UrlResponseParameter {
    fn from(responses: Vec<UrlResponse>) -> Self {
        Self::Sequence(responses.into_iter().map(Some).collect())
    }
}

impl From<Vec<Option<UrlResponse>>> for UrlResponseParameter {
    fn from(responses: Vec<Option<UrlResponse>>) -> Self {
        Self::Sequence(responses)
    }
}

/// URL handler with a mutable response source.
#[derive(Clone, Debug)]
pub struct UrlHandler {
    pub response: UrlResponseParameter,
}

impl UrlHandler {
    /// Creates a handler from any supported response source.
    pub fn new(response: impl Into<UrlResponseParameter>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

/// Test request captured by the server.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TestRequest {
    pub method: String,
    pub url: String,
    pub headers: TestHeaders,
    pub body: Option<String>,
    pub credentials: Option<String>,
}

impl TestRequest {
    /// Creates a request with method and URL.
    pub fn new(method: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            method: method.into(),
            url: url.into(),
            headers: TestHeaders::new(),
            body: None,
            credentials: None,
        }
    }

    /// Adds a request header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Adds a text body.
    pub fn with_body(mut self, body: impl Into<String>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Adds a credentials marker.
    pub fn with_credentials(mut self, credentials: impl Into<String>) -> Self {
        self.credentials = Some(credentials.into());
        self
    }
}

/// Parsed multipart/form-data request part.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultipartRequestPart {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub body: String,
}

impl MultipartRequestPart {
    /// Returns the part body as text.
    pub fn text(&self) -> &str {
        &self.body
    }
}

/// Captured request inspection helper.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestServerCall {
    request: TestRequest,
}

impl TestServerCall {
    fn new(request: TestRequest) -> Self {
        Self { request }
    }

    /// Parses the captured request body as JSON.
    pub fn request_body_json(&self) -> Option<JsonValue> {
        self.request
            .body
            .as_deref()
            .and_then(|body| serde_json::from_str(body).ok())
    }

    /// Parses a `multipart/form-data` request body by part name.
    pub fn request_body_multipart(&self) -> Option<BTreeMap<String, MultipartRequestPart>> {
        let content_type = self
            .request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value.as_str())?;
        let boundary = multipart_boundary(content_type)?;
        let body = self.request.body.as_deref()?;
        parse_multipart_body(body, boundary)
    }

    /// Returns captured request headers, excluding `user-agent`.
    pub fn request_headers(&self) -> TestHeaders {
        self.request
            .headers
            .iter()
            .filter(|(name, _)| !name.eq_ignore_ascii_case("user-agent"))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect()
    }

    /// Returns the captured request user-agent header.
    pub fn request_user_agent(&self) -> Option<&str> {
        self.request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("user-agent"))
            .map(|(_, value)| value.as_str())
    }

    /// Returns the captured request URL query parameters without URL-decoding.
    pub fn request_url_search_params(&self) -> BTreeMap<String, String> {
        self.request
            .url
            .split_once('?')
            .map(|(_, query)| {
                query
                    .split('&')
                    .filter(|part| !part.is_empty())
                    .map(|part| {
                        let (name, value) = part.split_once('=').unwrap_or((part, ""));
                        (name.to_string(), value.to_string())
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns the captured request URL.
    pub fn request_url(&self) -> &str {
        &self.request.url
    }

    /// Returns the captured request method.
    pub fn request_method(&self) -> &str {
        &self.request.method
    }

    /// Returns the captured request credentials marker.
    pub fn request_credentials(&self) -> Option<&str> {
        self.request.credentials.as_deref()
    }
}

/// Rendered response from a test URL handler.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderedResponse {
    pub status: u16,
    pub headers: TestHeaders,
    pub body: RenderedBody,
}

/// Rendered response body.
#[derive(Clone, Debug, PartialEq)]
pub enum RenderedBody {
    Json(JsonValue),
    StreamChunks(Vec<String>),
    Binary(Vec<u8>),
    Text(String),
    Empty,
}

/// In-memory Rust equivalent of upstream `createTestServer`.
#[derive(Clone, Debug)]
pub struct TestServer {
    pub urls: BTreeMap<String, UrlHandler>,
    pub calls: Vec<TestServerCall>,
    original_urls: BTreeMap<String, UrlHandler>,
}

impl TestServer {
    /// Creates a test server with configured URL handlers.
    pub fn new(routes: impl IntoIterator<Item = (impl Into<String>, UrlHandler)>) -> Self {
        let urls = routes
            .into_iter()
            .map(|(url, handler)| (url.into(), handler))
            .collect::<BTreeMap<_, _>>();
        Self {
            original_urls: urls.clone(),
            urls,
            calls: Vec::new(),
        }
    }

    /// Starts the test server. The Rust port is in-memory, so this is a no-op.
    pub fn start(&mut self) {}

    /// Stops the test server. The Rust port is in-memory, so this is a no-op.
    pub fn stop(&mut self) {}

    /// Restores original route responses and clears captured calls.
    pub fn reset(&mut self) {
        self.urls = self.original_urls.clone();
        self.calls.clear();
    }

    /// Handles a request against a configured URL and records the call.
    pub fn handle(&mut self, url: &str, request: TestRequest) -> RenderedResponse {
        let call_number = self.calls.len();
        self.calls.push(TestServerCall::new(request));
        let response = self
            .urls
            .get(url)
            .and_then(|handler| handler.response.response_for(call_number));
        render_url_response(response)
    }
}

/// Creates a test server with configured URL handlers.
pub fn create_test_server(
    routes: impl IntoIterator<Item = (impl Into<String>, UrlHandler)>,
) -> TestServer {
    TestServer::new(routes)
}

/// Rust analogue of upstream `convertArrayToReadableStream`.
pub fn convert_array_to_readable_stream<T>(values: impl IntoIterator<Item = T>) -> Vec<T> {
    values.into_iter().collect()
}

fn multipart_boundary(content_type: &str) -> Option<&str> {
    let mut parameters = content_type.split(';').map(str::trim);
    let media_type = parameters.next()?;
    if !media_type.eq_ignore_ascii_case("multipart/form-data") {
        return None;
    }
    parameters.find_map(|parameter| {
        let (name, value) = parameter.split_once('=')?;
        if name.trim().eq_ignore_ascii_case("boundary") {
            Some(value.trim().trim_matches('"'))
        } else {
            None
        }
    })
}

fn parse_multipart_body(
    body: &str,
    boundary: &str,
) -> Option<BTreeMap<String, MultipartRequestPart>> {
    let marker = format!("--{boundary}");
    let mut parts = BTreeMap::new();

    for section in body.split(&marker).skip(1) {
        let section = section.trim_start_matches("\r\n");
        if section.starts_with("--") {
            break;
        }
        let section = section.trim_end_matches("\r\n");
        if section.is_empty() {
            continue;
        }
        let (raw_headers, part_body) = section.split_once("\r\n\r\n")?;
        let headers = parse_multipart_headers(raw_headers);
        let content_disposition = headers.get("content-disposition")?;
        let disposition_parameters = parse_header_parameters(content_disposition);
        let name = disposition_parameters.get("name")?.clone();
        let part = MultipartRequestPart {
            name: name.clone(),
            filename: disposition_parameters.get("filename").cloned(),
            content_type: headers.get("content-type").cloned(),
            body: part_body.to_string(),
        };
        parts.insert(name, part);
    }

    Some(parts)
}

fn parse_multipart_headers(raw_headers: &str) -> BTreeMap<String, String> {
    raw_headers
        .lines()
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect()
}

fn parse_header_parameters(value: &str) -> BTreeMap<String, String> {
    value
        .split(';')
        .skip(1)
        .filter_map(|parameter| {
            let (name, value) = parameter.trim().split_once('=')?;
            Some((name.trim().to_string(), unquote_header_value(value.trim())))
        })
        .collect()
}

fn unquote_header_value(value: &str) -> String {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
        .to_string()
}

/// Controller for deterministic stream tests.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TestResponseController {
    chunks: Vec<String>,
    error: Option<String>,
    closed: bool,
}

impl TestResponseController {
    /// Creates an empty response controller.
    pub fn new() -> Self {
        Self::default()
    }

    /// Writes a stream chunk.
    pub fn write(&mut self, chunk: impl Into<String>) {
        self.chunks.push(chunk.into());
    }

    /// Records a stream error.
    pub fn error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }

    /// Closes the stream.
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// Returns written chunks.
    pub fn chunks(&self) -> &[String] {
        &self.chunks
    }

    /// Returns the recorded error, if any.
    pub fn error_message(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Returns whether the stream was closed.
    pub fn is_closed(&self) -> bool {
        self.closed
    }
}

fn render_url_response(response: Option<UrlResponse>) -> RenderedResponse {
    match response {
        Some(UrlResponse::JsonValue { headers, body }) => RenderedResponse {
            status: 200,
            headers: with_default_header(headers, "content-type", "application/json"),
            body: RenderedBody::Json(body),
        },
        Some(UrlResponse::StreamChunks { headers, chunks }) => RenderedResponse {
            status: 200,
            headers: with_stream_headers(headers),
            body: RenderedBody::StreamChunks(convert_array_to_readable_stream(chunks)),
        },
        Some(UrlResponse::Binary { headers, body }) => RenderedResponse {
            status: 200,
            headers,
            body: RenderedBody::Binary(body),
        },
        Some(UrlResponse::Empty { headers, status }) => RenderedResponse {
            status,
            headers,
            body: RenderedBody::Empty,
        },
        Some(UrlResponse::Error {
            headers,
            status,
            body,
        }) => RenderedResponse {
            status,
            headers,
            body: RenderedBody::Text(body),
        },
        Some(UrlResponse::ControlledStream {
            headers,
            controller,
        }) => RenderedResponse {
            status: 200,
            headers: with_stream_headers(headers),
            body: RenderedBody::StreamChunks(controller.chunks().to_vec()),
        },
        None => RenderedResponse {
            status: 404,
            headers: with_default_header(TestHeaders::new(), "content-type", "application/json"),
            body: RenderedBody::Json(json!({ "error": "Not Found" })),
        },
    }
}

fn with_default_header(
    mut headers: TestHeaders,
    name: impl Into<String>,
    value: impl Into<String>,
) -> TestHeaders {
    let name = name.into();
    if !headers.keys().any(|key| key.eq_ignore_ascii_case(&name)) {
        headers.insert(name, value.into());
    }
    headers
}

fn with_stream_headers(headers: TestHeaders) -> TestHeaders {
    let headers = with_default_header(headers, "content-type", "text/event-stream");
    let headers = with_default_header(headers, "cache-control", "no-cache");
    with_default_header(headers, "connection", "keep-alive")
}

#[cfg(test)]
mod tests {
    use super::{
        RenderedBody, TestRequest, TestResponseController, UrlHandler, UrlResponse,
        UrlResponseParameter, create_test_server,
    };
    use serde_json::json;

    #[test]
    fn create_test_server_exposes_urls_and_empty_calls() {
        let server = create_test_server([(
            "https://api.example.com/test",
            UrlHandler::new(UrlResponse::json_value(json!({ "message": "hello world" }))),
        )]);

        assert!(server.urls.contains_key("https://api.example.com/test"));
        assert!(server.calls.is_empty());
    }

    #[test]
    fn create_test_server_supports_response_mutations_and_reset() {
        let mut server = create_test_server([(
            "https://api.example.com/test",
            UrlHandler::new(UrlResponse::json_value(json!({ "count": 1 }))),
        )]);

        server
            .urls
            .get_mut("https://api.example.com/test")
            .expect("route exists")
            .response = UrlResponse::json_value(json!({ "count": 2 })).into();

        let response = server.handle(
            "https://api.example.com/test",
            TestRequest::new("GET", "https://api.example.com/test"),
        );
        assert_eq!(response.body, RenderedBody::Json(json!({ "count": 2 })));
        assert_eq!(server.calls.len(), 1);

        server.reset();
        assert!(server.calls.is_empty());
        let response = server.handle(
            "https://api.example.com/test",
            TestRequest::new("GET", "https://api.example.com/test"),
        );
        assert_eq!(response.body, RenderedBody::Json(json!({ "count": 1 })));
    }

    #[test]
    fn create_test_server_supports_response_types() {
        let mut controller = TestResponseController::new();
        controller.write("first");
        controller.write("second");
        controller.close();

        let mut server = create_test_server([
            (
                "https://api.example.com/json",
                UrlHandler::new(UrlResponse::json_value(json!({ "test": true }))),
            ),
            (
                "https://api.example.com/stream",
                UrlHandler::new(UrlResponse::stream_chunks(["chunk1", "chunk2"])),
            ),
            (
                "https://api.example.com/error",
                UrlHandler::new(UrlResponse::error(400, "Bad Request")),
            ),
            (
                "https://api.example.com/binary",
                UrlHandler::new(UrlResponse::binary([1, 2, 3])),
            ),
            (
                "https://api.example.com/empty",
                UrlHandler::new(UrlResponse::empty(204)),
            ),
            (
                "https://api.example.com/controlled",
                UrlHandler::new(UrlResponse::controlled_stream(controller)),
            ),
        ]);

        assert_eq!(
            server
                .handle(
                    "https://api.example.com/json",
                    TestRequest::new("GET", "https://api.example.com/json"),
                )
                .body,
            RenderedBody::Json(json!({ "test": true }))
        );
        assert_eq!(
            server
                .handle(
                    "https://api.example.com/stream",
                    TestRequest::new("GET", "https://api.example.com/stream"),
                )
                .body,
            RenderedBody::StreamChunks(vec!["chunk1".to_string(), "chunk2".to_string()])
        );
        let error = server.handle(
            "https://api.example.com/error",
            TestRequest::new("GET", "https://api.example.com/error"),
        );
        assert_eq!(error.status, 400);
        assert_eq!(error.body, RenderedBody::Text("Bad Request".to_string()));
        assert_eq!(
            server
                .handle(
                    "https://api.example.com/binary",
                    TestRequest::new("GET", "https://api.example.com/binary"),
                )
                .body,
            RenderedBody::Binary(vec![1, 2, 3])
        );
        assert_eq!(
            server
                .handle(
                    "https://api.example.com/empty",
                    TestRequest::new("GET", "https://api.example.com/empty"),
                )
                .status,
            204
        );
        assert_eq!(
            server
                .handle(
                    "https://api.example.com/controlled",
                    TestRequest::new("GET", "https://api.example.com/controlled"),
                )
                .body,
            RenderedBody::StreamChunks(vec!["first".to_string(), "second".to_string()])
        );
    }

    #[test]
    fn create_test_server_tracks_request_inspection() {
        let mut server = create_test_server([(
            "https://api.example.com/test",
            UrlHandler::new(UrlResponse::json_value(json!({ "ok": true }))),
        )]);

        server.handle(
            "https://api.example.com/test",
            TestRequest::new("POST", "https://api.example.com/test?q=rust&limit=2")
                .with_header("authorization", "Bearer token")
                .with_header("user-agent", "ai-sdk-test")
                .with_credentials("include")
                .with_body(r#"{ "prompt": "hello" }"#),
        );

        let call = server.calls.first().expect("call is recorded");
        assert_eq!(call.request_method(), "POST");
        assert_eq!(
            call.request_url(),
            "https://api.example.com/test?q=rust&limit=2"
        );
        assert_eq!(call.request_credentials(), Some("include"));
        assert_eq!(call.request_user_agent(), Some("ai-sdk-test"));
        assert_eq!(
            call.request_headers()
                .get("authorization")
                .map(String::as_str),
            Some("Bearer token")
        );
        assert!(!call.request_headers().contains_key("user-agent"));
        assert_eq!(call.request_body_json(), Some(json!({ "prompt": "hello" })));
        assert_eq!(
            call.request_url_search_params()
                .get("q")
                .map(String::as_str),
            Some("rust")
        );
    }

    #[test]
    fn create_test_server_parses_multipart_request_body() {
        let boundary = "----ai-sdk-rust-boundary";
        let body = format!(
            "--{boundary}\r\n\
Content-Disposition: form-data; name=\"prompt\"\r\n\
\r\n\
hello\r\n\
--{boundary}\r\n\
Content-Disposition: form-data; name=\"file\"; filename=\"note.txt\"\r\n\
Content-Type: text/plain\r\n\
\r\n\
file body\r\n\
--{boundary}--\r\n"
        );
        let mut server = create_test_server([(
            "https://api.example.com/upload",
            UrlHandler::new(UrlResponse::empty(200)),
        )]);

        server.handle(
            "https://api.example.com/upload",
            TestRequest::new("POST", "https://api.example.com/upload")
                .with_header(
                    "content-type",
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .with_body(body),
        );

        let call = server.calls.first().expect("call is recorded");
        let parts = call
            .request_body_multipart()
            .expect("multipart body parses");
        let prompt = parts.get("prompt").expect("prompt part exists");
        assert_eq!(prompt.filename, None);
        assert_eq!(prompt.content_type, None);
        assert_eq!(prompt.text(), "hello");

        let file = parts.get("file").expect("file part exists");
        assert_eq!(file.filename.as_deref(), Some("note.txt"));
        assert_eq!(file.content_type.as_deref(), Some("text/plain"));
        assert_eq!(file.text(), "file body");
    }

    #[test]
    fn create_test_server_selects_sequence_and_dynamic_responses_by_call_number() {
        let mut server = create_test_server([
            (
                "https://api.example.com/sequence",
                UrlHandler::new(vec![
                    UrlResponse::json_value(json!({ "call": 0 })),
                    UrlResponse::json_value(json!({ "call": 1 })),
                ]),
            ),
            (
                "https://api.example.com/dynamic",
                UrlHandler::new(UrlResponseParameter::dynamic(|call_number| {
                    Some(UrlResponse::json_value(json!({ "call": call_number })))
                })),
            ),
        ]);

        assert_eq!(
            server
                .handle(
                    "https://api.example.com/sequence",
                    TestRequest::new("GET", "https://api.example.com/sequence"),
                )
                .body,
            RenderedBody::Json(json!({ "call": 0 }))
        );
        assert_eq!(
            server
                .handle(
                    "https://api.example.com/sequence",
                    TestRequest::new("GET", "https://api.example.com/sequence"),
                )
                .body,
            RenderedBody::Json(json!({ "call": 1 }))
        );
        assert_eq!(
            server
                .handle(
                    "https://api.example.com/sequence",
                    TestRequest::new("GET", "https://api.example.com/sequence"),
                )
                .status,
            404
        );
        assert_eq!(
            server
                .handle(
                    "https://api.example.com/dynamic",
                    TestRequest::new("GET", "https://api.example.com/dynamic"),
                )
                .body,
            RenderedBody::Json(json!({ "call": 3 }))
        );
    }

    #[test]
    fn response_controller_records_writes_errors_and_close() {
        let mut controller = TestResponseController::new();
        controller.write("chunk1");
        controller.error("boom");
        controller.close();

        assert_eq!(controller.chunks(), &["chunk1".to_string()]);
        assert_eq!(controller.error_message(), Some("boom"));
        assert!(controller.is_closed());
    }
}
