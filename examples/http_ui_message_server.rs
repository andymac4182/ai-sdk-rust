use std::env;
use std::fmt;
use std::future::Future;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::thread;

use ai_sdk_rust::{
    FinishReason, Headers, InputTokenUsage, LanguageModelFinishReason, LanguageModelStreamFinish,
    LanguageModelStreamPart, LanguageModelStreamResult, LanguageModelTextDelta,
    LanguageModelTextEnd, LanguageModelTextStart, LanguageModelUsage, MockLanguageModel,
    OutputTokenUsage, Prompt, StreamTextOptions, StreamTextResult,
    StreamTextUiMessageStreamOptions, TextStreamResponse, TextStreamResponseInit,
    UI_MESSAGE_STREAM_VERSION, UI_MESSAGE_STREAM_VERSION_HEADER, UiMessageChunk,
    UiMessageStreamResponse, UiMessageStreamResponseInit, UiMessageStreamResponseOptions,
    UiMessageStreamResponseWriter, create_ui_message_stream_response, stream_text,
};
use serde_json::json;

fn main() {
    let mode = env::args()
        .nth(1)
        .unwrap_or_else(|| "--self-test".to_string());

    match mode.as_str() {
        "--serve" => {
            let listener = TcpListener::bind("127.0.0.1:8080").expect("bind example server");
            println!("listening on http://127.0.0.1:8080");
            run_server(listener, None).expect("server runs");
        }
        "--self-test" => run_self_test(),
        _ => {
            eprintln!("usage: cargo run --example http_ui_message_server -- [--self-test|--serve]");
            std::process::exit(2);
        }
    }
}

fn run_self_test() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind self-test server");
    let address = listener.local_addr().expect("self-test address");
    let server = thread::spawn(move || run_server(listener, Some(4)).expect("self-test server"));

    let ui_response = request(address, "POST", "/");
    assert_eq!(ui_response.status, 200);
    assert_header(
        &ui_response,
        "content-type",
        ai_sdk_rust::UI_MESSAGE_STREAM_CONTENT_TYPE,
    );
    assert_header(
        &ui_response,
        UI_MESSAGE_STREAM_VERSION_HEADER,
        UI_MESSAGE_STREAM_VERSION,
    );
    assert!(ui_response.body.contains("\"type\":\"text-delta\""));
    assert!(ui_response.body.contains("Founders' Day"));

    let pipe_response = request(address, "POST", "/pipe");
    assert_eq!(pipe_response.status, 200);
    assert_header(
        &pipe_response,
        "content-type",
        ai_sdk_rust::UI_MESSAGE_STREAM_CONTENT_TYPE,
    );
    assert!(
        pipe_response.body.contains("Lantern routes"),
        "unexpected /pipe body: {}",
        pipe_response.body
    );

    let text_response = request(address, "POST", "/text");
    assert_eq!(text_response.status, 200);
    assert_header(
        &text_response,
        "content-type",
        ai_sdk_rust::TEXT_STREAM_CONTENT_TYPE,
    );
    assert_eq!(text_response.body, "Rust streams steady text.");

    let data_response = request(address, "POST", "/stream-data");
    assert_eq!(data_response.status, 200);
    assert!(data_response.body.contains("\"type\":\"data-custom\""));
    assert!(data_response.body.contains("\"custom\":\"Hello, world!\""));
    assert!(data_response.body.contains("custom stream"));

    server.join().expect("self-test server joins");
    println!("HTTP UI-message server self-test passed at http://{address}");
}

fn run_server(listener: TcpListener, max_requests: Option<usize>) -> std::io::Result<()> {
    for (count, stream) in listener.incoming().enumerate() {
        handle_connection(stream?)?;
        if max_requests.is_some_and(|max| count + 1 >= max) {
            break;
        }
    }
    Ok(())
}

