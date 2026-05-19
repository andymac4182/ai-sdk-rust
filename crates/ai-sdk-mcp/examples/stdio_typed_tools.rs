use std::error::Error;
use std::future::Future;
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use ai_sdk_mcp::{
    CallToolResult, Configuration, InitializeResult, JsonRpcError, JsonRpcMessage, JsonRpcResponse,
    LATEST_PROTOCOL_VERSION, ListToolsResult, McpCallToolRequest, McpClientConfig, McpTool,
    McpToolSchema, McpToolSchemas, ServerCapabilities, StdioConfig, StdioMcpTransport,
    create_mcp_client,
};
use ai_sdk_provider::{JsonObject, JsonSchema, JsonValue};
use ai_sdk_provider_utils::{Schema, ToolExecutionOptions, ValidationResult};
use serde_json::json;

fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().any(|arg| arg == "--server") {
        return run_stdio_server();
    }

    let server_command = std::env::current_exe()?.to_string_lossy().into_owned();
    let client = create_mcp_client(
        McpClientConfig::new(StdioMcpTransport::new(
            StdioConfig::new(server_command).with_arg("--server"),
        ))
        .with_client_name("rust-stdio-example"),
    )?;

    println!("connected to {}", client.server_info()?.name);

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

    let schemas = McpToolSchemas::from([
        (
            "get-weather".to_string(),
            McpToolSchema::new()
                .with_input_schema(object_schema())
                .with_output_schema(weather_output_schema()),
        ),
        (
            "lookup-order".to_string(),
            McpToolSchema::new().with_input_schema(object_schema()),
        ),
    ]);
    let tools = client.tools_from_definitions_with_schemas(&definitions, &schemas)?;

    let weather = block_on(
        tools["get-weather"]
            .execute(
                json!({ "location": "Brisbane" }),
                ToolExecutionOptions::new("weather-stdio-1", Vec::new()),
            )
            .expect("weather tool is executable"),
    )?;
    println!("typed weather result: {weather}");

    let lookup = block_on(
        tools["lookup-order"]
            .execute(
                json!({ "orderId": "order_456" }),
                ToolExecutionOptions::new("lookup-stdio-1", Vec::new()),
            )
            .expect("lookup tool is executable"),
    )?;
    println!("raw lookup result: {lookup}");

    let metadata = tools["lookup-order"]
        .metadata()
        .cloned()
        .unwrap_or_default();
    println!(
        "lookup provider metadata: {}",
        serde_json::to_string_pretty(&metadata)?
    );

    client.close()?;
    Ok(())
}

fn run_stdio_server() -> Result<(), Box<dyn Error>> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        let message = serde_json::from_str::<JsonRpcMessage>(&line)?;
        let JsonRpcMessage::Request(request) = message else {
            continue;
        };

        let response = match request.method.as_str() {
            "initialize" => JsonRpcResponse::success(
                request.id,
                InitializeResult {
                    protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
                    capabilities: ServerCapabilities {
                        tools: Some(JsonObject::new()),
                        ..ServerCapabilities::default()
                    },
                    server_info: Configuration::new("local-stdio-mcp-server", "1.0.0"),
                    instructions: Some("Use the stdio MCP tools.".to_string()),
                    meta: None,
                },
            ),
            "tools/list" => JsonRpcResponse::success(request.id, local_tools()),
            "tools/call" => {
                let result = request
                    .params
                    .and_then(|params| serde_json::from_value::<McpCallToolRequest>(params).ok())
                    .map(|request| tool_result(&request.name))
                    .unwrap_or_else(|| CallToolResult {
                        content: Some(vec![json!({
                            "type": "text",
                            "text": "Invalid tool request"
                        })]),
                        is_error: Some(true),
                        ..CallToolResult::default()
                    });
                JsonRpcResponse::success(request.id, result)
            }
            method => JsonRpcResponse::error(
                request.id,
                JsonRpcError::new(-32601, format!("Unsupported MCP method: {method}")),
            ),
        };

        writeln!(
            stdout,
            "{}",
            serde_json::to_string(&JsonRpcMessage::Response(response))?
        )?;
        stdout.flush()?;
    }

    Ok(())
}

fn object_schema() -> JsonSchema {
    JsonObject::from_iter([
        ("type".to_string(), json!("object")),
        ("properties".to_string(), JsonObject::new().into()),
    ])
}

fn weather_json_schema() -> JsonSchema {
    JsonObject::from_iter([
        ("type".to_string(), json!("object")),
        (
            "properties".to_string(),
            json!({
                "temperature": { "type": "number" },
                "conditions": { "type": "string" }
            }),
        ),
        ("required".to_string(), json!(["temperature", "conditions"])),
    ])
}

fn weather_output_schema() -> Schema {
    Schema::new(weather_json_schema()).with_validator(|value| {
        let valid = value
            .get("temperature")
            .and_then(JsonValue::as_f64)
            .is_some()
            && value
                .get("conditions")
                .and_then(JsonValue::as_str)
                .is_some();
        if valid {
            ValidationResult::success(value.clone())
        } else {
            ValidationResult::failure("weather output shape is invalid")
        }
    })
}

fn local_tools() -> ListToolsResult {
    let mut weather_tool = McpTool::new("get-weather", object_schema());
    weather_tool.title = Some("Get Weather".to_string());
    weather_tool.description = Some("Get weather data for a location.".to_string());
    weather_tool.output_schema = Some(weather_json_schema());

    let mut lookup_order_tool = McpTool::new("lookup-order", object_schema());
    lookup_order_tool.title = Some("Lookup Order".to_string());
    lookup_order_tool.description = Some("Look up the status of a customer order.".to_string());

    ListToolsResult {
        tools: vec![weather_tool, lookup_order_tool],
        ..ListToolsResult::default()
    }
}

fn tool_result(name: &str) -> CallToolResult {
    match name {
        "get-weather" => CallToolResult {
            content: Some(vec![json!({
                "type": "text",
                "text": "{\"temperature\":27.0,\"conditions\":\"Clear\"}"
            })]),
            structured_content: Some(json!({
                "temperature": 27.0,
                "conditions": "Clear"
            })),
            is_error: Some(false),
            ..CallToolResult::default()
        },
        "lookup-order" => CallToolResult {
            content: Some(vec![json!({
                "type": "text",
                "text": "Order order_456 is out for delivery."
            })]),
            is_error: Some(false),
            ..CallToolResult::default()
        },
        _ => CallToolResult {
            content: Some(vec![json!({
                "type": "text",
                "text": format!("Unknown tool: {name}")
            })]),
            is_error: Some(true),
            ..CallToolResult::default()
        },
    }
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
