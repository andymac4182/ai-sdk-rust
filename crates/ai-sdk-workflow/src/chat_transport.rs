use std::cmp;
use std::error::Error;
use std::fmt;

use ai_sdk_rust::{Headers, JsonObject, JsonValue, UiMessage, UiMessageChunk};
use serde::{Deserialize, Serialize};

/// Default chat endpoint used by upstream `WorkflowChatTransport`.
pub const DEFAULT_WORKFLOW_CHAT_API: &str = "/api/chat";

/// Transport trigger for a chat request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkflowChatTrigger {
    /// A user submitted a new message.
    SubmitMessage,

    /// The caller requested regeneration for an existing message.
    RegenerateMessage,
}

/// HTTP method used by a workflow chat transport request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkflowChatRequestMethod {
    /// POST initial chat messages.
    Post,

    /// GET a resumable stream.
    Get,
}

/// Request sent by [`WorkflowChatTransport`] through a caller-provided client.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowChatRequest {
    /// HTTP method.
    pub method: WorkflowChatRequestMethod,

    /// Request URL.
    pub url: String,

    /// JSON request body for POST calls.
    pub body: Option<JsonValue>,

    /// Optional request headers.
    pub headers: Option<Headers>,
}

impl WorkflowChatRequest {
    fn post(url: impl Into<String>, body: JsonValue, headers: Option<Headers>) -> Self {
        Self {
            method: WorkflowChatRequestMethod::Post,
            url: url.into(),
            body: Some(body),
            headers,
        }
    }

    fn get(url: impl Into<String>, headers: Option<Headers>) -> Self {
        Self {
            method: WorkflowChatRequestMethod::Get,
            url: url.into(),
            body: None,
            headers,
        }
    }
}

/// Response returned by a workflow chat transport client.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowChatResponse {
    /// HTTP status code.
    pub status: u16,

    /// Plain response body used in error messages.
    pub body: String,

    /// Response headers.
    pub headers: Headers,

    /// Parsed UI-message chunks from the response stream.
    pub chunks: Vec<UiMessageChunk>,
}

impl WorkflowChatResponse {
    /// Creates a successful response with parsed UI-message chunks.
    pub fn ok(chunks: impl IntoIterator<Item = UiMessageChunk>) -> Self {
        Self {
            status: 200,
            body: String::new(),
            headers: Headers::new(),
            chunks: chunks.into_iter().collect(),
        }
    }

    /// Creates a non-success response.
    pub fn status(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
            headers: Headers::new(),
            chunks: Vec::new(),
        }
    }

    /// Adds a response header.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// Client abstraction used by [`WorkflowChatTransport`] for deterministic tests
/// and embedders that want to connect it to an HTTP implementation.
pub trait WorkflowChatTransportClient {
    /// Fetch one workflow chat request.
    fn fetch(
        &mut self,
        request: WorkflowChatRequest,
    ) -> Result<WorkflowChatResponse, WorkflowChatTransportError>;
}

/// Options used when sending chat messages.
#[derive(Clone, Debug, PartialEq)]
pub struct SendMessagesOptions {
    /// Submission trigger.
    pub trigger: WorkflowChatTrigger,

    /// Chat identifier.
    pub chat_id: String,

    /// Optional message identifier.
    pub message_id: Option<String>,

    /// UI messages to send.
    pub messages: Vec<UiMessage>,

    /// Optional request metadata.
    pub metadata: Option<JsonValue>,

    /// Additional JSON body properties to merge into the POST body.
    pub body: Option<JsonValue>,

    /// Optional request headers.
    pub headers: Option<Headers>,
}

impl SendMessagesOptions {
    /// Creates send-message options with no extra metadata.
    pub fn new(
        trigger: WorkflowChatTrigger,
        chat_id: impl Into<String>,
        messages: Vec<UiMessage>,
    ) -> Self {
        Self {
            trigger,
            chat_id: chat_id.into(),
            message_id: None,
            messages,
            metadata: None,
            body: None,
            headers: None,
        }
    }

    /// Sets the message id.
    pub fn with_message_id(mut self, message_id: impl Into<String>) -> Self {
        self.message_id = Some(message_id.into());
        self
    }

