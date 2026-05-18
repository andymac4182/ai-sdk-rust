use crate::headers::Headers;
use crate::json::JsonValue;
use crate::provider::ProviderMetadata;
use crate::provider_utils::normalize_headers;

/// Default content type used by upstream UI-message stream response helpers.
pub const UI_MESSAGE_STREAM_CONTENT_TYPE: &str = "text/event-stream";

/// Header that marks the upstream UI-message stream protocol version.
pub const UI_MESSAGE_STREAM_VERSION_HEADER: &str = "x-vercel-ai-ui-message-stream";

/// Current upstream UI-message stream protocol version.
pub const UI_MESSAGE_STREAM_VERSION: &str = "v1";

/// A subset of upstream UI-message stream chunks.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum UiMessageChunk {
    /// Start of a text part.
    TextStart {
        /// Text part identifier.
        id: String,

        /// Provider-specific metadata for the text part.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// Delta for a text part.
    TextDelta {
        /// Text part identifier.
        id: String,

        /// Text delta.
        delta: String,

        /// Provider-specific metadata for the text delta.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// End of a text part.
    TextEnd {
        /// Text part identifier.
        id: String,

        /// Provider-specific metadata for the text part.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_metadata: Option<ProviderMetadata>,
    },

    /// Error chunk sent to UI-message stream consumers.
    Error {
        /// Error text visible to the client.
        error_text: String,
    },
}

/// Role of an upstream UI message.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UiMessageRole {
    /// System message.
    System,

    /// User message.
    User,

    /// Assistant response message.
    Assistant,
}

/// Minimal portable shape of an upstream UI message.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiMessage {
    /// Message identifier.
    pub id: String,

    /// Message role.
    pub role: UiMessageRole,

    /// Optional message metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonValue>,

    /// UI message parts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<JsonValue>,
}

impl UiMessage {
    /// Creates a UI message with no metadata or parts.
    pub fn new(id: impl Into<String>, role: UiMessageRole) -> Self {
        Self {
            id: id.into(),
            role,
            metadata: None,
            parts: Vec::new(),
        }
    }

    /// Adds metadata to this UI message.
    pub fn with_metadata(mut self, metadata: impl Into<JsonValue>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    /// Adds one UI message part.
    pub fn with_part(mut self, part: impl Into<JsonValue>) -> Self {
        self.parts.push(part.into());
        self
    }
}

/// Response message id source accepted by [`get_response_ui_message_id`].
pub enum ResponseUiMessageId {
    /// Use this fixed response id.
    Id(String),

    /// Generate a new response id when the response is not a continuation.
    Generate(Box<dyn FnOnce() -> String>),
}

impl ResponseUiMessageId {
    /// Creates a fixed response message id.
    pub fn id(id: impl Into<String>) -> Self {
        Self::Id(id.into())
    }

    /// Creates a response message id generator.
    pub fn generate<F>(generate: F) -> Self
    where
        F: FnOnce() -> String + 'static,
    {
        Self::Generate(Box::new(generate))
    }
}

/// Determines the response UI message id for persistence-aware streams.
///
/// This mirrors upstream `getResponseUIMessageId`: if there are no original
/// messages, id generation is left to the client and `None` is returned. If the
/// last original message is an assistant message, its id is reused for
/// continuation. Otherwise the supplied id or id generator is used.
pub fn get_response_ui_message_id(
    original_messages: Option<&[UiMessage]>,
    response_message_id: ResponseUiMessageId,
) -> Option<String> {
    let original_messages = original_messages?;

    if let Some(last_message) = original_messages.last() {
        if last_message.role == UiMessageRole::Assistant {
            return Some(last_message.id.clone());
        }
    }

    match response_message_id {
        ResponseUiMessageId::Id(id) => Some(id),
        ResponseUiMessageId::Generate(generate) => Some(generate()),
    }
}

impl UiMessageChunk {
    /// Creates a text-start UI-message chunk.
    pub fn text_start(id: impl Into<String>) -> Self {
        Self::TextStart {
            id: id.into(),
            provider_metadata: None,
        }
    }

    /// Creates a text-delta UI-message chunk.
    pub fn text_delta(id: impl Into<String>, delta: impl Into<String>) -> Self {
        Self::TextDelta {
            id: id.into(),
            delta: delta.into(),
            provider_metadata: None,
        }
    }

    /// Creates a text-end UI-message chunk.
    pub fn text_end(id: impl Into<String>) -> Self {
        Self::TextEnd {
            id: id.into(),
            provider_metadata: None,
        }
    }

