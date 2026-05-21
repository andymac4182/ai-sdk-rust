use std::error::Error;
use std::future::Future;
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use ai_sdk_mcp::{
    CallToolResult, Configuration, InitializeResult, JsonRpcError, JsonRpcMessage, JsonRpcResponse,
    LATEST_PROTOCOL_VERSION, ListToolsResult, McpCallToolRequest, McpClientConfig, McpTool,
    ServerCapabilities, StdioConfig, StdioMcpTransport, create_mcp_client,
};
use ai_sdk_provider::{JsonObject, JsonSchema};
use ai_sdk_provider_utils::{ToolExecutionOptions, ToolModelOutputOptions};
use serde_json::json;

const TINY_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==";

fn main() -> Result<(), Box<dyn Error>> {
    if std::env::args().any(|arg| arg == "--server") {
        return run_stdio_server();
    }

    let server_command = std::env::current_exe()?.to_string_lossy().into_owned();
    let client = create_mcp_client(
        McpClientConfig::new(StdioMcpTransport::new(
            StdioConfig::new(server_command).with_arg("--server"),
        ))
        .with_client_name("rust-image-content-example"),
    )?;

    println!("connected to {}", client.server_info()?.name);

    let tools = client.tools()?;
    let image_tool = tools.get("get-image").expect("get-image tool exists");

    let raw_output = block_on(
        image_tool
            .execute(
                json!({}),
                ToolExecutionOptions::new("image-call-1", Vec::new()),
            )
            .expect("get-image tool is executable"),
    )?;
    println!(
        "raw MCP output: {}",
        serde_json::to_string_pretty(&raw_output)?
    );

    let model_output = block_on(
        image_tool
            .model_output(ToolModelOutputOptions::new(
                "image-call-1",
                json!({}),
                raw_output.clone(),
            ))
            .expect("get-image has a model-output converter"),
    );
    let model_output_json = serde_json::to_value(&model_output)?;
    assert_eq!(
        model_output_json,
        json!({
            "type": "content",
            "value": [{
                "type": "file",
                "data": { "type": "data", "data": TINY_PNG },
                "mediaType": "image/png"
            }]
        })
    );
    println!(
        "AI SDK model output: {}",
        serde_json::to_string_pretty(&model_output_json)?
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
                    server_info: Configuration::new("image-test-server", "1.0.0"),
                    instructions: None,
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

fn local_tools() -> ListToolsResult {
    let mut image_tool = McpTool::new("get-image", object_schema());
    image_tool.description = Some("Returns a test image".to_string());

    ListToolsResult {
        tools: vec![image_tool],
        ..ListToolsResult::default()
    }
}

fn tool_result(name: &str) -> CallToolResult {
    match name {
        "get-image" => CallToolResult {
            content: Some(vec![json!({
                "type": "image",
                "data": TINY_PNG,
                "mimeType": "image/png"
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

fn object_schema() -> JsonSchema {
    JsonObject::from_iter([
        ("type".to_string(), json!("object")),
        ("properties".to_string(), JsonObject::new().into()),
    ])
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