    /// Sets request metadata.
    pub fn with_metadata(mut self, metadata: impl Into<JsonValue>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    /// Sets additional JSON body properties.
    pub fn with_body(mut self, body: impl Into<JsonValue>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Sets request headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(headers);
        self
    }
}

/// Options used when reconnecting to a workflow stream.
#[derive(Clone, Debug, PartialEq)]
pub struct ReconnectToStreamOptions {
    /// Chat identifier.
    pub chat_id: String,

    /// Optional request metadata.
    pub metadata: Option<JsonValue>,

    /// Optional initial start index for this reconnect call.
    pub start_index: Option<isize>,

    /// Optional request headers.
    pub headers: Option<Headers>,
}

impl ReconnectToStreamOptions {
    /// Creates reconnect options.
    pub fn new(chat_id: impl Into<String>) -> Self {
        Self {
            chat_id: chat_id.into(),
            metadata: None,
            start_index: None,
            headers: None,
        }
    }

    /// Sets request metadata.
    pub fn with_metadata(mut self, metadata: impl Into<JsonValue>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }

    /// Overrides the first reconnect start index.
    pub fn with_start_index(mut self, start_index: isize) -> Self {
        self.start_index = Some(start_index);
        self
    }

    /// Sets request headers.
    pub fn with_headers(mut self, headers: Headers) -> Self {
        self.headers = Some(headers);
        self
    }
}

/// Successful transport output.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowChatTransportResult {
    /// Chunks yielded by the transport.
    pub chunks: Vec<UiMessageChunk>,

    /// End callback payload, when a finish chunk was received.
    pub chat_end: Option<WorkflowChatEnd>,
}

/// End-of-chat payload mirrored from upstream `onChatEnd`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowChatEnd {
    /// Chat identifier.
    pub chat_id: String,

    /// Total chunks observed by the transport.
    pub chunk_index: usize,
}

/// Workflow chat transport options.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowChatTransportOptions {
    /// API endpoint. Defaults to `/api/chat`.
    pub api: String,

    /// Maximum consecutive reconnect errors. Defaults to 3.
    pub max_consecutive_errors: usize,

    /// Default first reconnect start index. Defaults to 0.
    pub initial_start_index: isize,
}

impl Default for WorkflowChatTransportOptions {
    fn default() -> Self {
        Self {
            api: DEFAULT_WORKFLOW_CHAT_API.to_string(),
            max_consecutive_errors: 3,
            initial_start_index: 0,
        }
    }
}

/// Rust workflow chat transport foundation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkflowChatTransport {
    options: WorkflowChatTransportOptions,
}

impl WorkflowChatTransport {
    /// Creates a transport with default options.
    pub fn new() -> Self {
        Self::with_options(WorkflowChatTransportOptions::default())
    }

    /// Creates a transport with explicit options.
    pub fn with_options(options: WorkflowChatTransportOptions) -> Self {
        Self { options }
    }

    /// Sets the API endpoint.
    pub fn with_api(mut self, api: impl Into<String>) -> Self {
        self.options.api = api.into();
        self
    }

    /// Sets the maximum number of consecutive reconnect errors.
    pub fn with_max_consecutive_errors(mut self, max_consecutive_errors: usize) -> Self {
        self.options.max_consecutive_errors = max_consecutive_errors.max(1);
        self
    }

    /// Sets the default first reconnect start index.
    pub fn with_initial_start_index(mut self, initial_start_index: isize) -> Self {
        self.options.initial_start_index = initial_start_index;
        self
    }

    /// Returns the configured API endpoint.
    pub fn api(&self) -> &str {
        &self.options.api
    }

    /// Returns the configured maximum consecutive reconnect errors.
    pub fn max_consecutive_errors(&self) -> usize {
        self.options.max_consecutive_errors
    }

    /// Returns the configured initial start index.
    pub fn initial_start_index(&self) -> isize {
        self.options.initial_start_index
    }