    /// Creates an error UI-message chunk.
    pub fn error(error_text: impl Into<String>) -> Self {
        Self::Error {
            error_text: error_text.into(),
        }
    }
}

/// Response metadata supplied to UI-message stream response helpers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UiMessageStreamResponseInit {
    /// HTTP status code. Defaults to `200`.
    pub status: Option<u16>,

    /// Optional HTTP status text.
    pub status_text: Option<String>,

    /// Optional response headers.
    pub headers: Option<Headers>,
}

impl UiMessageStreamResponseInit {
    /// Creates empty response initialization options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the response status code.
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    /// Sets the response status text.
    pub fn with_status_text(mut self, status_text: impl Into<String>) -> Self {
        self.status_text = Some(status_text.into());
        self
    }

    /// Replaces response headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Adds a response header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }
}

/// Options shared by UI-message stream response helpers.
#[derive(Clone, Debug, PartialEq)]
pub struct UiMessageStreamResponseOptions {
    /// UI-message chunks to encode as server-sent events.
    pub stream: Vec<UiMessageChunk>,

    /// HTTP status code. Defaults to `200`.
    pub status: Option<u16>,

    /// Optional HTTP status text.
    pub status_text: Option<String>,

    /// Optional response headers.
    pub headers: Option<Headers>,
}

impl UiMessageStreamResponseOptions {
    /// Creates options for a UI-message stream.
    pub fn new<I>(stream: I) -> Self
    where
        I: IntoIterator<Item = UiMessageChunk>,
    {
        Self::from_init(stream, UiMessageStreamResponseInit::default())
    }

    /// Creates options for a UI-message stream and response initialization values.
    pub fn from_init<I>(stream: I, init: UiMessageStreamResponseInit) -> Self
    where
        I: IntoIterator<Item = UiMessageChunk>,
    {
        Self {
            stream: stream.into_iter().collect(),
            status: init.status,
            status_text: init.status_text,
            headers: init.headers,
        }
    }

    /// Sets the response status code.
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    /// Sets the response status text.
    pub fn with_status_text(mut self, status_text: impl Into<String>) -> Self {
        self.status_text = Some(status_text.into());
        self
    }

    /// Replaces response headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Adds a response header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Headers::new)
            .insert(name.into(), value.into());
        self
    }
}

/// Collected response returned by [`create_ui_message_stream_response`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiMessageStreamResponse {
    /// HTTP status code.
    pub status: u16,

    /// Optional HTTP status text.
    pub status_text: Option<String>,

    /// Response headers.
    pub headers: Headers,

    /// UTF-8 encoded SSE chunks.
    pub body: Vec<Vec<u8>>,
}

impl UiMessageStreamResponse {
    /// Decodes the UTF-8 body chunks back into strings.
    pub fn decoded_body(&self) -> Result<Vec<String>, std::string::FromUtf8Error> {
        self.body.iter().cloned().map(String::from_utf8).collect()
    }
}

/// Creates a collected response from UI-message chunks.
///
/// This mirrors upstream `createUIMessageStreamResponse`: missing status
/// defaults to `200`, UI-message SSE headers are applied unless already
/// supplied, and each JSON chunk is encoded as an SSE `data:` event followed by
/// a `[DONE]` sentinel.
pub fn create_ui_message_stream_response(
    options: UiMessageStreamResponseOptions,
) -> UiMessageStreamResponse {
    let UiMessageStreamResponseOptions {
        stream,
        status,
        status_text,
        headers,
    } = options;

    UiMessageStreamResponse {
        status: status.unwrap_or(200),
        status_text,
        headers: prepare_ui_message_stream_headers(headers),
        body: encode_ui_message_sse_stream(stream),
    }
}

/// Minimal sink trait used by [`pipe_ui_message_stream_to_response`].
pub trait UiMessageStreamResponseWriter {
    /// Error type returned by the response writer.
    type Error;

    /// Writes response status and headers before body chunks.
    fn write_head(
        &mut self,
        status: u16,
        status_text: Option<&str>,
        headers: &Headers,
    ) -> Result<(), Self::Error>;

    /// Writes one UTF-8 encoded SSE chunk.
    fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), Self::Error>;

    /// Finalizes the response.
    fn end(&mut self) -> Result<(), Self::Error>;
}

/// Pipes UI-message chunks to a server-response writer.
///
/// This mirrors upstream `pipeUIMessageStreamToResponse` without binding this
/// crate to a concrete HTTP framework.
pub fn pipe_ui_message_stream_to_response<W>(
    response: &mut W,
    options: UiMessageStreamResponseOptions,
) -> Result<(), W::Error>
where
    W: UiMessageStreamResponseWriter,
{
    let UiMessageStreamResponseOptions {
        stream,
        status,
        status_text,
        headers,
    } = options;

    let status = status.unwrap_or(200);
    let headers = prepare_ui_message_stream_headers(headers);

    response.write_head(status, status_text.as_deref(), &headers)?;

    for chunk in encode_ui_message_sse_stream(stream) {
        response.write_chunk(&chunk)?;
    }

    response.end()
}