fn handle_connection(mut stream: TcpStream) -> std::io::Result<()> {
    let request = read_request(&mut stream)?;
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => write_html_form(&mut stream),
        ("GET", "/health") => write_plain_response(&mut stream, 200, "OK", "server running"),
        ("POST", "/") => {
            let result = deterministic_stream_text(
                "Invent a new holiday and describe its traditions.",
                "Founders' Day has lanterns and shared recipes.",
            );
            let response = result.to_ui_message_stream_response(UiMessageStreamResponseInit::new());
            write_ui_response(&mut stream, response)
        }
        ("POST", "/pipe") => {
            let result = deterministic_stream_text(
                "Invent a new holiday and describe its traditions.",
                "Lantern routes mark the new holiday parade.",
            );
            let mut response = HttpStreamResponseWriter::new(&mut stream);
            result.pipe_ui_message_stream_to_response(
                &mut response,
                UiMessageStreamResponseInit::new(),
            )
        }
        ("POST", "/text") => {
            let result = deterministic_stream_text(
                "Write a short poem about coding.",
                "Rust streams steady text.",
            );
            let response = result.to_text_stream_response(TextStreamResponseInit::new());
            write_text_response(&mut stream, response)
        }
        ("POST", "/stream-data") => {
            let result = deterministic_stream_text(
                "Invent a new holiday and describe its traditions.",
                "The custom stream finishes with model text.",
            );
            let mut chunks = vec![
                UiMessageChunk::start(),
                UiMessageChunk::data("data-custom", json!({ "custom": "Hello, world!" })),
            ];
            chunks.extend(result.to_ui_message_stream_with_options(
                StreamTextUiMessageStreamOptions::new().with_send_start(false),
            ));
            let response =
                create_ui_message_stream_response(UiMessageStreamResponseOptions::new(chunks));
            write_ui_response(&mut stream, response)
        }
        _ => write_plain_response(&mut stream, 404, "Not Found", "not found"),
    }
}

fn deterministic_stream_text(prompt: &str, text: &str) -> StreamTextResult {
    let model = MockLanguageModel::new().with_stream_result(LanguageModelStreamResult::new(vec![
        LanguageModelStreamPart::TextStart(LanguageModelTextStart::new("text-1")),
        LanguageModelStreamPart::TextDelta(LanguageModelTextDelta::new("text-1", text)),
        LanguageModelStreamPart::TextEnd(LanguageModelTextEnd::new("text-1")),
        LanguageModelStreamPart::Finish(LanguageModelStreamFinish::new(usage(), finish_reason())),
    ]));

    poll_ready(stream_text(
        StreamTextOptions::from_prompt(&model, Prompt::from_prompt(prompt))
            .expect("prompt standardizes"),
    ))
}

fn usage() -> LanguageModelUsage {
    LanguageModelUsage {
        input_tokens: InputTokenUsage {
            total: Some(8),
            ..InputTokenUsage::default()
        },
        output_tokens: OutputTokenUsage {
            total: Some(6),
            text: Some(6),
            ..OutputTokenUsage::default()
        },
        raw: None,
    }
}

fn finish_reason() -> LanguageModelFinishReason {
    LanguageModelFinishReason {
        unified: FinishReason::Stop,
        raw: Some("stop".to_string()),
    }
}

struct HttpRequest {
    method: String,
    path: String,
}

fn read_request(stream: &mut TcpStream) -> std::io::Result<HttpRequest> {
    let mut request_bytes = Vec::new();
    let mut buffer = [0_u8; 1024];
    loop {
        let bytes_read = stream.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        request_bytes.extend_from_slice(&buffer[..bytes_read]);
        if request_bytes.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let request = String::from_utf8_lossy(&request_bytes);
    let first_line = request.lines().next().unwrap_or_default();
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or("/").to_string();
    Ok(HttpRequest { method, path })
}

fn write_html_form(stream: &mut TcpStream) -> std::io::Result<()> {
    write_plain_response(
        stream,
        200,
        "OK",
        "<html><body><form method=\"POST\"><button type=\"submit\">Invent a new holiday</button></form></body></html>",
    )
}

fn write_plain_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    body: &str,
) -> std::io::Result<()> {
    let headers = Headers::from([
        (
            "content-type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        ),
        ("content-length".to_string(), body.len().to_string()),
        ("connection".to_string(), "close".to_string()),
    ]);
    write_http_response(
        stream,
        status,
        Some(reason),
        &headers,
        &[body.as_bytes().to_vec()],
    )
}

fn write_ui_response(
    stream: &mut TcpStream,
    response: UiMessageStreamResponse,
) -> std::io::Result<()> {
    write_http_response(
        stream,
        response.status,
        response.status_text.as_deref(),
        &response.headers,
        &response.body,
    )
}

fn write_text_response(
    stream: &mut TcpStream,
    response: TextStreamResponse,
) -> std::io::Result<()> {
    write_http_response(
        stream,
        response.status,
        response.status_text.as_deref(),
        &response.headers,
        &response.body,
    )
}

fn write_http_response(
    stream: &mut TcpStream,
    status: u16,
    status_text: Option<&str>,
    headers: &Headers,
    body: &[Vec<u8>],
) -> std::io::Result<()> {
    let status_text = status_text.unwrap_or_else(|| default_status_text(status));
    write!(stream, "HTTP/1.1 {status} {status_text}\r\n")?;
    let body_len = body.iter().map(Vec::len).sum::<usize>();
    let has_content_length = headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("content-length"));
    let has_connection = headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("connection"));
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    if !has_content_length {
        write!(stream, "content-length: {body_len}\r\n")?;
    }
    if !has_connection {
        write!(stream, "connection: close\r\n")?;
    }
    write!(stream, "\r\n")?;
    for chunk in body {
        stream.write_all(chunk)?;
    }
    stream.flush()
}

