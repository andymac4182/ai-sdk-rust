use std::collections::BTreeMap;
use std::error::Error;
use std::future::Future;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ai_sdk_mcp::{
    CallToolResult, Configuration, InitializeResult, JsonRpcError, JsonRpcMessage, JsonRpcResponse,
    LATEST_PROTOCOL_VERSION, ListToolsResult, McpCallToolRequest, McpClientConfig,
    McpHttpTransport, McpTool, McpToolSchema, McpToolSchemas, ServerCapabilities,
    create_mcp_client,
};
use ai_sdk_provider::{JsonObject, JsonSchema, JsonValue};
use ai_sdk_provider_utils::{Schema, ToolExecutionOptions, ValidationResult};
use serde_json::json;

const AUTH_HEADER: &str = "Bearer local-example-token";

fn main() -> Result<(), Box<dyn Error>> {
    let server = LocalHttpMcpServer::start(AUTH_HEADER);
    let client = create_mcp_client(
        McpClientConfig::new(
            McpHttpTransport::new(format!("{}/mcp", server.url()))
                .with_header("Authorization", AUTH_HEADER),
        )
        .with_client_name("rust-http-auth-example"),
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
                json!({ "location": "New York" }),
                ToolExecutionOptions::new("weather-1", Vec::new()),
            )
            .expect("weather tool is executable"),
    )?;
    println!("typed weather result: {weather}");

    let lookup = block_on(
        tools["lookup-order"]
            .execute(
                json!({ "orderId": "order_123" }),
                ToolExecutionOptions::new("lookup-1", Vec::new()),
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
    println!("authenticated HTTP requests: {}", server.requests().len());

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
                "text": "{\"temperature\":22.5,\"conditions\":\"Sunny\"}"
            })]),
            structured_content: Some(json!({
                "temperature": 22.5,
                "conditions": "Sunny"
            })),
            is_error: Some(false),
            ..CallToolResult::default()
        },
        "lookup-order" => CallToolResult {
            content: Some(vec![json!({
                "type": "text",
                "text": "Order order_123 is packed and ready to ship."
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

#[derive(Clone, Debug)]
struct LocalHttpRequest {
    method: String,
    headers: BTreeMap<String, String>,
    body: JsonValue,
}

struct LocalHttpMcpServer {
    url: String,
    requests: Arc<Mutex<Vec<LocalHttpRequest>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl LocalHttpMcpServer {
    fn start(auth_header: impl Into<String>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local MCP server");
        listener
            .set_nonblocking(true)
            .expect("set local MCP server nonblocking");
        let url = format!("http://{}", listener.local_addr().expect("local address"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let handle_requests = Arc::clone(&requests);
        let handle_stop = Arc::clone(&stop);
        let expected_auth_header = auth_header.into();

        let handle = thread::spawn(move || {
            while !handle_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        handle_local_connection(stream, &handle_requests, &expected_auth_header);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });

        Self {
            url,
            requests,
            stop,
            handle: Some(handle),
        }
    }

    fn url(&self) -> String {
        self.url.clone()
    }

    fn requests(&self) -> Vec<LocalHttpRequest> {
        self.requests.lock().expect("local requests lock").clone()
    }
}

impl Drop for LocalHttpMcpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(
            self.url
                .strip_prefix("http://")
                .expect("local server URL has prefix"),
        );
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn handle_local_connection(
    mut stream: TcpStream,
    requests: &Arc<Mutex<Vec<LocalHttpRequest>>>,
    expected_auth_header: &str,
) {
    let Some(request) = read_local_request(&mut stream) else {
        return;
    };
    requests
        .lock()
        .expect("local requests lock")
        .push(request.clone());

    if request.headers.get("authorization").map(String::as_str) != Some(expected_auth_header) {
        write_response(
            &mut stream,
            401,
            [("content-type", "text/plain")],
            "missing bearer token",
        );
        return;
    }

    match request.method.as_str() {
        "GET" => write_response(&mut stream, 405, [("content-type", "text/plain")], ""),
        "DELETE" => write_response(&mut stream, 200, [("content-type", "text/plain")], ""),
        "POST" => write_mcp_response(&mut stream, request.body),
        _ => write_response(&mut stream, 405, [("content-type", "text/plain")], ""),
    }
}

fn write_mcp_response(stream: &mut TcpStream, body: JsonValue) {
    let method = body
        .get("method")
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    if method == "notifications/initialized" {
        write_response(stream, 200, [("content-type", "text/plain")], "");
        return;
    }

    let id = body.get("id").cloned().unwrap_or(JsonValue::Null);
    let response = match method {
        "initialize" => JsonRpcResponse::success(
            id,
            InitializeResult {
                protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
                capabilities: ServerCapabilities {
                    tools: Some(JsonObject::new()),
                    ..ServerCapabilities::default()
                },
                server_info: Configuration::new("local-http-auth-mcp-server", "1.0.0"),
                instructions: Some("Use the authenticated local MCP tools.".to_string()),
                meta: None,
            },
        ),
        "tools/list" => JsonRpcResponse::success(id, local_tools()),
        "tools/call" => {
            let result = body
                .get("params")
                .cloned()
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
            JsonRpcResponse::success(id, result)
        }
        _ => JsonRpcResponse::error(
            id,
            JsonRpcError::new(-32601, format!("Unsupported MCP method: {method}")),
        ),
    };
    let mut headers =
        BTreeMap::from([("content-type".to_string(), "application/json".to_string())]);
    if method == "initialize" {
        headers.insert(
            "mcp-session-id".to_string(),
            "example-session-1".to_string(),
        );
    }
    write_response(
        stream,
        200,
        headers,
        serde_json::to_string(&JsonRpcMessage::Response(response))
            .expect("JSON-RPC response serializes"),
    );
}

fn read_local_request(stream: &mut TcpStream) -> Option<LocalHttpRequest> {
    stream
        .set_nonblocking(false)
        .expect("local stream is blocking");
    let mut buffer = Vec::new();
    let mut chunk = [0; 1024];
    loop {
        let read = stream.read(&mut chunk).ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(request) = parse_local_request(&buffer) {
            return Some(request);
        }
    }
}

fn parse_local_request(buffer: &[u8]) -> Option<LocalHttpRequest> {
    let header_end = buffer.windows(4).position(|window| window == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&buffer[..header_end]);
    let mut lines = head.lines();
    let request_line = lines.next()?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next()?.to_string();
    let _path = request_parts.next()?.to_string();
    let mut headers = BTreeMap::new();
    for line in lines {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        headers.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
    }
    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;
    if buffer.len() < body_start + content_length {
        return None;
    }
    let body = String::from_utf8_lossy(&buffer[body_start..body_start + content_length]);
    Some(LocalHttpRequest {
        method,
        headers,
        body: serde_json::from_str(&body).unwrap_or(JsonValue::Null),
    })
}

fn write_response<K, V, I>(stream: &mut TcpStream, status: u16, headers: I, body: impl Into<String>)
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    let body = body.into();
    let mut response = format!(
        "HTTP/1.1 {status} OK\r\ncontent-length: {}\r\nconnection: close\r\n",
        body.len()
    );
    for (key, value) in headers {
        response.push_str(&format!("{}: {}\r\n", key.into(), value.into()));
    }
    response.push_str("\r\n");
    response.push_str(&body);
    stream
        .write_all(response.as_bytes())
        .expect("write local response");
}
