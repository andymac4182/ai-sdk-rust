use std::error::Error;
use std::future::Future;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::Duration;

use ai_sdk_mcp::{
    CallToolResult, Configuration, InitializeResult, JsonRpcError, JsonRpcMessage, JsonRpcResponse,
    LATEST_PROTOCOL_VERSION, ListToolsResult, McpCallToolRequest, McpClientConfig, McpTool,
    McpToolSchema, McpToolSchemas, ServerCapabilities, SseMcpTransport, create_mcp_client,
};
use ai_sdk_provider::{JsonObject, JsonSchema, JsonValue};
use ai_sdk_provider_utils::{Schema, ToolExecutionOptions, ValidationResult};
use serde_json::json;

fn main() -> Result<(), Box<dyn Error>> {
    let server = LocalSseMcpServer::start()?;
    let client = create_mcp_client(
        McpClientConfig::new(SseMcpTransport::new(format!("{}/sse", server.url())))
            .with_client_name("rust-sse-example"),
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
                ToolExecutionOptions::new("weather-sse-1", Vec::new()),
            )
            .expect("weather tool is executable"),
    )?;
    println!("typed weather result: {weather}");

    let lookup = block_on(
        tools["lookup-order"]
            .execute(
                json!({ "orderId": "order_456" }),
                ToolExecutionOptions::new("lookup-sse-1", Vec::new()),
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
    println!("server handled {} requests", server.requests().len());
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

#[derive(Clone, Debug)]
struct LocalHttpRequest {
    method: String,
    path: String,
    body: JsonValue,
}

struct LocalSseMcpServer {
    url: String,
    requests: Arc<Mutex<Vec<LocalHttpRequest>>>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl LocalSseMcpServer {
    fn start() -> Result<Self, Box<dyn Error>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let url = format!("http://{}", listener.local_addr()?);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let server_requests = Arc::clone(&requests);
        let server_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || {
            while !server_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => handle_connection(stream, &server_requests),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            url,
            requests,
            stop,
            handle: Some(handle),
        })
    }

    fn url(&self) -> &str {
        &self.url
    }

    fn requests(&self) -> Vec<LocalHttpRequest> {
        self.requests
            .lock()
            .expect("local SSE MCP server request log")
            .clone()
    }
}

impl Drop for LocalSseMcpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.url.trim_start_matches("http://"));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn handle_connection(mut stream: TcpStream, requests: &Arc<Mutex<Vec<LocalHttpRequest>>>) {
    let Some(request) = read_request(&mut stream) else {
        return;
    };
    requests
        .lock()
        .expect("local SSE MCP server request log")
        .push(request.clone());

    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/sse") => write_response(
            &mut stream,
            200,
            "text/event-stream",
            "event: endpoint\ndata: /messages\n\n",
        ),
        ("POST", "/messages") => write_mcp_post_response(&mut stream, request.body),
        _ => write_response(&mut stream, 404, "text/plain", "not found"),
    }
}

fn write_mcp_post_response(stream: &mut TcpStream, body: JsonValue) {
    let message = match serde_json::from_value::<JsonRpcMessage>(body) {
        Ok(message) => message,
        Err(error) => {
            let response = JsonRpcResponse::error(
                json!(null),
                JsonRpcError::new(-32700, format!("Invalid JSON-RPC message: {error}")),
            );
            return write_sse_message(stream, response);
        }
    };

    let JsonRpcMessage::Request(request) = message else {
        return write_response(stream, 202, "text/plain", "");
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
                server_info: Configuration::new("local-sse-mcp-server", "1.0.0"),
                instructions: Some("Use the SSE MCP tools.".to_string()),
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

    write_sse_message(stream, response);
}

fn write_sse_message(stream: &mut TcpStream, response: JsonRpcResponse) {
    let body = format!(
        "event: message\ndata: {}\n\n",
        serde_json::to_string(&JsonRpcMessage::Response(response)).expect("response serializes")
    );
    write_response(stream, 200, "text/event-stream", &body);
}

fn write_response(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) {
    let reason = match status {
        200 => "OK",
        202 => "Accepted",
        404 => "Not Found",
        _ => "OK",
    };
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn read_request(stream: &mut TcpStream) -> Option<LocalHttpRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(header_end) = find_header_end(&buffer) {
            let content_length = content_length(&buffer[..header_end]).unwrap_or(0);
            if buffer.len() >= header_end + 4 + content_length {
                break;
            }
        }
    }

    let header_end = find_header_end(&buffer)?;
    let headers = String::from_utf8_lossy(&buffer[..header_end]);
    let mut lines = headers.lines();
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_string();
    let path = parts.next()?.to_string();
    let body = &buffer[header_end + 4..];
    let body = if body.is_empty() {
        json!(null)
    } else {
        serde_json::from_slice(body).ok()?
    };

    Some(LocalHttpRequest { method, path, body })
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length(headers: &[u8]) -> Option<usize> {
    let headers = String::from_utf8_lossy(headers);
    headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.eq_ignore_ascii_case("content-length") {
            value.trim().parse().ok()
        } else {
            None
        }
    })
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