    /// Builds and sends the initial chat POST request. If the response stream
    /// does not contain a finish chunk, the transport reconnects using the
    /// workflow run id from the response headers.
    pub fn send_messages<C>(
        &self,
        client: &mut C,
        options: SendMessagesOptions,
    ) -> Result<WorkflowChatTransportResult, WorkflowChatTransportError>
    where
        C: WorkflowChatTransportClient,
    {
        let request = self.send_messages_request(&options)?;
        let response = client.fetch(request)?;
        ensure_success(&response)?;

        let workflow_run_id = header_case_insensitive(&response.headers, "x-workflow-run-id")
            .ok_or(WorkflowChatTransportError::MissingWorkflowRunId)?
            .to_string();

        let mut chunks = Vec::new();
        let mut chunk_index = 0;
        let got_finish = append_chunks(&mut chunks, &mut chunk_index, response.chunks);

        if got_finish {
            return Ok(WorkflowChatTransportResult {
                chunks,
                chat_end: Some(WorkflowChatEnd {
                    chat_id: options.chat_id,
                    chunk_index,
                }),
            });
        }

        let reconnect_options = ReconnectToStreamOptions {
            chat_id: options.chat_id,
            metadata: options.metadata,
            start_index: None,
            headers: options.headers,
        };
        let reconnect = self.reconnect_to_stream_from(
            client,
            reconnect_options,
            Some(workflow_run_id),
            chunk_index,
        )?;
        chunks.extend(reconnect.chunks);

        Ok(WorkflowChatTransportResult {
            chunks,
            chat_end: reconnect.chat_end,
        })
    }

    /// Reconnects to an existing chat stream.
    pub fn reconnect_to_stream<C>(
        &self,
        client: &mut C,
        options: ReconnectToStreamOptions,
    ) -> Result<WorkflowChatTransportResult, WorkflowChatTransportError>
    where
        C: WorkflowChatTransportClient,
    {
        self.reconnect_to_stream_from(client, options, None, 0)
    }

    /// Builds the initial send-messages request.
    pub fn send_messages_request(
        &self,
        options: &SendMessagesOptions,
    ) -> Result<WorkflowChatRequest, WorkflowChatTransportError> {
        Ok(WorkflowChatRequest::post(
            self.options.api.clone(),
            send_messages_body(options)?,
            options.headers.clone(),
        ))
    }

    /// Builds one reconnect request for a chat id/run id and start index.
    pub fn reconnect_request(
        &self,
        options: &ReconnectToStreamOptions,
        workflow_run_id: Option<&str>,
        start_index: isize,
    ) -> WorkflowChatRequest {
        WorkflowChatRequest::get(
            format!(
                "{}/{}{}?startIndex={}",
                self.options.api,
                percent_encode_path_segment(workflow_run_id.unwrap_or(&options.chat_id)),
                "/stream",
                start_index
            ),
            options.headers.clone(),
        )
    }

    fn reconnect_to_stream_from<C>(
        &self,
        client: &mut C,
        options: ReconnectToStreamOptions,
        workflow_run_id: Option<String>,
        initial_chunk_index: usize,
    ) -> Result<WorkflowChatTransportResult, WorkflowChatTransportError>
    where
        C: WorkflowChatTransportClient,
    {
        let mut chunks = Vec::new();
        let mut chunk_index = initial_chunk_index;
        let explicit_start_index = options
            .start_index
            .unwrap_or(self.options.initial_start_index);
        let mut use_explicit_start_index = initial_chunk_index == 0 && explicit_start_index != 0;
        let mut replay_from_start = false;
        let mut consecutive_errors = 0usize;

        loop {
            let start_index = if use_explicit_start_index {
                explicit_start_index
            } else if replay_from_start {
                0
            } else {
                saturating_usize_to_isize(chunk_index)
            };

            let request = self.reconnect_request(&options, workflow_run_id.as_deref(), start_index);
            let response = match client.fetch(request) {
                Ok(response) => response,
                Err(error) => {
                    consecutive_errors += 1;
                    if consecutive_errors >= self.options.max_consecutive_errors {
                        return Err(WorkflowChatTransportError::ReconnectFailed {
                            max_consecutive_errors: self.options.max_consecutive_errors,
                            last_error: error.to_string(),
                        });
                    }
                    continue;
                }
            };
            ensure_success(&response)?;

            if use_explicit_start_index && explicit_start_index > 0 {
                chunk_index = explicit_start_index as usize;
            } else if use_explicit_start_index && explicit_start_index < 0 {
                if let Some(tail_index) = response_tail_index(&response.headers) {
                    let resolved = tail_index + 1 + explicit_start_index;
                    chunk_index = cmp::max(0, resolved) as usize;
                } else {
                    replay_from_start = true;
                }
            }
            use_explicit_start_index = false;

            let got_finish = append_chunks(&mut chunks, &mut chunk_index, response.chunks);
            consecutive_errors = 0;

            if got_finish {
                return Ok(WorkflowChatTransportResult {
                    chunks,
                    chat_end: Some(WorkflowChatEnd {
                        chat_id: options.chat_id,
                        chunk_index,
                    }),
                });
            }
        }
    }
}

