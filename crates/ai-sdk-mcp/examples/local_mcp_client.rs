use std::error::Error;

use ai_sdk_mcp::{
    CallToolResult, Configuration, InitializeResult, JsonRpcError, JsonRpcMessage,
    JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, LATEST_PROTOCOL_VERSION, ListToolsResult,
    McpCallToolRequest, McpClientError, McpClientResult, McpTool, McpTransport, ServerCapabilities,
    create_mcp_client,
};
use ai_sdk_provider::{JsonObject, JsonSchema, JsonValue};
use serde_json::json;

fn main() -> Result<(), Box<dyn Error>> {
    let client = create_mcp_client(
        ai_sdk_mcp::McpClientConfig::new(LocalToolTransport::default())
            .with_client_name("local-rust-mcp-example"),
    )?;

    let server = client.server_info()?;
    println!("connected to {} {}", server.name, server.version);

    let definitions = client.list_tools(None)?;
    println!(
        "server tools: {}",
        definitions
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let tools = client.tools_from_definitions(&definitions)?;
    println!("converted {} MCP tools into AI SDK tools", tools.len());

    let greeting = client
        .call_tool(McpCallToolRequest::new("greet").with_arguments(json!({ "name": "Alice" })))?;
    println!("greet result: {}", text_content(&greeting).join(" "));

    let sum = client
        .call_tool(McpCallToolRequest::new("add").with_arguments(json!({ "a": 42, "b": 58 })))?;
    println!("add result: {}", text_content(&sum).join(" "));

    let time = client.call_tool(McpCallToolRequest::new("get-current-time"))?;
    println!("time result: {}", text_content(&time).join(" "));

    client.close()?;
    Ok(())
}

#[derive(Default)]
struct LocalToolTransport {
    protocol_version: Option<String>,
}

impl McpTransport for LocalToolTransport {
    fn send(&mut self, message: JsonRpcMessage) -> McpClientResult<Vec<JsonRpcMessage>> {
        match message {
            JsonRpcMessage::Request(request) => self.handle_request(request),
            JsonRpcMessage::Notification(notification) => {
                self.handle_notification(notification)?;
                Ok(Vec::new())
            }
            JsonRpcMessage::Response(_) => Ok(Vec::new()),
        }
    }

    fn set_protocol_version(&mut self, protocol_version: String) {
        self.protocol_version = Some(protocol_version);
    }
}

impl LocalToolTransport {
    fn handle_request(&mut self, request: JsonRpcRequest) -> McpClientResult<Vec<JsonRpcMessage>> {
        let response = match request.method.as_str() {
            "initialize" => JsonRpcResponse::success(
                request.id,
                InitializeResult {
                    protocol_version: self
                        .protocol_version
                        .clone()
                        .unwrap_or_else(|| LATEST_PROTOCOL_VERSION.to_string()),
                    capabilities: ServerCapabilities {
                        tools: Some(JsonObject::new()),
                        ..Default::default()
                    },
                    server_info: Configuration::new("local-tool-server", "1.0.0"),
                    instructions: Some(
                        "Use the listed tools for deterministic local examples.".to_string(),
                    ),
                    meta: None,
                },
            ),
            "tools/list" => JsonRpcResponse::success(request.id, local_tools()),
            "tools/call" => match request.params {
                Some(params) => tool_call_response(request.id, params),
                None => JsonRpcResponse::error(
                    request.id,
                    JsonRpcError::new(-32602, "Missing tools/call params"),
                ),
            },
            method => JsonRpcResponse::error(
                request.id,
                JsonRpcError::new(-32601, format!("Unsupported MCP method: {method}")),
            ),
        };

        Ok(vec![JsonRpcMessage::Response(response)])
    }

    fn handle_notification(&mut self, notification: JsonRpcNotification) -> McpClientResult<()> {
        if notification.method == "notifications/initialized" {
            return Ok(());
        }

        Err(McpClientError::new(format!(
            "Unsupported MCP notification: {}",
            notification.method
        )))
    }
}

fn local_tools() -> ListToolsResult {
    let mut add = McpTool::new("add", add_schema());
    add.description = Some("Add two numbers together.".to_string());

    let mut current_time = McpTool::new("get-current-time", object_schema());
    current_time.description = Some("Return the deterministic example timestamp.".to_string());

    let mut greet = McpTool::new("greet", greet_schema());
    greet.description = Some("Greet a person by name.".to_string());

    ListToolsResult {
        tools: vec![add, current_time, greet],
        ..Default::default()
    }
}

fn tool_call_response(id: JsonValue, params: JsonValue) -> JsonRpcResponse {
    let call = match serde_json::from_value::<McpCallToolRequest>(params) {
        Ok(call) => call,
        Err(error) => {
            return JsonRpcResponse::error(
                id,
                JsonRpcError::new(-32602, format!("Invalid tools/call params: {error}")),
            );
        }
    };

    let result = match call.name.as_str() {
        "add" => add_result(call.arguments.as_ref()),
        "get-current-time" => text_result("Current time: 2026-05-19T00:00:00Z"),
        "greet" => greet_result(call.arguments.as_ref()),
        tool_name => {
            return JsonRpcResponse::error(
                id,
                JsonRpcError::new(-32602, format!("Unknown tool: {tool_name}")),
            );
        }
    };

    JsonRpcResponse::success(id, result)
}

fn add_result(arguments: Option<&JsonValue>) -> CallToolResult {
    let a = arguments
        .and_then(|value| value.get("a"))
        .and_then(JsonValue::as_i64)
        .unwrap_or_default();
    let b = arguments
        .and_then(|value| value.get("b"))
        .and_then(JsonValue::as_i64)
        .unwrap_or_default();

    text_result(format!("{a} + {b} = {}", a + b))
}

fn greet_result(arguments: Option<&JsonValue>) -> CallToolResult {
    let name = arguments
        .and_then(|value| value.get("name"))
        .and_then(JsonValue::as_str)
        .unwrap_or("friend");

    text_result(format!("Hello, {name}! Nice to meet you."))
}

fn text_result(text: impl Into<String>) -> CallToolResult {
    CallToolResult {
        content: Some(vec![json!({ "type": "text", "text": text.into() })]),
        is_error: Some(false),
        ..Default::default()
    }
}

fn text_content(result: &CallToolResult) -> Vec<String> {
    result
        .content
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter_map(|content| content.get("text").and_then(JsonValue::as_str))
        .map(str::to_string)
        .collect()
}

fn object_schema() -> JsonSchema {
    json_object(json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    }))
}

fn add_schema() -> JsonSchema {
    json_object(json!({
        "type": "object",
        "properties": {
            "a": { "type": "integer", "description": "First number" },
            "b": { "type": "integer", "description": "Second number" }
        },
        "required": ["a", "b"],
        "additionalProperties": false
    }))
}

fn greet_schema() -> JsonSchema {
    json_object(json!({
        "type": "object",
        "properties": {
            "name": { "type": "string", "description": "Name of the person to greet" }
        },
        "required": ["name"],
        "additionalProperties": false
    }))
}

fn json_object(value: JsonValue) -> JsonObject {
    match value {
        JsonValue::Object(object) => object,
        _ => unreachable!("schema literals are JSON objects"),
    }
}
