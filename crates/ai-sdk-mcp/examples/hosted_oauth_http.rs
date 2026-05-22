use std::collections::BTreeMap;
use std::error::Error;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ai_sdk_mcp::{
    AuthOptions, AuthResult, CallToolResult, Configuration, InitializeResult, JsonRpcError,
    JsonRpcMessage, JsonRpcResponse, LATEST_PROTOCOL_VERSION, ListToolsResult, McpCallToolRequest,
    McpClientConfig, McpOAuthError, McpOAuthResult, McpTool, McpTransportConfig,
    OAuthClientInformation, OAuthClientInformationFull, OAuthClientMetadata, OAuthClientProvider,
    OAuthCredentialScope, OAuthTokens, ServerCapabilities, auth, create_mcp_client,
};
use ai_sdk_provider::{JsonObject, JsonSchema, JsonValue};
use serde_json::json;
use url::Url;

const ACCESS_TOKEN: &str = "local-hosted-oauth-token";

fn main() -> Result<(), Box<dyn Error>> {
    let server = LocalHostedOAuthMcpServer::start();
    let mcp_url = format!("{}/mcp", server.url());
    let mut auth_provider = InMemoryOAuthClientProvider::new(format!("{}/callback", server.url()));

    authorize_with_pkce_once(&mut auth_provider, &mcp_url)?;

    let client = create_mcp_client(
        McpClientConfig::from_transport_config(
            McpTransportConfig::http(mcp_url)
                .with_header("x-example", "hosted-oauth")
                .with_auth_provider(auth_provider),
        )
        .with_client_name("rust-hosted-oauth-http-example"),
    )?;

    println!("connected to {}", client.server_info()?.name);

    let definitions = client.list_tools(None)?;
    println!(
        "protected tools: {}",
        definitions
            .tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );

    let result = client.call_tool(
        McpCallToolRequest::new("get-secret-data")
            .with_arguments(json!({ "secretKey": "local-demo" })),
    )?;
    println!("protected tool result: {}", text_content(&result).join(" "));

    client.close()?;
    println!("oauth and mcp requests: {}", server.requests().len());

    Ok(())
}

fn authorize_with_pkce_once(
    provider: &mut InMemoryOAuthClientProvider,
    server_url: &str,
) -> Result<(), Box<dyn Error>> {
    match auth(provider, AuthOptions::new(server_url))? {
        AuthResult::Authorized => return Ok(()),
        AuthResult::Redirect => {}
    }

    let authorization_url = provider
        .last_authorization_url()
        .ok_or("authorization redirect was not captured")?;
    let callback_url = request_authorization_redirect(&authorization_url)?;
    let code = callback_url
        .query_pairs()
        .find_map(|(key, value)| (key == "code").then(|| value.into_owned()))
        .ok_or("authorization callback did not include code")?;
    let mut callback_options = AuthOptions::new(server_url).with_authorization_code(code);
    if let Some(state) = callback_url
        .query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
    {
        callback_options = callback_options.with_callback_state(state);
    }

    match auth(provider, callback_options)? {
        AuthResult::Authorized => Ok(()),
        AuthResult::Redirect => Err("authorization code exchange unexpectedly redirected".into()),
    }
}

fn request_authorization_redirect(authorization_url: &Url) -> Result<Url, Box<dyn Error>> {
    let response = ureq::get(authorization_url.as_str())
        .config()
        .http_status_as_error(false)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .build()
        .call()?;
    if response.status().as_u16() != 302 {
        return Err(format!("authorization endpoint returned {}", response.status()).into());
    }
    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .ok_or("authorization redirect did not include Location")?;
    Url::parse(location).map_err(Into::into)
}

fn text_content(result: &CallToolResult) -> Vec<String> {
    result
        .content
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter_map(|part| part.get("text").and_then(JsonValue::as_str))
        .map(ToString::to_string)
        .collect()
}

#[derive(Debug)]
struct InMemoryOAuthClientProvider {
    tokens: Option<OAuthTokens>,
    code_verifier: Option<String>,
    client_information: Option<OAuthClientInformationFull>,
    redirect_url: String,
    redirects: Vec<Url>,
    state: Option<String>,
}

impl InMemoryOAuthClientProvider {
    fn new(redirect_url: String) -> Self {
        Self {
            tokens: None,
            code_verifier: None,
            client_information: None,
            redirect_url,
            redirects: Vec::new(),
            state: None,
        }
    }

    fn last_authorization_url(&self) -> Option<Url> {
        self.redirects.last().cloned()
    }
}

impl OAuthClientProvider for InMemoryOAuthClientProvider {
    fn tokens(&self) -> McpOAuthResult<Option<OAuthTokens>> {
        Ok(self.tokens.clone())
    }

    fn save_tokens(&mut self, tokens: OAuthTokens) -> McpOAuthResult<()> {
        self.tokens = Some(tokens);
        Ok(())
    }

    fn redirect_to_authorization(&mut self, authorization_url: Url) -> McpOAuthResult<()> {
        self.redirects.push(authorization_url);
        Ok(())
    }

