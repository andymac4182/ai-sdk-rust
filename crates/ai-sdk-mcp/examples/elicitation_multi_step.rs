use std::error::Error;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};

use ai_sdk_mcp::{
    CallToolResult, ClientCapabilities, Configuration, ElicitAction, ElicitResult,
    ElicitationCapability, InitializeResult, JsonRpcError, JsonRpcMessage, JsonRpcNotification,
    JsonRpcRequest, JsonRpcResponse, LATEST_PROTOCOL_VERSION, ListToolsResult, McpCallToolRequest,
    McpClientConfig, McpClientError, McpClientResult, McpTool, McpTransport, ServerCapabilities,
    create_mcp_client,
};
use ai_sdk_provider::{JsonObject, JsonSchema, JsonValue};
use ai_sdk_provider_utils::ToolExecutionOptions;
use serde_json::json;

fn main() -> Result<(), Box<dyn Error>> {
    let transport = MultiStepElicitationTransport::default();
    let handle = transport.clone();
    let client = create_mcp_client(
        McpClientConfig::new(transport)
            .with_client_name("rust-elicitation-multi-step-example")
            .with_capabilities(ClientCapabilities {
                elicitation: Some(ElicitationCapability {
                    apply_defaults: Some(true),
                    ..ElicitationCapability::default()
                }),
                ..ClientCapabilities::default()
            }),
    )?;

    client.on_elicitation_request(|request| {
        println!("elicitation request: {}", request.params.message);
        match request.params.message.as_str() {
            "Step 1: Enter basic event information" => Ok(ElicitResult {
                action: ElicitAction::Accept,
                content: Some(JsonObject::from_iter([
                    ("title".to_string(), json!("Design review")),
                    (
                        "description".to_string(),
                        json!("Review the next SDK parity slice"),
                    ),
                ])),
                meta: None,
            }),
            "Step 2: Enter date and time" => Ok(ElicitResult {
                action: ElicitAction::Accept,
                content: Some(JsonObject::from_iter([
                    ("date".to_string(), json!("2026-05-21")),
                    ("startTime".to_string(), json!("09:30")),
                    ("duration".to_string(), json!(45)),
                ])),
                meta: None,
            }),
            message => Err(McpClientError::new(format!(
                "unexpected elicitation request: {message}"
            ))),
        }
    })?;

    let tools = client.tools()?;
    let output = block_on(
        tools["create_event"]
            .execute(
                json!({}),
                ToolExecutionOptions::new("create-event-1", Vec::new()),
            )
            .expect("create_event tool is executable"),
    )?;
    assert_eq!(
        output
            .get("content")
            .and_then(JsonValue::as_array)
            .and_then(|content| content.first())
            .and_then(|part| part.get("text"))
            .and_then(JsonValue::as_str),
        Some(
            "Event created successfully!\n\n{\"date\":\"2026-05-21\",\"description\":\"Review the next SDK parity slice\",\"duration\":45,\"startTime\":\"09:30\",\"title\":\"Design review\"}"
        )
    );

    let elicitation_responses = handle.elicitation_responses()?;
    assert_eq!(elicitation_responses.len(), 2);
    assert_response_content(
        &elicitation_responses[0],
        json!({
            "title": "Design review",
            "description": "Review the next SDK parity slice"
        }),
    );
    assert_response_content(
        &elicitation_responses[1],
        json!({
            "date": "2026-05-21",
            "startTime": "09:30",
            "duration": 45
        }),
    );

    println!(
        "create_event output: {}",
        serde_json::to_string_pretty(&output)?
    );
    client.close()?;
    Ok(())
}

#[derive(Clone, Default)]
struct MultiStepElicitationTransport {
    state: Arc<Mutex<MultiStepElicitationState>>,
}

#[derive(Default)]
struct MultiStepElicitationState {
    elicitation_responses: Vec<JsonRpcResponse>,
}

impl MultiStepElicitationTransport {
    fn elicitation_responses(&self) -> McpClientResult<Vec<JsonRpcResponse>> {
        self.state
            .lock()
            .map(|state| state.elicitation_responses.clone())
            .map_err(|_| McpClientError::new("multi-step elicitation transport is poisoned"))
    }
}

impl McpTransport for MultiStepElicitationTransport {
    fn send(&mut self, message: JsonRpcMessage) -> McpClientResult<Vec<JsonRpcMessage>> {
        match message {
            JsonRpcMessage::Request(request) => self.handle_request(request),
            JsonRpcMessage::Response(response) => {
                self.state
                    .lock()
                    .map_err(|_| {
                        McpClientError::new("multi-step elicitation transport is poisoned")
                    })?
                    .elicitation_responses
                    .push(response);
                Ok(Vec::new())
            }
            JsonRpcMessage::Notification(notification) => {
                self.handle_notification(notification)?;
                Ok(Vec::new())
            }
        }
    }
}

