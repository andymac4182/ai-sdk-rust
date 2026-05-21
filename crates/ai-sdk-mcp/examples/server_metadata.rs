use std::error::Error;
use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use ai_sdk_mcp::{
    CallToolResult, Configuration, InitializeResult, LATEST_PROTOCOL_VERSION, McpClientConfig,
    McpTool, MockMcpTransport, ServerCapabilities, create_mcp_client,
};
use ai_sdk_provider::{JsonObject, JsonSchema};
use ai_sdk_provider_utils::ToolExecutionOptions;
use serde_json::json;

const SERVER_INSTRUCTIONS: &str = "Use search tools to resolve IDs - never ask the user. Always confirm destructive actions before executing.";

fn main() -> Result<(), Box<dyn Error>> {
    let transport = MockMcpTransport::new()
        .with_initialize_result(InitializeResult {
            protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities {
                tools: Some(JsonObject::new()),
                ..ServerCapabilities::default()
            },
            server_info: Configuration::new("server-with-instructions", "1.0.0"),
            instructions: Some(SERVER_INSTRUCTIONS.to_string()),
            meta: None,
        })
        .with_tools([ping_tool()])
        .with_tool_call_result("ping", ping_result());

    let client = create_mcp_client(
        McpClientConfig::new(transport).with_client_name("rust-server-metadata-example"),
    )?;

    let server = client.server_info()?;
    let instructions = client.instructions()?.expect("server exposes instructions");
    assert_eq!(server.name, "server-with-instructions");
    assert_eq!(server.version, "1.0.0");
    assert_eq!(instructions, SERVER_INSTRUCTIONS);

    println!("serverInfo: {} {}", server.name, server.version);
    println!("instructions: {instructions}");

    let tools = client.tools()?;
    let output = block_on(
        tools["ping"]
            .execute(json!({}), ToolExecutionOptions::new("ping-1", Vec::new()))
            .expect("ping tool is executable"),
    )?;
    assert_eq!(
        output,
        serde_json::to_value(ping_result()).expect("ping result serializes")
    );
    println!("ping output: {}", serde_json::to_string_pretty(&output)?);

    client.close()?;
    Ok(())
}

fn ping_tool() -> McpTool {
    let mut tool = McpTool::new("ping", object_schema());
    tool.description = Some("A simple ping tool".to_string());
    tool
}

fn ping_result() -> CallToolResult {
    CallToolResult {
        content: Some(vec![json!({ "type": "text", "text": "pong" })]),
        is_error: Some(false),
        ..CallToolResult::default()
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