impl Default for WorkflowChatTransport {
    fn default() -> Self {
        Self::new()
    }
}

/// Error returned by workflow chat transport operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkflowChatTransportError {
    /// Client-level fetch error.
    Fetch(String),

    /// HTTP status error.
    Http { status: u16, body: String },

    /// The initial response did not include a workflow run id header.
    MissingWorkflowRunId,

    /// Reconnection exceeded the configured consecutive error limit.
    ReconnectFailed {
        max_consecutive_errors: usize,
        last_error: String,
    },

    /// UI messages could not be serialized into JSON.
    SerializeMessages(String),
}

impl WorkflowChatTransportError {
    /// Creates a fetch error.
    pub fn fetch(message: impl Into<String>) -> Self {
        Self::Fetch(message.into())
    }
}

impl fmt::Display for WorkflowChatTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fetch(message) => formatter.write_str(message),
            Self::Http { status, body } => {
                write!(formatter, "Failed to fetch chat: {status} {body}")
            }
            Self::MissingWorkflowRunId => formatter
                .write_str("Workflow run ID not found in \"x-workflow-run-id\" response header"),
            Self::ReconnectFailed {
                max_consecutive_errors,
                last_error,
            } => write!(
                formatter,
                "Failed to reconnect after {max_consecutive_errors} consecutive errors. Last error: {last_error}"
            ),
            Self::SerializeMessages(message) => {
                write!(formatter, "failed to serialize chat messages: {message}")
            }
        }
    }
}

impl Error for WorkflowChatTransportError {}

fn ensure_success(response: &WorkflowChatResponse) -> Result<(), WorkflowChatTransportError> {
    if response.is_success() {
        Ok(())
    } else {
        Err(WorkflowChatTransportError::Http {
            status: response.status,
            body: response.body.clone(),
        })
    }
}

fn send_messages_body(
    options: &SendMessagesOptions,
) -> Result<JsonValue, WorkflowChatTransportError> {
    let mut body = JsonObject::new();
    body.insert(
        "messages".to_string(),
        serde_json::to_value(&options.messages)
            .map_err(|error| WorkflowChatTransportError::SerializeMessages(error.to_string()))?,
    );

    if let Some(JsonValue::Object(extra)) = &options.body {
        body.extend(extra.clone());
    }

    Ok(JsonValue::Object(body))
}

fn append_chunks(
    target: &mut Vec<UiMessageChunk>,
    chunk_index: &mut usize,
    chunks: Vec<UiMessageChunk>,
) -> bool {
    let mut got_finish = false;

    for chunk in chunks {
        *chunk_index += 1;
        if matches!(chunk, UiMessageChunk::Finish { .. }) {
            got_finish = true;
        }
        target.push(chunk);
    }

    got_finish
}

fn header_case_insensitive<'a>(headers: &'a Headers, name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

fn response_tail_index(headers: &Headers) -> Option<isize> {
    header_case_insensitive(headers, "x-workflow-stream-tail-index")?
        .parse()
        .ok()
}

fn saturating_usize_to_isize(value: usize) -> isize {
    isize::try_from(value).unwrap_or(isize::MAX)
}

