use std::error::Error;

use ai_sdk_mcp::{
    BlobResourceContent, CallToolResult, ClientCapabilities, Configuration, ElicitAction,
    ElicitResult, ElicitationCapability, GetPromptResult, InitializeResult, JsonRpcError,
    JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, LATEST_PROTOCOL_VERSION,
    ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult, ListToolsResult,
    McpCallToolRequest, McpClientError, McpClientResult, McpGetPromptRequest, McpPrompt,
    McpPromptArgument, McpPromptMessage, McpResource, McpResourceTemplate, McpTool, McpTransport,
    ReadResourceResult, ResourceContent, ServerCapabilities, TextResourceContent,
    create_mcp_client,
};
use ai_sdk_provider::{JsonObject, JsonSchema, JsonValue};
use serde_json::json;

fn main() -> Result<(), Box<dyn Error>> {
    let client = create_mcp_client(
        ai_sdk_mcp::McpClientConfig::new(LocalToolTransport::default())
            .with_client_name("local-rust-mcp-example")
            .with_capabilities(ClientCapabilities {
                elicitation: Some(ElicitationCapability {
                    apply_defaults: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            }),
    )?;

    let server = client.server_info()?;
    println!("connected to {} {}", server.name, server.version);
    if let Some(instructions) = client.instructions()? {
        println!("server instructions: {instructions}");
    }

    client.on_elicitation_request(|request| {
        println!("elicitation request: {}", request.params.message);
        Ok(ElicitResult {
            action: ElicitAction::Accept,
            content: Some(JsonObject::from_iter([(
                "confirmed".to_string(),
                JsonValue::Bool(true),
            )])),
            meta: None,
        })
    })?;

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

    let resources = client.list_resources(None)?;
    println!(
        "resources: {}",
        resources
            .resources
            .iter()
            .map(|resource| resource.uri.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let status = client.read_resource("memory://example/status")?;
    println!(
        "resource status: {}",
        resource_text_content(&status).join(" ")
    );

    let templates = client.list_resource_templates()?;
    println!(
        "resource templates: {}",
        templates
            .resource_templates
            .iter()
            .map(|template| template.uri_template.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let prompts = client.list_prompts(None)?;
    println!(
        "prompts: {}",
        prompts
            .prompts
            .iter()
            .map(|prompt| prompt.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let prompt = client.get_prompt(
        McpGetPromptRequest::new("summarize-resource")
            .with_arguments(json!({ "uri": "memory://example/status" })),
    )?;
    println!("prompt preview: {}", prompt_text_content(&prompt).join(" "));

    let approval = client.call_tool(McpCallToolRequest::new("ask-operator"))?;
    println!("approval result: {}", text_content(&approval).join(" "));

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
        match request.method.as_str() {
            "initialize" => Ok(response_messages(JsonRpcResponse::success(
                request.id,
                initialize_result(self.protocol_version.clone()),
            ))),
            "tools/list" => Ok(response_messages(JsonRpcResponse::success(
                request.id,
                local_tools(),
            ))),
            "tools/call" => match request.params {
                Some(params) => tool_call_messages(request.id, params),
                None => Ok(response_messages(JsonRpcResponse::error(
                    request.id,
                    JsonRpcError::new(-32602, "Missing tools/call params"),
                ))),
            },
            "resources/list" => Ok(response_messages(JsonRpcResponse::success(
                request.id,
                local_resources(),
            ))),
            "resources/read" => Ok(response_messages(read_resource_response(
                request.id,
                request.params,
            ))),
            "resources/templates/list" => Ok(response_messages(JsonRpcResponse::success(
                request.id,
                local_resource_templates(),
            ))),
            "prompts/list" => Ok(response_messages(JsonRpcResponse::success(
                request.id,
                local_prompts(),
            ))),
            "prompts/get" => Ok(response_messages(get_prompt_response(
                request.id,
                request.params,
            ))),
            method => Ok(response_messages(JsonRpcResponse::error(
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

fn initialize_result(protocol_version: Option<String>) -> InitializeResult {
    InitializeResult {
        protocol_version: protocol_version.unwrap_or_else(|| LATEST_PROTOCOL_VERSION.to_string()),
        capabilities: ServerCapabilities {
            tools: Some(JsonObject::new()),
            resources: Some(JsonObject::new()),
            prompts: Some(JsonObject::new()),
            ..Default::default()
        },
        server_info: Configuration::new("local-tool-server", "1.0.0"),
        instructions: Some(
            "Use the listed tools, resources, and prompts for deterministic local examples."
                .to_string(),
        ),
        meta: None,
    }
}

fn response_messages(response: JsonRpcResponse) -> Vec<JsonRpcMessage> {
    vec![JsonRpcMessage::Response(response)]
}

fn local_tools() -> ListToolsResult {
    let mut add = McpTool::new("add", add_schema());
    add.description = Some("Add two numbers together.".to_string());

    let mut ask_operator = McpTool::new("ask-operator", object_schema());
    ask_operator.description = Some("Ask the host application for confirmation.".to_string());

    let mut current_time = McpTool::new("get-current-time", object_schema());
    current_time.description = Some("Return the deterministic example timestamp.".to_string());

    let mut greet = McpTool::new("greet", greet_schema());
    greet.description = Some("Greet a person by name.".to_string());

    ListToolsResult {
        tools: vec![add, ask_operator, current_time, greet],
        ..Default::default()
    }
}

fn tool_call_messages(id: JsonValue, params: JsonValue) -> McpClientResult<Vec<JsonRpcMessage>> {
    let call = match serde_json::from_value::<McpCallToolRequest>(params) {
        Ok(call) => call,
        Err(error) => {
            return Ok(response_messages(JsonRpcResponse::error(
                id,
                JsonRpcError::new(-32602, format!("Invalid tools/call params: {error}")),
            )));
        }
    };

    let result = match call.name.as_str() {
        "add" => add_result(call.arguments.as_ref()),
        "ask-operator" => {
            return Ok(vec![
                JsonRpcMessage::Request(
                    JsonRpcRequest::new(json!("elicitation-1"), "elicitation/create").with_params(
                        json!({
                            "message": "Approve the deterministic MCP example action?",
                            "requestedSchema": {
                                "type": "object",
                                "properties": {
                                    "confirmed": { "type": "boolean" }
                                },
                                "required": ["confirmed"],
                                "additionalProperties": false
                            }
                        }),
                    ),
                ),
                JsonRpcMessage::Response(JsonRpcResponse::success(
                    id,
                    text_result("Operator approval request completed."),
                )),
            ]);
        }
        "get-current-time" => text_result("Current time: 2026-05-19T00:00:00Z"),
        "greet" => greet_result(call.arguments.as_ref()),
        tool_name => {
            return Ok(response_messages(JsonRpcResponse::error(
                id,
                JsonRpcError::new(-32602, format!("Unknown tool: {tool_name}")),
            )));
        }
    };

    Ok(response_messages(JsonRpcResponse::success(id, result)))
}

fn local_resources() -> ListResourcesResult {
    ListResourcesResult {
        resources: vec![McpResource {
            uri: "memory://example/status".to_string(),
            name: "project-status".to_string(),
            title: Some("Project status".to_string()),
            description: Some("A deterministic status resource.".to_string()),
            mime_type: Some("text/plain".to_string()),
            size: None,
            extra: JsonObject::new(),
        }],
        ..Default::default()
    }
}

fn read_resource_response(id: JsonValue, params: Option<JsonValue>) -> JsonRpcResponse {
    let uri = params
        .as_ref()
        .and_then(|params| params.get("uri"))
        .and_then(JsonValue::as_str)
        .unwrap_or_default();

    if uri != "memory://example/status" {
        return JsonRpcResponse::error(
            id,
            JsonRpcError::new(-32602, format!("Unknown resource: {uri}")),
        );
    }

    JsonRpcResponse::success(
        id,
        ReadResourceResult {
            contents: vec![ResourceContent::Text(TextResourceContent {
                uri: uri.to_string(),
                name: Some("project-status".to_string()),
                title: Some("Project status".to_string()),
                mime_type: Some("text/plain".to_string()),
                meta: None,
                text: "Status: all deterministic MCP example systems are nominal.".to_string(),
                extra: JsonObject::new(),
            })],
            meta: None,
        },
    )
}

fn local_resource_templates() -> ListResourceTemplatesResult {
    ListResourceTemplatesResult {
        resource_templates: vec![McpResourceTemplate {
            uri_template: "memory://example/{name}".to_string(),
            name: "example-memory-resource".to_string(),
            title: Some("Example memory resource".to_string()),
            description: Some("Read one deterministic in-memory example resource.".to_string()),
            mime_type: Some("text/plain".to_string()),
            extra: JsonObject::new(),
        }],
        meta: None,
    }
}

fn local_prompts() -> ListPromptsResult {
    ListPromptsResult {
        prompts: vec![McpPrompt {
            name: "summarize-resource".to_string(),
            title: Some("Summarize resource".to_string()),
            description: Some("Build a prompt from a resource URI.".to_string()),
            arguments: Some(vec![McpPromptArgument {
                name: "uri".to_string(),
                description: Some("The MCP resource URI to summarize.".to_string()),
                required: Some(true),
                extra: JsonObject::new(),
            }]),
            extra: JsonObject::new(),
        }],
        ..Default::default()
    }
}

fn get_prompt_response(id: JsonValue, params: Option<JsonValue>) -> JsonRpcResponse {
    let request = match params
        .map(serde_json::from_value::<McpGetPromptRequest>)
        .transpose()
    {
        Ok(Some(request)) => request,
        Ok(None) => {
            return JsonRpcResponse::error(
                id,
                JsonRpcError::new(-32602, "Missing prompts/get params"),
            );
        }
        Err(error) => {
            return JsonRpcResponse::error(
                id,
                JsonRpcError::new(-32602, format!("Invalid prompts/get params: {error}")),
            );
        }
    };

    if request.name != "summarize-resource" {
        return JsonRpcResponse::error(
            id,
            JsonRpcError::new(-32602, format!("Unknown prompt: {}", request.name)),
        );
    }

    let uri = request
        .arguments
        .as_ref()
        .and_then(|arguments| arguments.get("uri"))
        .and_then(JsonValue::as_str)
        .unwrap_or("memory://example/status");

    JsonRpcResponse::success(
        id,
        GetPromptResult {
            description: Some("A deterministic prompt built from an MCP resource.".to_string()),
            messages: vec![McpPromptMessage {
                role: "user".to_string(),
                content: json!({
                    "type": "text",
                    "text": format!("Summarize the MCP resource at {uri}."),
                }),
                extra: JsonObject::new(),
            }],
            meta: None,
        },
    )
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

fn resource_text_content(result: &ReadResourceResult) -> Vec<String> {
    result
        .contents
        .iter()
        .filter_map(|content| match content {
            ResourceContent::Text(TextResourceContent { text, .. }) => Some(text.clone()),
            ResourceContent::Blob(BlobResourceContent { .. }) => None,
        })
        .collect()
}

fn prompt_text_content(result: &GetPromptResult) -> Vec<String> {
    result
        .messages
        .iter()
        .filter_map(|message| message.content.get("text").and_then(JsonValue::as_str))
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