    fn save_code_verifier(&mut self, code_verifier: String) -> McpOAuthResult<()> {
        self.code_verifier = Some(code_verifier);
        Ok(())
    }

    fn code_verifier(&self) -> McpOAuthResult<String> {
        self.code_verifier
            .clone()
            .ok_or_else(|| McpOAuthError::new("No code verifier saved"))
    }

    fn redirect_url(&self) -> String {
        self.redirect_url.clone()
    }

    fn client_metadata(&self) -> OAuthClientMetadata {
        OAuthClientMetadata::new(vec![self.redirect_url.clone()])
            .with_client_name("Rust AI SDK MCP OAuth Example")
            .with_scope("protected-tools")
    }

    fn client_information(&self) -> McpOAuthResult<Option<OAuthClientInformation>> {
        Ok(self
            .client_information
            .as_ref()
            .map(|client| client.information.clone()))
    }

    fn can_save_client_information(&self) -> bool {
        true
    }

    fn save_client_information(
        &mut self,
        client_information: OAuthClientInformationFull,
    ) -> McpOAuthResult<()> {
        self.client_information = Some(client_information);
        Ok(())
    }

    fn state(&self) -> McpOAuthResult<Option<String>> {
        Ok(Some("local-oauth-state".to_string()))
    }

    fn can_save_state(&self) -> bool {
        true
    }

    fn save_state(&mut self, state: String) -> McpOAuthResult<()> {
        self.state = Some(state);
        Ok(())
    }

    fn stored_state(&self) -> McpOAuthResult<Option<String>> {
        Ok(self.state.clone())
    }

    fn invalidate_credentials(&mut self, scope: OAuthCredentialScope) -> McpOAuthResult<()> {
        match scope {
            OAuthCredentialScope::All => {
                self.tokens = None;
                self.client_information = None;
                self.code_verifier = None;
            }
            OAuthCredentialScope::Client => self.client_information = None,
            OAuthCredentialScope::Tokens => self.tokens = None,
            OAuthCredentialScope::Verifier => self.code_verifier = None,
        }
        Ok(())
    }
}

fn object_schema() -> JsonSchema {
    JsonObject::from_iter([
        ("type".to_string(), json!("object")),
        (
            "properties".to_string(),
            json!({
                "secretKey": { "type": "string" }
            }),
        ),
        ("required".to_string(), json!(["secretKey"])),
    ])
}

fn protected_tools() -> ListToolsResult {
    let mut tool = McpTool::new("get-secret-data", object_schema());
    tool.title = Some("Get Secret Data".to_string());
    tool.description = Some("Retrieve protected local demo data.".to_string());
    ListToolsResult {
        tools: vec![tool],
        ..ListToolsResult::default()
    }
}

