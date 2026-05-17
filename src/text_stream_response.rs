use crate::headers::Headers;
use crate::provider_utils::normalize_headers;

/// Default content type used by upstream text-stream response helpers.
pub const TEXT_STREAM_CONTENT_TYPE: &str = "text/plain; charset=utf-8";

/// Options shared by text-stream response helpers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextStreamResponseOptions {
    /// Text chunks to encode as UTF-8 response chunks.
    pub text_stream: Vec<String>,

    /// HTTP status code. Defaults to `200`.
    pub status: Option<u16>,

    /// Optional HTTP status text.
    pub status_text: Option<String>,

    /// Optional response headers.
    pub headers: Option<Headers>,
}

impl TextStreamResponseOptions {
    /// Creates options for a text stream.
    pub fn new<I, S>(text_stream: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            text_stream: text_stream.into_iter().map(Into::into).collect(),
            status: None,
            status_text: None,
            headers: None,
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

/// Collected response returned by [`create_text_stream_response`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextStreamResponse {
    /// HTTP status code.
    pub status: u16,

    /// Optional HTTP status text.
    pub status_text: Option<String>,

    /// Response headers.
    pub headers: Headers,

    /// UTF-8 encoded text chunks.
    pub body: Vec<Vec<u8>>,
}

impl TextStreamResponse {
    /// Decodes the UTF-8 body chunks back into strings.
    pub fn decoded_body(&self) -> Result<Vec<String>, std::string::FromUtf8Error> {
        self.body.iter().cloned().map(String::from_utf8).collect()
    }
}

/// Creates a collected response from text chunks.
///
/// This mirrors upstream `createTextStreamResponse`: missing status defaults to
/// `200`, default content type is applied unless already supplied, and each
/// text chunk is encoded separately as UTF-8.
pub fn create_text_stream_response(options: TextStreamResponseOptions) -> TextStreamResponse {
    let TextStreamResponseOptions {
        text_stream,
        status,
        status_text,
        headers,
    } = options;

    TextStreamResponse {
        status: status.unwrap_or(200),
        status_text,
        headers: prepare_text_stream_headers(headers),
        body: encode_text_stream(text_stream),
    }
}

/// Minimal sink trait used by [`pipe_text_stream_to_response`].
pub trait TextStreamResponseWriter {
    /// Error type returned by the response writer.
    type Error;

    /// Writes response status and headers before body chunks.
    fn write_head(
        &mut self,
        status: u16,
        status_text: Option<&str>,
        headers: &Headers,
    ) -> Result<(), Self::Error>;

    /// Writes one UTF-8 encoded text chunk.
    fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), Self::Error>;

    /// Finalizes the response.
    fn end(&mut self) -> Result<(), Self::Error>;
}

/// Pipes text chunks to a server-response writer.
///
/// This mirrors upstream `pipeTextStreamToResponse` without binding this crate
/// to a concrete HTTP framework.
pub fn pipe_text_stream_to_response<W>(
    response: &mut W,
    options: TextStreamResponseOptions,
) -> Result<(), W::Error>
where
    W: TextStreamResponseWriter,
{
    let TextStreamResponseOptions {
        text_stream,
        status,
        status_text,
        headers,
    } = options;

    let status = status.unwrap_or(200);
    let headers = prepare_text_stream_headers(headers);

    response.write_head(status, status_text.as_deref(), &headers)?;

    for chunk in encode_text_stream(text_stream) {
        response.write_chunk(&chunk)?;
    }

    response.end()
}

fn prepare_text_stream_headers(headers: Option<Headers>) -> Headers {
    let mut headers = normalize_headers(headers.map(|headers| {
        headers
            .into_iter()
            .map(|(name, value)| (name, Some(value)))
            .collect::<Vec<_>>()
    }));

    headers
        .entry("content-type".to_string())
        .or_insert_with(|| TEXT_STREAM_CONTENT_TYPE.to_string());

    headers
}

fn encode_text_stream(text_stream: Vec<String>) -> Vec<Vec<u8>> {
    text_stream.into_iter().map(String::into_bytes).collect()
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use super::*;

    #[test]
    fn create_text_stream_response_sets_headers_status_and_encoded_chunks() {
        let response = create_text_stream_response(
            TextStreamResponseOptions::new(["test-data"])
                .with_status(201)
                .with_status_text("Created")
                .with_header("Custom-Header", "test"),
        );

        assert_eq!(response.status, 201);
        assert_eq!(response.status_text.as_deref(), Some("Created"));
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some(TEXT_STREAM_CONTENT_TYPE)
        );
        assert_eq!(
            response.headers.get("custom-header").map(String::as_str),
            Some("test")
        );
        assert_eq!(
            response.decoded_body().expect("body chunks decode"),
            vec!["test-data".to_string()]
        );
    }

    #[test]
    fn create_text_stream_response_preserves_existing_content_type_and_defaults_status() {
        let response = create_text_stream_response(
            TextStreamResponseOptions::new(["event"])
                .with_header("Content-Type", "text/event-stream"),
        );

        assert_eq!(response.status, 200);
        assert_eq!(response.status_text, None);
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some("text/event-stream")
        );
    }

    #[test]
    fn pipe_text_stream_to_response_writes_headers_chunks_and_end() {
        let mut response = MockTextStreamResponse::default();

        pipe_text_stream_to_response(
            &mut response,
            TextStreamResponseOptions::new(["hello", " ", "world"])
                .with_status(202)
                .with_status_text("Accepted")
                .with_header("Custom-Header", "test"),
        )
        .expect("mock response writes");

        assert_eq!(response.status, Some(202));
        assert_eq!(response.status_text.as_deref(), Some("Accepted"));
        assert_eq!(
            response.headers.get("content-type").map(String::as_str),
            Some(TEXT_STREAM_CONTENT_TYPE)
        );
        assert_eq!(
            response.headers.get("custom-header").map(String::as_str),
            Some("test")
        );
        assert_eq!(response.decoded_chunks(), vec!["hello", " ", "world"]);
        assert!(response.ended);
    }

    #[derive(Default)]
    struct MockTextStreamResponse {
        status: Option<u16>,
        status_text: Option<String>,
        headers: Headers,
        chunks: Vec<Vec<u8>>,
        ended: bool,
    }

    impl MockTextStreamResponse {
        fn decoded_chunks(&self) -> Vec<String> {
            self.chunks
                .iter()
                .map(|chunk| String::from_utf8(chunk.clone()).expect("chunk decodes"))
                .collect()
        }
    }

    impl TextStreamResponseWriter for MockTextStreamResponse {
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