fn default_status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        404 => "Not Found",
        _ => "OK",
    }
}

struct HttpStreamResponseWriter<'a> {
    stream: &'a mut TcpStream,
    status: Option<u16>,
    status_text: Option<String>,
    headers: Headers,
    body: Vec<Vec<u8>>,
}

impl<'a> HttpStreamResponseWriter<'a> {
    fn new(stream: &'a mut TcpStream) -> Self {
        Self {
            stream,
            status: None,
            status_text: None,
            headers: Headers::new(),
            body: Vec::new(),
        }
    }
}

impl UiMessageStreamResponseWriter for HttpStreamResponseWriter<'_> {
    type Error = std::io::Error;

    fn write_head(
        &mut self,
        status: u16,
        status_text: Option<&str>,
        headers: &Headers,
    ) -> Result<(), Self::Error> {
        self.status = Some(status);
        self.status_text = status_text.map(ToString::to_string);
        self.headers = headers.clone();
        self.headers
            .insert("connection".to_string(), "close".to_string());
        Ok(())
    }

    fn write_chunk(&mut self, chunk: &[u8]) -> Result<(), Self::Error> {
        self.body.push(chunk.to_vec());
        Ok(())
    }

    fn end(&mut self) -> Result<(), Self::Error> {
        write_http_response(
            self.stream,
            self.status.unwrap_or(200),
            self.status_text.as_deref(),
            &self.headers,
            &self.body,
        )
    }
}

struct HttpResponse {
    status: u16,
    headers: Headers,
    body: String,
}

fn request(address: SocketAddr, method: &str, path: &str) -> HttpResponse {
    let mut stream = TcpStream::connect(address).expect("connect to self-test server");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nhost: {address}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
    )
    .expect("write request");
    stream.flush().expect("flush request");

    let mut response = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(bytes_read) => response.extend_from_slice(&buffer[..bytes_read]),
            Err(error)
                if error.kind() == std::io::ErrorKind::ConnectionReset && !response.is_empty() =>
            {
                break;
            }
            Err(error) => panic!("read response: {error}"),
        }
    }
    parse_response(&response).unwrap_or_else(|error| {
        panic!(
            "parse response for {method} {path}: {error}; raw response: {}",
            String::from_utf8_lossy(&response)
        )
    })
}

fn parse_response(bytes: &[u8]) -> Result<HttpResponse, ParseHttpResponseError> {
    let response = String::from_utf8(bytes.to_vec())?;
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or(ParseHttpResponseError::MissingHeaderTerminator)?;
    let mut lines = head.lines();
    let status_line = lines.next().ok_or(ParseHttpResponseError::MissingStatus)?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or(ParseHttpResponseError::MissingStatus)?
        .parse::<u16>()
        .map_err(|_| ParseHttpResponseError::InvalidStatus)?;
    let mut headers = Headers::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    Ok(HttpResponse {
        status,
        headers,
        body: body.to_string(),
    })
}

fn assert_header(response: &HttpResponse, name: &str, expected: &str) {
    assert_eq!(
        response
            .headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str),
        Some(expected)
    );
}

#[derive(Debug)]
enum ParseHttpResponseError {
    Utf8(std::string::FromUtf8Error),
    MissingHeaderTerminator,
    MissingStatus,
    InvalidStatus,
}

impl From<std::string::FromUtf8Error> for ParseHttpResponseError {
    fn from(error: std::string::FromUtf8Error) -> Self {
        Self::Utf8(error)
    }
}

impl fmt::Display for ParseHttpResponseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Utf8(error) => write!(formatter, "{error}"),
            Self::MissingHeaderTerminator => write!(formatter, "missing HTTP header terminator"),
            Self::MissingStatus => write!(formatter, "missing HTTP status"),
            Self::InvalidStatus => write!(formatter, "invalid HTTP status"),
        }
    }
}

impl std::error::Error for ParseHttpResponseError {}

fn poll_ready<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = Box::pin(future);

    match Pin::new(&mut future).poll(&mut context) {
        Poll::Ready(value) => value,
        Poll::Pending => unreachable!("example uses only ready futures"),
    }
}