impl MultiStepElicitationTransport {
    fn handle_request(&self, request: JsonRpcRequest) -> McpClientResult<Vec<JsonRpcMessage>> {
        match request.method.as_str() {
            "initialize" => Ok(response(JsonRpcResponse::success(
                request.id,
                InitializeResult {
                    protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
                    capabilities: ServerCapabilities {
                        tools: Some(JsonObject::new()),
                        ..ServerCapabilities::default()
                    },
                    server_info: Configuration::new("elicitation-multi-step-server", "1.0.0"),
                    instructions: None,
                    meta: None,
                },
            ))),
            "tools/list" => Ok(response(JsonRpcResponse::success(
                request.id,
                ListToolsResult {
                    tools: vec![create_event_tool()],
                    ..ListToolsResult::default()
                },
            ))),
            "tools/call" => match request.params {
                Some(params) => tool_call_messages(request.id, params),
                None => Ok(response(JsonRpcResponse::error(
                    request.id,
                    JsonRpcError::new(-32602, "Missing tools/call params"),
                ))),
            },
            method => Ok(response(JsonRpcResponse::error(
                request.id,
                JsonRpcError::new(-32601, format!("Unsupported MCP method: {method}")),
            ))),
        }
    }

    fn handle_notification(&self, notification: JsonRpcNotification) -> McpClientResult<()> {
        if notification.method == "notifications/initialized" {
            return Ok(());
        }

        Err(McpClientError::new(format!(
            "Unsupported MCP notification: {}",
            notification.method
        )))
    }
}

fn tool_call_messages(id: JsonValue, params: JsonValue) -> McpClientResult<Vec<JsonRpcMessage>> {
    let call = match serde_json::from_value::<McpCallToolRequest>(params) {
        Ok(call) => call,
        Err(error) => {
            return Ok(response(JsonRpcResponse::error(
                id,
                JsonRpcError::new(-32602, format!("Invalid tools/call params: {error}")),
            )));
        }
    };

    if call.name != "create_event" {
        return Ok(response(JsonRpcResponse::error(
            id,
            JsonRpcError::new(-32602, format!("Unknown tool: {}", call.name)),
        )));
    }

    Ok(vec![
        JsonRpcMessage::Request(
            JsonRpcRequest::new(json!("basic-info"), "elicitation/create").with_params(json!({
                "message": "Step 1: Enter basic event information",
                "requestedSchema": {
                    "type": "object",
                    "properties": {
                        "title": {
                            "type": "string",
                            "title": "Event Title",
                            "description": "Name of the event",
                            "minLength": 1
                        },
                        "description": {
                            "type": "string",
                            "title": "Description",
                            "description": "Event description (optional)"
                        }
                    },
                    "required": ["title"]
                }
            })),
        ),
        JsonRpcMessage::Request(
            JsonRpcRequest::new(json!("date-time"), "elicitation/create").with_params(json!({
                "message": "Step 2: Enter date and time",
                "requestedSchema": {
                    "type": "object",
                    "properties": {
                        "date": {
                            "type": "string",
                            "title": "Date",
                            "description": "Event date"
                        },
                        "startTime": {
                            "type": "string",
                            "title": "Start Time",
                            "description": "Event start time (HH:MM)"
                        },
                        "duration": {
                            "type": "integer",
                            "title": "Duration",
                            "description": "Duration in minutes",
                            "minimum": 15,
                            "maximum": 480
                        }
                    },
                    "required": ["date", "startTime", "duration"]
                }
            })),
        ),
        JsonRpcMessage::Response(JsonRpcResponse::success(
            id,
            CallToolResult {
                content: Some(vec![json!({
                    "type": "text",
                    "text": "Event created successfully!\n\n{\"date\":\"2026-05-21\",\"description\":\"Review the next SDK parity slice\",\"duration\":45,\"startTime\":\"09:30\",\"title\":\"Design review\"}"
                })]),
                is_error: Some(false),
                ..CallToolResult::default()
            },
        )),
    ])
}

fn create_event_tool() -> McpTool {
    let mut tool = McpTool::new("create_event", object_schema());
    tool.description = Some("Create a calendar event by collecting event details".to_string());
    tool
}

fn object_schema() -> JsonSchema {
    JsonObject::from_iter([
        ("type".to_string(), json!("object")),
        ("properties".to_string(), JsonObject::new().into()),
    ])
}

fn response(response: JsonRpcResponse) -> Vec<JsonRpcMessage> {
    vec![JsonRpcMessage::Response(response)]
}

fn assert_response_content(response: &JsonRpcResponse, expected_content: JsonValue) {
    assert_eq!(
        response
            .result
            .as_ref()
            .and_then(|result| result.get("action"))
            .and_then(JsonValue::as_str),
        Some("accept")
    );
    assert_eq!(
        response
            .result
            .as_ref()
            .and_then(|result| result.get("content")),
        Some(&expected_content)
    );
}

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    match Future::poll(future.as_mut(), &mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("example future unexpectedly pending"),
    }
}