fn percent_encode_path_segment(segment: &str) -> String {
    let mut encoded = String::new();

    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }

    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Default)]
    struct ScriptedWorkflowChatClient {
        requests: Vec<WorkflowChatRequest>,
        responses: Vec<Result<WorkflowChatResponse, WorkflowChatTransportError>>,
    }

    impl ScriptedWorkflowChatClient {
        fn new(responses: impl IntoIterator<Item = WorkflowChatResponse>) -> Self {
            Self {
                requests: Vec::new(),
                responses: responses.into_iter().map(Ok).collect(),
            }
        }

        fn with_errors(
            responses: impl IntoIterator<
                Item = Result<WorkflowChatResponse, WorkflowChatTransportError>,
            >,
        ) -> Self {
            Self {
                requests: Vec::new(),
                responses: responses.into_iter().collect(),
            }
        }
    }

    impl WorkflowChatTransportClient for ScriptedWorkflowChatClient {
        fn fetch(
            &mut self,
            request: WorkflowChatRequest,
        ) -> Result<WorkflowChatResponse, WorkflowChatTransportError> {
            self.requests.push(request);
            if self.responses.is_empty() {
                return Err(WorkflowChatTransportError::fetch("no scripted response"));
            }
            self.responses.remove(0)
        }
    }

    #[test]
    fn workflow_chat_transport_uses_default_options_and_builds_send_request() {
        let transport = WorkflowChatTransport::new();
        let messages = vec![
            UiMessage::new("msg-1", ai_sdk_rust::UiMessageRole::User)
                .with_part(json!({ "type": "text", "text": "hello" })),
        ];
        let options = SendMessagesOptions::new(
            WorkflowChatTrigger::SubmitMessage,
            "chat-1",
            messages.clone(),
        )
        .with_body(json!({ "custom": "body" }));

        assert_eq!(transport.api(), DEFAULT_WORKFLOW_CHAT_API);
        assert_eq!(transport.max_consecutive_errors(), 3);
        assert_eq!(transport.initial_start_index(), 0);
        assert_eq!(
            transport
                .send_messages_request(&options)
                .expect("request builds"),
            WorkflowChatRequest::post(
                "/api/chat",
                json!({
                    "messages": messages,
                    "custom": "body"
                }),
                None,
            )
        );
    }

    #[test]
    fn workflow_chat_transport_sends_messages_and_reports_chat_end() {
        let transport = WorkflowChatTransport::new();
        let mut client = ScriptedWorkflowChatClient::new([WorkflowChatResponse::ok([
            UiMessageChunk::text_delta("text-1", "hello"),
            UiMessageChunk::finish(),
        ])
        .with_header("x-workflow-run-id", "run-1")]);

        let result = transport
            .send_messages(
                &mut client,
                SendMessagesOptions::new(WorkflowChatTrigger::SubmitMessage, "chat-1", Vec::new()),
            )
            .expect("messages send");

        assert_eq!(
            client.requests,
            vec![WorkflowChatRequest::post(
                "/api/chat",
                json!({ "messages": [] }),
                None,
            )]
        );
        assert_eq!(
            result.chat_end,
            Some(WorkflowChatEnd {
                chat_id: "chat-1".to_string(),
                chunk_index: 2,
            })
        );
    }

    #[test]
    fn workflow_chat_transport_requires_workflow_run_id_for_interrupted_send() {
        let transport = WorkflowChatTransport::new();
        let mut client = ScriptedWorkflowChatClient::new([WorkflowChatResponse::ok([
            UiMessageChunk::text_delta("text-1", "hello"),
        ])]);

        let error = transport
            .send_messages(
                &mut client,
                SendMessagesOptions::new(WorkflowChatTrigger::SubmitMessage, "chat-1", Vec::new()),
            )
            .expect_err("missing workflow run id is an error");

        assert_eq!(error, WorkflowChatTransportError::MissingWorkflowRunId);
    }

    #[test]
    fn workflow_chat_transport_reconnects_after_interrupted_send_using_run_id_and_chunk_index() {
        let transport = WorkflowChatTransport::new();
        let mut client = ScriptedWorkflowChatClient::new([
            WorkflowChatResponse::ok([UiMessageChunk::text_delta("text-1", "hello")])
                .with_header("x-workflow-run-id", "run-1"),
            WorkflowChatResponse::ok([UiMessageChunk::finish()]),
        ]);

        let result = transport
            .send_messages(
                &mut client,
                SendMessagesOptions::new(WorkflowChatTrigger::SubmitMessage, "chat-1", Vec::new()),
            )
            .expect("interrupted send reconnects");

        assert_eq!(
            client.requests[1],
            WorkflowChatRequest::get("/api/chat/run-1/stream?startIndex=1", None)
        );
        assert_eq!(result.chunks.len(), 2);
        assert_eq!(
            result.chat_end,
            Some(WorkflowChatEnd {
                chat_id: "chat-1".to_string(),
                chunk_index: 2,
            })
        );
    }

    #[test]
    fn workflow_chat_transport_reconnect_uses_positive_initial_start_index_for_retries() {
        let transport = WorkflowChatTransport::new().with_initial_start_index(100);
        let mut client = ScriptedWorkflowChatClient::new([
            WorkflowChatResponse::ok(Vec::new()),
            WorkflowChatResponse::ok([UiMessageChunk::finish()]),
        ]);

        transport
            .reconnect_to_stream(&mut client, ReconnectToStreamOptions::new("chat-1"))
            .expect("reconnect completes");

        assert_eq!(
            client.requests,
            vec![
                WorkflowChatRequest::get("/api/chat/chat-1/stream?startIndex=100", None),
                WorkflowChatRequest::get("/api/chat/chat-1/stream?startIndex=100", None),
            ]
        );
    }

    #[test]
    fn workflow_chat_transport_reconnect_resolves_negative_start_index_from_tail_header() {
        let transport = WorkflowChatTransport::new().with_initial_start_index(-20);
        let mut client = ScriptedWorkflowChatClient::new([
            WorkflowChatResponse::ok(Vec::new()).with_header("x-workflow-stream-tail-index", "499"),
            WorkflowChatResponse::ok([UiMessageChunk::finish()]),
        ]);

        transport
            .reconnect_to_stream(&mut client, ReconnectToStreamOptions::new("chat-1"))
            .expect("reconnect completes");

        assert_eq!(
            client.requests,
            vec![
                WorkflowChatRequest::get("/api/chat/chat-1/stream?startIndex=-20", None),
                WorkflowChatRequest::get("/api/chat/chat-1/stream?startIndex=480", None),
            ]
        );
    }

    #[test]
    fn workflow_chat_transport_reconnect_falls_back_to_zero_for_invalid_negative_tail_header() {
        let transport = WorkflowChatTransport::new().with_initial_start_index(-10);
        let mut client = ScriptedWorkflowChatClient::new([
            WorkflowChatResponse::ok(Vec::new())
                .with_header("x-workflow-stream-tail-index", "not-a-number"),
            WorkflowChatResponse::ok([UiMessageChunk::finish()]),
        ]);

        transport
            .reconnect_to_stream(&mut client, ReconnectToStreamOptions::new("chat-1"))
            .expect("reconnect completes");

        assert_eq!(
            client.requests,
            vec![
                WorkflowChatRequest::get("/api/chat/chat-1/stream?startIndex=-10", None),
                WorkflowChatRequest::get("/api/chat/chat-1/stream?startIndex=0", None),
            ]
        );
    }

    #[test]
    fn workflow_chat_transport_reconnect_formats_consecutive_errors() {
        let transport = WorkflowChatTransport::new().with_max_consecutive_errors(2);
        let mut client = ScriptedWorkflowChatClient::with_errors([
            Err(WorkflowChatTransportError::fetch(
                "temporary object-like error",
            )),
            Err(WorkflowChatTransportError::fetch("still failing")),
        ]);

        let error = transport
            .reconnect_to_stream(&mut client, ReconnectToStreamOptions::new("chat-1"))
            .expect_err("consecutive errors fail");

        assert_eq!(
            error.to_string(),
            "Failed to reconnect after 2 consecutive errors. Last error: still failing"
        );
    }

    #[test]
    fn workflow_chat_transport_reports_http_errors() {
        let transport = WorkflowChatTransport::new();
        let mut client = ScriptedWorkflowChatClient::new([WorkflowChatResponse::status(
            500,
            "Internal Server Error",
        )]);

        let error = transport
            .reconnect_to_stream(&mut client, ReconnectToStreamOptions::new("chat-1"))
            .expect_err("http error propagates");

        assert_eq!(
            error,
            WorkflowChatTransportError::Http {
                status: 500,
                body: "Internal Server Error".to_string(),
            }
        );
    }
}