fn prepare_ui_message_stream_headers(headers: Option<Headers>) -> Headers {
    let mut headers = normalize_headers(headers.map(|headers| {
        headers
            .into_iter()
            .map(|(name, value)| (name, Some(value)))
            .collect::<Vec<_>>()
    }));

    headers
        .entry("content-type".to_string())
        .or_insert_with(|| UI_MESSAGE_STREAM_CONTENT_TYPE.to_string());
    headers
        .entry("cache-control".to_string())
        .or_insert_with(|| "no-cache".to_string());
    headers
        .entry("connection".to_string())
        .or_insert_with(|| "keep-alive".to_string());
    headers
        .entry(UI_MESSAGE_STREAM_VERSION_HEADER.to_string())
        .or_insert_with(|| UI_MESSAGE_STREAM_VERSION.to_string());
    headers
        .entry("x-accel-buffering".to_string())
        .or_insert_with(|| "no".to_string());

    headers
}

fn encode_ui_message_sse_stream(stream: Vec<UiMessageChunk>) -> Vec<Vec<u8>> {
    let mut chunks = stream
        .into_iter()
        .map(|chunk| {
            format!(
                "data: {}\n\n",
                serde_json::to_string(&chunk).expect("UI-message chunk serializes")
            )
            .into_bytes()
        })
        .collect::<Vec<_>>();

    chunks.push(b"data: [DONE]\n\n".to_vec());
    chunks
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::convert::Infallible;
    use std::rc::Rc;

    use super::*;
    use serde_json::json;

    #[test]
    fn create_ui_message_stream_response_sets_sse_headers_and_encoded_chunks() {
        let response = create_ui_message_stream_response(
            UiMessageStreamResponseOptions::new([UiMessageChunk::text_delta("1", "test-data")])
                .with_status(201)
                .with_status_text("Created")
                .with_header("Custom-Header", "test"),
        );

        assert_eq!(response.status, 201);
        assert_eq!(response.status_text.as_deref(), Some("Created"));
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some(UI_MESSAGE_STREAM_CONTENT_TYPE)
        );
        assert_eq!(
            response.headers.get("cache-control").map(String::as_str),
            Some("no-cache")
        );
        assert_eq!(
            response.headers.get("connection").map(String::as_str),
            Some("keep-alive")
        );
        assert_eq!(
            response
                .headers
                .get(UI_MESSAGE_STREAM_VERSION_HEADER)
                .map(String::as_str),
            Some(UI_MESSAGE_STREAM_VERSION)
        );
        assert_eq!(
            response
                .headers
                .get("x-accel-buffering")
                .map(String::as_str),
            Some("no")
        );
        assert_eq!(
            response.headers.get("custom-header").map(String::as_str),
            Some("test")
        );
        assert_eq!(
            response.decoded_body().expect("body chunks decode"),
            vec![
                r#"data: {"type":"text-delta","id":"1","delta":"test-data"}

"#
                .to_string(),
                "data: [DONE]\n\n".to_string()
            ]
        );
    }

    #[test]
    fn create_ui_message_stream_response_preserves_existing_headers_and_encodes_errors() {
        let response = create_ui_message_stream_response(
            UiMessageStreamResponseOptions::new([UiMessageChunk::error("Custom error message")])
                .with_header("Content-Type", "application/x-custom")
                .with_header(UI_MESSAGE_STREAM_VERSION_HEADER, "custom-version"),
        );

        assert_eq!(response.status, 200);
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some("application/x-custom")
        );
        assert_eq!(
            response
                .headers
                .get(UI_MESSAGE_STREAM_VERSION_HEADER)
                .map(String::as_str),
            Some("custom-version")
        );
        assert_eq!(
            response.decoded_body().expect("body chunks decode"),
            vec![
                r#"data: {"type":"error","errorText":"Custom error message"}

"#
                .to_string(),
                "data: [DONE]\n\n".to_string()
            ]
        );
    }

    #[test]
    fn pipe_ui_message_stream_to_response_writes_headers_chunks_and_end() {
        let mut response = MockUiMessageStreamResponse::default();

        pipe_ui_message_stream_to_response(
            &mut response,
            UiMessageStreamResponseOptions::new([
                UiMessageChunk::text_start("1"),
                UiMessageChunk::text_delta("1", "test-data"),
                UiMessageChunk::text_end("1"),
            ])
            .with_status(202)
            .with_status_text("Accepted")
            .with_header("Custom-Header", "test"),
        )
        .expect("mock response writes");

        assert_eq!(response.status, Some(202));
        assert_eq!(response.status_text.as_deref(), Some("Accepted"));
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some(UI_MESSAGE_STREAM_CONTENT_TYPE)
        );
        assert_eq!(
            response.headers.get("custom-header").map(String::as_str),
            Some("test")
        );
        assert_eq!(
            response.decoded_chunks(),
            vec![
                r#"data: {"type":"text-start","id":"1"}

"#,
                r#"data: {"type":"text-delta","id":"1","delta":"test-data"}

"#,
                r#"data: {"type":"text-end","id":"1"}

"#,
                "data: [DONE]\n\n"
            ]
        );
        assert!(response.ended);
    }

    #[test]
    fn get_response_ui_message_id_returns_none_without_original_messages() {
        let called = Rc::new(Cell::new(false));
        let called_in_generator = Rc::clone(&called);

        let result = get_response_ui_message_id(
            None,
            ResponseUiMessageId::generate(move || {
                called_in_generator.set(true);
                "new-id".to_string()
            }),
        );

        assert_eq!(result, None);
        assert!(!called.get());
    }

    #[test]
    fn get_response_ui_message_id_reuses_last_assistant_message_id() {
        let called = Rc::new(Cell::new(false));
        let called_in_generator = Rc::clone(&called);
        let original_messages = vec![
            UiMessage::new("user-id", UiMessageRole::User),
            UiMessage::new("assistant-id", UiMessageRole::Assistant),
        ];

        let result = get_response_ui_message_id(
            Some(&original_messages),
            ResponseUiMessageId::generate(move || {
                called_in_generator.set(true);
                "new-id".to_string()
            }),
        );

        assert_eq!(result.as_deref(), Some("assistant-id"));
        assert!(!called.get());
    }

    #[test]
    fn get_response_ui_message_id_generates_when_last_message_is_not_assistant() {
        let original_messages = vec![
            UiMessage::new("assistant-id", UiMessageRole::Assistant),
            UiMessage::new("user-id", UiMessageRole::User),
        ];

        let result = get_response_ui_message_id(
            Some(&original_messages),
            ResponseUiMessageId::generate(|| "new-id".to_string()),
        );

        assert_eq!(result.as_deref(), Some("new-id"));
    }

    #[test]
    fn get_response_ui_message_id_uses_generator_for_empty_messages() {
        let original_messages = Vec::new();

        let result = get_response_ui_message_id(
            Some(&original_messages),
            ResponseUiMessageId::generate(|| "new-id".to_string()),
        );

        assert_eq!(result.as_deref(), Some("new-id"));
    }

    #[test]
    fn get_response_ui_message_id_uses_fixed_response_id() {
        let original_messages = vec![UiMessage::new("user-id", UiMessageRole::User)];

        let result = get_response_ui_message_id(
            Some(&original_messages),
            ResponseUiMessageId::id("fixed-id"),
        );

        assert_eq!(result.as_deref(), Some("fixed-id"));
    }

    #[test]
    fn ui_message_serializes_upstream_minimal_shape() {
        let message = UiMessage::new("message-id", UiMessageRole::Assistant)
            .with_metadata(json!({ "traceId": "trace-1" }))
            .with_part(json!({ "type": "text", "text": "hello" }));

        assert_eq!(
            serde_json::to_value(message).expect("message serializes"),
            json!({
                "id": "message-id",
                "role": "assistant",
                "metadata": { "traceId": "trace-1" },
                "parts": [{ "type": "text", "text": "hello" }]
            })
        );
    }

    #[derive(Default)]
    struct MockUiMessageStreamResponse {
        status: Option<u16>,
        status_text: Option<String>,
        headers: Headers,
        chunks: Vec<Vec<u8>>,
        ended: bool,
    }

    impl MockUiMessageStreamResponse {
        fn decoded_chunks(&self) -> Vec<String> {
            self.chunks
                .iter()
                .map(|chunk| String::from_utf8(chunk.clone()).expect("chunk decodes"))
                .collect()
        }
    }

    impl UiMessageStreamResponseWriter for MockUiMessageStreamResponse {
        type Error = Infallible;

        fn write_head(
            &mut self,
            status: u16,
            status_text: Option<&str>,
            headers: &Headers,
        ) -> Result<(), Self::Error> {
            self.status = Some(status);
            self.status_text = status_text.map(ToString::to_string);
            self.headers = headers.clone();
            Ok(())
        }

        fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), Self::Error> {
            self.chunks.push(chunk.to_vec());
            Ok(())
        }

        fn end(&mut self) -> Result<(), Self::Error> {
            self.ended = true;
            Ok(())
        }
    }
}