fn tool_result(name: &str, arguments: Option<&JsonValue>) -> CallToolResult {
    match name {
        "get-secret-data" => {
            let secret_key = arguments
                .and_then(|value| value.get("secretKey"))
                .and_then(JsonValue::as_str)
                .unwrap_or("unknown");
            CallToolResult {
                content: Some(vec![json!({
                    "type": "text",
                    "text": format!("Secret data for {secret_key}: local OAuth access granted.")
                })]),
                is_error: Some(false),
                ..CallToolResult::default()
            }
        }
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
    target: String,
    headers: BTreeMap<String, String>,
    body: String,
}

struct LocalHostedOAuthMcpServer {
    url: String,
    requests: Arc<Mutex<Vec<LocalHttpRequest>>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl LocalHostedOAuthMcpServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local OAuth MCP server");
        listener
            .set_nonblocking(true)
            .expect("set local OAuth MCP server nonblocking");
        let url = format!("http://{}", listener.local_addr().expect("local address"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let handle_requests = Arc::clone(&requests);
        let handle_stop = Arc::clone(&stop);
        let handle_url = url.clone();

        let handle = thread::spawn(move || {
            while !handle_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        handle_local_connection(stream, &handle_requests, &handle_url);
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

impl Drop for LocalHostedOAuthMcpServer {
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
    base_url: &str,
) {
    let Some(request) = read_local_request(&mut stream) else {
        return;
    };
    requests
        .lock()
        .expect("local requests lock")
        .push(request.clone());

    let path = request.target.split('?').next().unwrap_or_default();
    match (request.method.as_str(), path) {
        ("GET", "/.well-known/oauth-protected-resource") => {
            write_json(
                &mut stream,
                json!({
                    "resource": format!("{base_url}/mcp"),
                    "authorization_servers": [base_url]
                }),
            );
        }
        ("GET", "/.well-known/oauth-authorization-server") => {
            write_json(
                &mut stream,
                json!({
                    "issuer": base_url,
                    "authorization_endpoint": format!("{base_url}/authorize"),
                    "token_endpoint": format!("{base_url}/token"),
                    "registration_endpoint": format!("{base_url}/register"),
                    "response_types_supported": ["code"],
                    "grant_types_supported": ["authorization_code", "refresh_token"],
                    "token_endpoint_auth_methods_supported": ["client_secret_post", "none"],
                    "code_challenge_methods_supported": ["S256"]
                }),
            );
        }
        ("POST", "/register") => {
            write_json(
                &mut stream,
                json!({
                    "client_id": "local-client",
                    "client_secret": "local-client-secret",
                    "client_id_issued_at": 1_700_000_000_u64,
                    "redirect_uris": [format!("{base_url}/callback")]
                }),
            );
        }
        ("GET", "/authorize") => write_authorization_redirect(&mut stream, base_url, &request),
        ("POST", "/token") => write_token_response(&mut stream, &request),
        ("GET", "/mcp") if is_authorized(&request) => {
            write_response(&mut stream, 405, [("content-type", "text/plain")], "");
        }
        ("POST", "/mcp") if is_authorized(&request) => write_mcp_response(&mut stream, &request),
        ("DELETE", "/mcp") if is_authorized(&request) => {
            write_response(&mut stream, 200, [("content-type", "text/plain")], "");
        }
        ("GET" | "POST" | "DELETE", "/mcp") => write_unauthorized(&mut stream, base_url),
        _ => write_response(
            &mut stream,
            404,
            [("content-type", "text/plain")],
            "not found",
        ),
    }
}

fn write_authorization_redirect(
    stream: &mut TcpStream,
    base_url: &str,
    request: &LocalHttpRequest,
) {
    let request_url = Url::parse(&format!("{base_url}{}", request.target))
        .expect("authorization request URL parses");
    let redirect_uri = request_url
        .query_pairs()
        .find_map(|(key, value)| (key == "redirect_uri").then(|| value.into_owned()))
        .expect("authorization request includes redirect_uri");
    let mut callback = Url::parse(&redirect_uri).expect("redirect URI parses");
    callback
        .query_pairs_mut()
        .append_pair("code", "local-auth-code");
    if let Some(state) = request_url
        .query_pairs()
        .find_map(|(key, value)| (key == "state").then(|| value.into_owned()))
    {
        callback.query_pairs_mut().append_pair("state", &state);
    }
    write_response(
        stream,
        302,
        [
            ("content-type", "text/plain"),
            ("location", callback.as_str()),
        ],
        "",
    );
}

fn write_token_response(stream: &mut TcpStream, request: &LocalHttpRequest) {
    let params = url::form_urlencoded::parse(request.body.as_bytes())
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    let valid = params.get("grant_type").map(String::as_str) == Some("authorization_code")
        && params.get("code").map(String::as_str) == Some("local-auth-code")
        && params.get("code_verifier").is_some()
        && params.get("client_id").map(String::as_str) == Some("local-client")
        && params.get("client_secret").map(String::as_str) == Some("local-client-secret");

    if valid {
        write_json(
            stream,
            json!({
                "access_token": ACCESS_TOKEN,
                "token_type": "Bearer",
                "expires_in": 3600,
                "refresh_token": "local-refresh-token"
            }),
        );
    } else {
        write_response(
            stream,
            400,
            [("content-type", "application/json")],
            json!({ "error": "invalid_grant" }).to_string(),
        );
    }
}

fn write_mcp_response(stream: &mut TcpStream, request: &LocalHttpRequest) {
    let body = serde_json::from_str::<JsonValue>(&request.body).unwrap_or(JsonValue::Null);
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
                server_info: Configuration::new("local-hosted-oauth-mcp-server", "1.0.0"),
                instructions: Some("Use the local OAuth-protected MCP tools.".to_string()),
                meta: None,
            },
        ),
        "tools/list" => JsonRpcResponse::success(id, protected_tools()),
        "tools/call" => {
            let result = body
                .get("params")
                .cloned()
                .and_then(|params| serde_json::from_value::<McpCallToolRequest>(params).ok())
                .map(|request| tool_result(&request.name, request.arguments.as_ref()))
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
            "hosted-oauth-session-1".to_string(),
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

fn is_authorized(request: &LocalHttpRequest) -> bool {
    let expected = format!("Bearer {ACCESS_TOKEN}");
    request.headers.get("authorization").map(String::as_str) == Some(expected.as_str())
}

fn write_unauthorized(stream: &mut TcpStream, base_url: &str) {
    write_response(
        stream,
        401,
        [
            ("content-type", "text/plain".to_string()),
            (
                "www-authenticate",
                format!(
                    "Bearer resource_metadata=\"{base_url}/.well-known/oauth-protected-resource\""
                ),
            ),
        ],
        "Unauthorized",
    );
}

fn read_local_request(stream: &mut TcpStream) -> Option<LocalHttpRequest> {
    stream.set_nonblocking(false).expect("local stream blocks");
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
    let target = request_parts.next()?.to_string();
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
        target,
        headers,
        body: body.to_string(),
    })
}

fn write_json(stream: &mut TcpStream, value: JsonValue) {
    write_response(
        stream,
        200,
        [("content-type", "application/json")],
        value.to_string(),
    );
}

fn write_response<K, V, I>(stream: &mut TcpStream, status: u16, headers: I, body: impl Into<String>)
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    let body = body.into();
    let status_text = match status {
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "OK",
    };
    let mut response = format!(
        "HTTP/1.1 {status} {status_text}\r\ncontent-length: {}\r\nconnection: close\r\n",
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
