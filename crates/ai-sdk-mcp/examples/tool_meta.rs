use std::error::Error;

use ai_sdk_mcp::{
    Configuration, InitializeResult, JsonRpcError, JsonRpcMessage, JsonRpcNotification,
    JsonRpcRequest, JsonRpcResponse, LATEST_PROTOCOL_VERSION, ListToolsResult, MCP_APP_MIME_TYPE,
    McpClientError, McpClientResult, McpTool, McpTransport, OPENAI_OUTPUT_TEMPLATE_META_KEY,
    ReadResourceResult, ResourceContent, ServerCapabilities, TextResourceContent,
    create_mcp_client, get_mcp_app_resource_from_read_result, get_mcp_app_resource_uri,
};
use ai_sdk_provider::{JsonObject, JsonSchema, JsonValue};
use serde_json::json;

const WEATHER_WIDGET_URI: &str = "ui://widgets/weather.html";

fn main() -> Result<(), Box<dyn Error>> {
    let client = create_mcp_client(
        ai_sdk_mcp::McpClientConfig::new(ToolMetaTransport)
            .with_client_name("rust-tool-meta-example"),
    )?;

    let definitions = client.list_tools(None)?;
    let weather_tool = definitions
        .tools
        .iter()
        .find(|tool| tool.name == "get-weather")
        .expect("weather tool is defined");
    let output_template =
        get_mcp_app_resource_uri(weather_tool)?.expect("weather tool has an output template");

    let tools = client.tools_from_definitions(&definitions)?;
    let weather_metadata = tools
        .get("get-weather")
        .and_then(|tool| tool.metadata())
        .cloned()
        .unwrap_or_default();

    println!("Tool: get-weather");
    println!(
        "  Description: {}",
        weather_tool.description.as_deref().unwrap_or("")
    );
    println!(
        "  Provider metadata: {}",
        serde_json::to_string_pretty(&weather_metadata)?
    );
    println!("  Output template: {output_template}");

    let widget_resource = client.read_resource(output_template.clone())?;
    let widget = get_mcp_app_resource_from_read_result(&output_template, &widget_resource)?;
    println!("Weather widget HTML: {}", widget.html);

    let time_tool = definitions
        .tools
        .iter()
        .find(|tool| tool.name == "get-time")
        .expect("time tool is defined");
    println!("\nTool: get-time");
    println!(
        "  Description: {}",
        time_tool.description.as_deref().unwrap_or("")
    );
    println!(
        "  Output template: {:?}",
        get_mcp_app_resource_uri(time_tool)?
    );

    client.close()?;
    Ok(())
}

struct ToolMetaTransport;

impl McpTransport for ToolMetaTransport {
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
}

impl ToolMetaTransport {
    fn handle_request(&mut self, request: JsonRpcRequest) -> McpClientResult<Vec<JsonRpcMessage>> {
        match request.method.as_str() {
            "initialize" => Ok(response(JsonRpcResponse::success(
                request.id,
                initialize_result(),
            ))),
            "tools/list" => Ok(response(JsonRpcResponse::success(
                request.id,
                tool_definitions(),
            ))),
            "resources/read" => Ok(response(read_resource_response(request.id, request.params))),
            method => Ok(response(JsonRpcResponse::error(
                request.id,
                JsonRpcError::new(-32601, format!("Unsupported MCP method: {method}")),
            ))),
        }
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

fn response(response: JsonRpcResponse) -> Vec<JsonRpcMessage> {
    vec![JsonRpcMessage::Response(response)]
}

fn initialize_result() -> InitializeResult {
    InitializeResult {
        protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
        capabilities: ServerCapabilities {
            tools: Some(JsonObject::new()),
            resources: Some(JsonObject::new()),
            ..Default::default()
        },
        server_info: Configuration::new("tool-meta-example-server", "1.0.0"),
        instructions: None,
        meta: None,
    }
}

fn tool_definitions() -> ListToolsResult {
    let mut weather_tool = McpTool::new("get-weather", weather_input_schema());
    weather_tool.description = Some("Get weather information for a location".to_string());
    weather_tool.meta = Some(JsonObject::from_iter([(
        OPENAI_OUTPUT_TEMPLATE_META_KEY.to_string(),
        json!(WEATHER_WIDGET_URI),
    )]));

    let mut time_tool = McpTool::new("get-time", object_schema());
    time_tool.description = Some("Get current time".to_string());

    ListToolsResult {
        tools: vec![weather_tool, time_tool],
        ..Default::default()
    }
}

fn read_resource_response(id: JsonValue, params: Option<JsonValue>) -> JsonRpcResponse {
    let uri = params
        .as_ref()
        .and_then(|params| params.get("uri"))
        .and_then(JsonValue::as_str)
        .unwrap_or_default();

    if uri != WEATHER_WIDGET_URI {
        return JsonRpcResponse::error(
            id,
            JsonRpcError::new(-32602, format!("Unknown resource: {uri}")),
        );
    }

    JsonRpcResponse::success(
        id,
        ReadResourceResult {
            contents: vec![ResourceContent::Text(TextResourceContent {
                uri: WEATHER_WIDGET_URI.to_string(),
                name: None,
                title: None,
                mime_type: Some(MCP_APP_MIME_TYPE.to_string()),
                meta: Some(JsonObject::from_iter([(
                    "ui".to_string(),
                    json!({
                        "prefersBorder": true
                    }),
                )])),
                text: "<div>Weather widget</div>".to_string(),
                extra: JsonObject::new(),
            })],
            meta: None,
        },
    )
}

fn weather_input_schema() -> JsonSchema {
    JsonObject::from_iter([
        ("type".to_string(), json!("object")),
        (
            "properties".to_string(),
            json!({
                "location": {
                    "type": "string"
                }
            }),
        ),
        ("required".to_string(), json!(["location"])),
    ])
}

fn object_schema() -> JsonSchema {
    JsonObject::from_iter([
        ("type".to_string(), json!("object")),
        ("properties".to_string(), JsonObject::new().into()),
    ])
}
