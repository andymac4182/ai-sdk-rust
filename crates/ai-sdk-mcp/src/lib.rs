//! Portable MCP helpers for the Rust port of upstream `@ai-sdk/mcp`.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::string::FromUtf8Error;
use std::sync::{Arc, Mutex};

use ai_sdk_provider::{
    FileData, FileDataContent, JsonObject, JsonSchema, JsonValue, LanguageModelFilePart,
    LanguageModelTextPart, LanguageModelToolResultContentPart, LanguageModelToolResultOutput,
};
use ai_sdk_provider_utils::{
    Base64DecodeError, FlexibleSchema, ParseJsonResult, Tool, ToolExecutionError,
    ToolModelOutputOptions, ValidateTypesResult, convert_base64_to_bytes,
    safe_parse_json_with_schema, safe_validate_types, with_user_agent_suffix,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Latest MCP protocol version advertised by upstream `@ai-sdk/mcp`.
pub const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";

/// MCP protocol versions accepted by upstream `@ai-sdk/mcp`.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &[
    LATEST_PROTOCOL_VERSION,
    "2025-06-18",
    "2025-03-26",
    "2024-11-05",
];

/// MCP capability extension name used by hosts that can render MCP Apps.
pub const MCP_APP_EXTENSION_NAME: &str = "io.modelcontextprotocol/ui";

/// MIME type for HTML resources that are meant to be rendered as MCP Apps.
pub const MCP_APP_MIME_TYPE: &str = "text/html;profile=mcp-app";

/// Deprecated flat metadata key for app resource URIs.
pub const MCP_APP_LEGACY_RESOURCE_URI_META_KEY: &str = "ui/resourceUri";

/// JSON-RPC request id used by MCP transports.
pub type JsonRpcId = JsonValue;

/// JSON-RPC request message.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<JsonValue>,
}

impl JsonRpcRequest {
    /// Creates a JSON-RPC 2.0 request.
    pub fn new(id: impl Into<JsonRpcId>, method: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            method: method.into(),
            params: None,
        }
    }

    /// Sets request params.
    pub fn with_params(mut self, params: impl Into<JsonValue>) -> Self {
        self.params = Some(params.into());
        self
    }
}

/// JSON-RPC notification message.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<JsonValue>,
}

impl JsonRpcNotification {
    /// Creates a JSON-RPC 2.0 notification.
    pub fn new(method: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params: None,
        }
    }

    /// Sets notification params.
    pub fn with_params(mut self, params: impl Into<JsonValue>) -> Self {
        self.params = Some(params.into());
        self
    }
}

/// JSON-RPC error object.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<JsonValue>,
}

impl JsonRpcError {
    /// Creates a JSON-RPC error object.
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Sets error data.
    pub fn with_data(mut self, data: impl Into<JsonValue>) -> Self {
        self.data = Some(data.into());
        self
    }
}

/// JSON-RPC response message.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: JsonRpcId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// Creates a successful JSON-RPC 2.0 response.
    pub fn success(id: impl Into<JsonRpcId>, result: impl Serialize) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            result: Some(serde_json::to_value(result).expect("JSON-RPC result serializes")),
            error: None,
        }
    }

    /// Creates a JSON-RPC 2.0 error response.
    pub fn error(id: impl Into<JsonRpcId>, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            result: None,
            error: Some(error),
        }
    }
}

/// JSON-RPC message accepted by MCP transports.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Notification(JsonRpcNotification),
    Response(JsonRpcResponse),
}

/// Maps to `Implementation` in the MCP specification.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Configuration {
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

impl Configuration {
    /// Creates client or server implementation metadata.
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            title: None,
            extra: JsonObject::new(),
        }
    }
}

/// Client elicitation capability.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationCapability {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub apply_defaults: Option<bool>,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

/// MCP client capabilities.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<ElicitationCapability>,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

/// MCP server capabilities.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental: Option<JsonObject>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<JsonObject>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompts: Option<JsonObject>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<JsonObject>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<JsonObject>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elicitation: Option<ElicitationCapability>,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

/// MCP initialize result.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: Configuration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
}

/// MCP tool annotations.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolAnnotations {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

/// MCP tool definition.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub input_schema: JsonSchema,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<JsonSchema>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<McpToolAnnotations>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

impl McpTool {
    /// Creates an MCP tool definition.
    pub fn new(name: impl Into<String>, input_schema: JsonSchema) -> Self {
        Self {
            name: name.into(),
            title: None,
            description: None,
            input_schema,
            output_schema: None,
            annotations: None,
            meta: None,
            extra: JsonObject::new(),
        }
    }
}

/// MCP paginated tools result.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListToolsResult {
    pub tools: Vec<McpTool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
}

/// MCP resource definition.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResource {
    pub uri: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

/// MCP paginated resources result.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResourcesResult {
    pub resources: Vec<McpResource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
}

/// MCP resource template definition.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpResourceTemplate {
    pub uri_template: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

/// MCP resource templates result.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListResourceTemplatesResult {
    pub resource_templates: Vec<McpResourceTemplate>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
}

/// MCP resource contents containing text.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextResourceContent {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
    pub text: String,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

/// MCP resource contents containing a base64 blob.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobResourceContent {
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
    pub blob: String,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

/// MCP resource contents returned by `resources/read`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ResourceContent {
    Text(TextResourceContent),
    Blob(BlobResourceContent),
}

impl ResourceContent {
    fn uri(&self) -> &str {
        match self {
            Self::Text(content) => &content.uri,
            Self::Blob(content) => &content.uri,
        }
    }

    fn mime_type(&self) -> Option<&str> {
        match self {
            Self::Text(content) => content.mime_type.as_deref(),
            Self::Blob(content) => content.mime_type.as_deref(),
        }
    }

    fn meta(&self) -> Option<&JsonObject> {
        match self {
            Self::Text(content) => content.meta.as_ref(),
            Self::Blob(content) => content.meta.as_ref(),
        }
    }
}

/// MCP `resources/read` result.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadResourceResult {
    pub contents: Vec<ResourceContent>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
}

/// MCP prompt definition.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPrompt {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<McpPromptArgument>>,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

/// MCP prompt argument definition.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPromptArgument {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

/// MCP paginated prompts result.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPromptsResult {
    pub prompts: Vec<McpPrompt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
}

/// MCP prompt message returned by `prompts/get`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpPromptMessage {
    pub role: String,
    pub content: JsonValue,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

/// MCP `prompts/get` result.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPromptResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub messages: Vec<McpPromptMessage>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
}

/// MCP tool call result.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallToolResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Vec<JsonValue>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_content: Option<JsonValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<JsonValue>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

/// Elicitation request.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationRequest {
    pub method: String,
    pub params: ElicitationRequestParams,
}

/// Elicitation request params.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitationRequestParams {
    pub message: String,
    pub requested_schema: JsonValue,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
}

/// Elicitation result action.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ElicitAction {
    Accept,
    Decline,
    Cancel,
}

/// Elicitation result.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElicitResult {
    pub action: ElicitAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<JsonObject>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
}

/// Returns MCP App client capabilities.
pub fn mcp_app_client_capabilities() -> JsonValue {
    json!({
        "extensions": {
            MCP_APP_EXTENSION_NAME: {
                "mimeTypes": [MCP_APP_MIME_TYPE],
            },
        },
    })
}

/// MCP App tool visibility target.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum McpAppToolVisibility {
    Model,
    App,
}

/// Normalized `_meta.ui` metadata from an MCP tool definition.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct McpAppToolMeta {
    pub resource_uri: Option<String>,
    pub visibility: Option<Vec<McpAppToolVisibility>>,
    pub extra: JsonObject,
}

/// HTML and metadata needed by a host to render an MCP App.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct McpAppResource {
    pub uri: String,
    pub mime_type: String,
    pub html: String,
    pub meta: Option<JsonObject>,
}

/// Result of splitting MCP App tool visibility.
#[derive(Clone, Debug, PartialEq)]
pub struct SplitMcpAppTools {
    pub model_visible: ListToolsResult,
    pub app_visible: ListToolsResult,
}

/// Error returned while normalizing MCP App metadata or resources.
#[derive(Debug)]
pub enum McpAppError {
    InvalidResourceUri(String),
    UnsupportedResourceUri(String),
    ResourceNotFound(String),
    UnsupportedResourceMimeType {
        uri: String,
        mime_type: Option<String>,
    },
    UnsupportedResourceContent(String),
    InvalidResourceBlob {
        uri: String,
        source: Base64DecodeError,
    },
    InvalidResourceUtf8 {
        uri: String,
        source: FromUtf8Error,
    },
}

impl fmt::Display for McpAppError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidResourceUri(uri) => {
                write!(formatter, "Invalid MCP App resource URI: {uri}")
            }
            Self::UnsupportedResourceUri(uri) => {
                write!(formatter, "Unsupported MCP App resource URI: {uri}")
            }
            Self::ResourceNotFound(uri) => {
                write!(
                    formatter,
                    "MCP App resource not found in read result: {uri}"
                )
            }
            Self::UnsupportedResourceMimeType { mime_type, .. } => {
                write!(
                    formatter,
                    "Unsupported MCP App resource MIME type: {mime_type:?}"
                )
            }
            Self::UnsupportedResourceContent(uri) => {
                write!(
                    formatter,
                    "Unsupported MCP App resource content format: {uri}"
                )
            }
            Self::InvalidResourceBlob { uri, .. } => {
                write!(formatter, "Invalid MCP App resource blob: {uri}")
            }
            Self::InvalidResourceUtf8 { uri, .. } => {
                write!(formatter, "Invalid MCP App resource UTF-8: {uri}")
            }
        }
    }
}

impl std::error::Error for McpAppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidResourceBlob { source, .. } => Some(source),
            Self::InvalidResourceUtf8 { source, .. } => Some(source),
            Self::InvalidResourceUri(_)
            | Self::UnsupportedResourceUri(_)
            | Self::ResourceNotFound(_)
            | Self::UnsupportedResourceMimeType { .. }
            | Self::UnsupportedResourceContent(_) => None,
        }
    }
}

/// Error returned by the MCP client.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpClientError {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<JsonValue>,
}

impl McpClientError {
    /// Creates an MCP client error.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: None,
            data: None,
        }
    }

    /// Creates an MCP client error from a JSON-RPC error object.
    pub fn from_json_rpc(error: JsonRpcError) -> Self {
        Self {
            message: error.message,
            code: Some(error.code),
            data: error.data,
        }
    }
}

impl fmt::Display for McpClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for McpClientError {}

/// Result alias for MCP client operations.
pub type McpClientResult<T> = Result<T, McpClientError>;

type ElicitationRequestHandler =
    Arc<dyn Fn(ElicitationRequest) -> McpClientResult<ElicitResult> + Send + Sync>;

/// Environment variable names inherited by MCP stdio child processes.
pub fn default_stdio_inherited_environment_keys() -> &'static [&'static str] {
    if cfg!(windows) {
        &[
            "APPDATA",
            "HOMEDRIVE",
            "HOMEPATH",
            "LOCALAPPDATA",
            "PATH",
            "PROCESSOR_ARCHITECTURE",
            "SYSTEMDRIVE",
            "SYSTEMROOT",
            "TEMP",
            "USERNAME",
            "USERPROFILE",
        ]
    } else {
        &["HOME", "LOGNAME", "PATH", "SHELL", "TERM", "USER"]
    }
}

/// Constructs the environment for an MCP stdio child process.
///
/// Custom variables are copied first, then the upstream default inherited
/// variables from the current process are overlaid when present. Function-like
/// shell values that start with `()` are intentionally skipped.
pub fn get_stdio_environment(
    custom_env: Option<BTreeMap<String, String>>,
) -> BTreeMap<String, String> {
    get_stdio_environment_from(custom_env, std::env::vars())
}

fn get_stdio_environment_from<K, V, I>(
    custom_env: Option<BTreeMap<String, String>>,
    inherited_env: I,
) -> BTreeMap<String, String>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    let inherited_env = inherited_env
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect::<BTreeMap<String, String>>();
    let mut env = custom_env.unwrap_or_default();

    for key in default_stdio_inherited_environment_keys() {
        let Some(value) = inherited_env.get(*key) else {
            continue;
        };
        if value.starts_with("()") {
            continue;
        }
        env.insert((*key).to_string(), value.clone());
    }

    env
}

/// Serializes one JSON-RPC message for MCP stdio transport.
pub fn serialize_stdio_message(message: &JsonRpcMessage) -> String {
    format!(
        "{}\n",
        serde_json::to_string(message).expect("JSON-RPC stdio message serializes")
    )
}

/// Deserializes one newline-delimited JSON-RPC message from MCP stdio transport.
pub fn deserialize_stdio_message(line: &str) -> McpClientResult<JsonRpcMessage> {
    serde_json::from_str(line).map_err(|error| {
        McpClientError::new(format!(
            "MCP stdio Transport Error: Failed to parse message: {error}"
        ))
    })
}

/// Buffer for newline-delimited MCP stdio messages.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct StdioReadBuffer {
    buffer: Vec<u8>,
}

impl StdioReadBuffer {
    /// Appends raw stdout bytes.
    pub fn append(&mut self, chunk: impl AsRef<[u8]>) {
        self.buffer.extend_from_slice(chunk.as_ref());
    }

    /// Reads the next complete line, excluding the trailing newline.
    pub fn read_line(&mut self) -> Option<String> {
        let index = self.buffer.iter().position(|byte| *byte == b'\n')?;
        let line = String::from_utf8_lossy(&self.buffer[..index]).to_string();
        self.buffer.drain(..=index);
        Some(line)
    }

    /// Clears buffered data.
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

/// Transport interface for MCP JSON-RPC communication.
///
/// This is the Rust equivalent of upstream's transport boundary. Concrete
/// network transports are intentionally separate slices; this trait lets the
/// client lifecycle be tested against deterministic and custom transports.
pub trait McpTransport: Send {
    /// Starts the transport.
    fn start(&mut self) -> McpClientResult<()> {
        Ok(())
    }

    /// Sends one JSON-RPC message and returns messages synchronously produced by the server.
    fn send(&mut self, message: JsonRpcMessage) -> McpClientResult<Vec<JsonRpcMessage>>;

    /// Closes the transport.
    fn close(&mut self) -> McpClientResult<()> {
        Ok(())
    }

    /// Records the negotiated MCP protocol version on the transport.
    fn set_protocol_version(&mut self, _protocol_version: String) {}
}

/// Configuration for an MCP stdio child process transport.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StdioConfig {
    pub command: String,
    pub args: Vec<String>,
    pub env: Option<BTreeMap<String, String>>,
    pub cwd: Option<PathBuf>,
}

impl StdioConfig {
    /// Creates stdio transport configuration.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            args: Vec::new(),
            env: None,
            cwd: None,
        }
    }

    /// Adds one command argument.
    pub fn with_arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Sets command arguments.
    pub fn with_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Sets custom environment variables.
    pub fn with_env(mut self, env: BTreeMap<String, String>) -> Self {
        self.env = Some(env);
        self
    }

    /// Sets the child process working directory.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }
}

/// MCP stdio transport backed by a child process.
///
/// This synchronous Rust version writes one JSON-RPC message per line and, for
/// requests, reads one response line from stdout. It intentionally keeps
/// long-running async event handling and stderr policy as later transport
/// parity work.
#[derive(Debug)]
pub struct StdioMcpTransport {
    config: StdioConfig,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
    protocol_version: Option<String>,
}

impl StdioMcpTransport {
    /// Creates a stdio MCP transport.
    pub fn new(config: StdioConfig) -> Self {
        Self {
            config,
            child: None,
            stdin: None,
            stdout: None,
            protocol_version: None,
        }
    }

    /// Returns whether the child process is started.
    pub fn is_started(&self) -> bool {
        self.child.is_some()
    }

    /// Returns the negotiated protocol version, when set by the client.
    pub fn protocol_version(&self) -> Option<&str> {
        self.protocol_version.as_deref()
    }
}

impl McpTransport for StdioMcpTransport {
    fn start(&mut self) -> McpClientResult<()> {
        if self.child.is_some() {
            return Err(McpClientError::new("StdioMCPTransport already started."));
        }

        let mut command = Command::new(&self.config.command);
        command
            .args(&self.config.args)
            .env_clear()
            .envs(get_stdio_environment(self.config.env.clone()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if let Some(cwd) = &self.config.cwd {
            command.current_dir(cwd);
        }

        let mut child = command.spawn().map_err(|error| {
            McpClientError::new(format!(
                "MCP stdio Transport Error: failed to spawn process: {error}"
            ))
        })?;
        self.stdin = Some(child.stdin.take().ok_or_else(|| {
            McpClientError::new("MCP stdio Transport Error: child stdin is unavailable")
        })?);
        self.stdout = Some(BufReader::new(child.stdout.take().ok_or_else(|| {
            McpClientError::new("MCP stdio Transport Error: child stdout is unavailable")
        })?));
        self.child = Some(child);
        Ok(())
    }

    fn send(&mut self, message: JsonRpcMessage) -> McpClientResult<Vec<JsonRpcMessage>> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| McpClientError::new("StdioClientTransport not connected"))?;
        stdin
            .write_all(serialize_stdio_message(&message).as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|error| {
                McpClientError::new(format!(
                    "MCP stdio Transport Error: failed to write message: {error}"
                ))
            })?;

        if matches!(
            message,
            JsonRpcMessage::Notification(_) | JsonRpcMessage::Response(_)
        ) {
            return Ok(Vec::new());
        }

        let stdout = self
            .stdout
            .as_mut()
            .ok_or_else(|| McpClientError::new("StdioClientTransport not connected"))?;
        let mut line = String::new();
        let bytes = stdout.read_line(&mut line).map_err(|error| {
            McpClientError::new(format!(
                "MCP stdio Transport Error: failed to read message: {error}"
            ))
        })?;
        if bytes == 0 {
            return Err(McpClientError::new(
                "MCP stdio Transport Error: child process closed stdout",
            ));
        }
        Ok(vec![deserialize_stdio_message(line.trim_end())?])
    }

    fn close(&mut self) -> McpClientResult<()> {
        self.stdin = None;
        self.stdout = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    }

    fn set_protocol_version(&mut self, protocol_version: String) {
        self.protocol_version = Some(protocol_version);
    }
}

/// Configuration used to create an MCP client.
pub struct McpClientConfig {
    pub transport: Box<dyn McpTransport>,
    pub client_name: Option<String>,
    pub name: Option<String>,
    pub version: Option<String>,
    pub capabilities: Option<ClientCapabilities>,
}

impl McpClientConfig {
    /// Creates a client configuration from a transport.
    pub fn new(transport: impl McpTransport + 'static) -> Self {
        Self {
            transport: Box::new(transport),
            client_name: None,
            name: None,
            version: None,
            capabilities: None,
        }
    }

    /// Sets the client name advertised during initialization.
    pub fn with_client_name(mut self, client_name: impl Into<String>) -> Self {
        self.client_name = Some(client_name.into());
        self
    }

    /// Sets the deprecated client name field.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the client version advertised during initialization.
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Sets client capabilities advertised during initialization.
    pub fn with_capabilities(mut self, capabilities: ClientCapabilities) -> Self {
        self.capabilities = Some(capabilities);
        self
    }
}

/// Parameters for paginated MCP list requests.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedRequestParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<JsonObject>,
}

/// Arguments passed to `tools/call`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpCallToolRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<JsonValue>,
}

impl McpCallToolRequest {
    /// Creates a tool call request.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            arguments: None,
        }
    }

    /// Sets the tool call arguments.
    pub fn with_arguments(mut self, arguments: impl Into<JsonValue>) -> Self {
        self.arguments = Some(arguments.into());
        self
    }
}

/// Arguments passed to `prompts/get`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpGetPromptRequest {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<JsonValue>,
}

impl McpGetPromptRequest {
    /// Creates a prompt request.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            arguments: None,
        }
    }

    /// Sets prompt arguments.
    pub fn with_arguments(mut self, arguments: impl Into<JsonValue>) -> Self {
        self.arguments = Some(arguments.into());
        self
    }
}

/// Optional schema overrides used when creating AI SDK tools from MCP tools.
#[derive(Clone, Debug, Default)]
pub struct McpToolSchema {
    pub input_schema: Option<JsonSchema>,
    pub output_schema: Option<FlexibleSchema<JsonValue>>,
}

impl McpToolSchema {
    /// Creates an empty MCP tool schema override.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the input schema used for the generated AI SDK tool.
    pub fn with_input_schema(mut self, input_schema: JsonSchema) -> Self {
        self.input_schema = Some(input_schema);
        self
    }

    /// Sets the output schema used to validate successful MCP tool results.
    pub fn with_output_schema(
        mut self,
        output_schema: impl Into<FlexibleSchema<JsonValue>>,
    ) -> Self {
        self.output_schema = Some(output_schema.into());
        self
    }
}

/// Tool schema overrides keyed by MCP tool name.
pub type McpToolSchemas = BTreeMap<String, McpToolSchema>;

/// AI SDK tools created from MCP tool definitions.
pub type McpToolSet = BTreeMap<String, Tool>;

/// Creates and initializes an MCP client.
pub fn create_mcp_client(config: McpClientConfig) -> McpClientResult<McpClient> {
    let client_name = config
        .client_name
        .or(config.name)
        .unwrap_or_else(|| "ai-sdk-mcp-client".to_string());
    let version = config.version.unwrap_or_else(|| "1.0.0".to_string());
    let client = McpClient {
        inner: Arc::new(Mutex::new(McpClientInner {
            transport: config.transport,
            client_info: Configuration::new(client_name, version),
            client_capabilities: config.capabilities.unwrap_or_default(),
            request_message_id: 0,
            server_capabilities: ServerCapabilities::default(),
            server_info: Configuration::new("", ""),
            instructions: None,
            is_closed: true,
            elicitation_request_handler: None,
        })),
    };

    if let Err(error) = client.init() {
        let _ = client.close();
        return Err(error);
    }

    Ok(client)
}

/// Lightweight MCP client that mirrors upstream request lifecycle behavior.
#[derive(Clone)]
pub struct McpClient {
    inner: Arc<Mutex<McpClientInner>>,
}

struct McpClientInner {
    transport: Box<dyn McpTransport>,
    client_info: Configuration,
    client_capabilities: ClientCapabilities,
    request_message_id: u64,
    server_capabilities: ServerCapabilities,
    server_info: Configuration,
    instructions: Option<String>,
    is_closed: bool,
    elicitation_request_handler: Option<ElicitationRequestHandler>,
}

impl McpClient {
    /// Creates and initializes an MCP client.
    pub fn new(config: McpClientConfig) -> McpClientResult<Self> {
        create_mcp_client(config)
    }

    /// Returns information about the initialized MCP server.
    pub fn server_info(&self) -> McpClientResult<Configuration> {
        self.with_inner(|inner| Ok(inner.server_info.clone()))
    }

    /// Returns optional instructions provided by the initialized MCP server.
    pub fn instructions(&self) -> McpClientResult<Option<String>> {
        self.with_inner(|inner| Ok(inner.instructions.clone()))
    }

    /// Lists available MCP tools.
    pub fn list_tools(
        &self,
        params: Option<PaginatedRequestParams>,
    ) -> McpClientResult<ListToolsResult> {
        self.with_inner(|inner| inner.request("tools/list", optional_params_value(params)?))
    }

    /// Calls an MCP tool.
    pub fn call_tool(&self, request: McpCallToolRequest) -> McpClientResult<CallToolResult> {
        self.with_inner(|inner| inner.request("tools/call", Some(to_json_value(request)?)))
    }

    /// Creates executable dynamic AI SDK tools from the server's tool list.
    pub fn tools(&self) -> McpClientResult<McpToolSet> {
        let definitions = self.list_tools(None)?;
        self.tools_from_definitions(&definitions)
    }

    /// Creates executable AI SDK tools with explicit schema overrides.
    ///
    /// When schema overrides are supplied, only tools listed in `schemas` are
    /// returned, matching upstream `tools({ schemas })` filtering.
    pub fn tools_with_schemas(&self, schemas: &McpToolSchemas) -> McpClientResult<McpToolSet> {
        let definitions = self.list_tools(None)?;
        self.tools_from_definitions_with_schemas(&definitions, schemas)
    }

    /// Creates executable dynamic AI SDK tools from cached MCP tool definitions.
    pub fn tools_from_definitions(
        &self,
        definitions: &ListToolsResult,
    ) -> McpClientResult<McpToolSet> {
        self.tools_from_definitions_inner(definitions, None)
    }

    /// Creates executable AI SDK tools from cached MCP definitions with schema overrides.
    pub fn tools_from_definitions_with_schemas(
        &self,
        definitions: &ListToolsResult,
        schemas: &McpToolSchemas,
    ) -> McpClientResult<McpToolSet> {
        self.tools_from_definitions_inner(definitions, Some(schemas))
    }

    fn tools_from_definitions_inner(
        &self,
        definitions: &ListToolsResult,
        schemas: Option<&McpToolSchemas>,
    ) -> McpClientResult<McpToolSet> {
        let client_name = self.with_inner(|inner| Ok(inner.client_info.name.clone()))?;
        let mut tools = BTreeMap::new();

        for definition in &definitions.tools {
            let schema = schemas.and_then(|schemas| schemas.get(&definition.name));
            if schemas.is_some() && schema.is_none() {
                continue;
            }

            let output_schema = schema.and_then(|schema| schema.output_schema.clone());
            let output_json_schema = output_schema
                .as_ref()
                .map(|schema| schema.as_schema().json_schema().clone());
            let mut input_schema = schema
                .and_then(|schema| schema.input_schema.clone())
                .unwrap_or_else(|| definition.input_schema.clone());
            input_schema
                .entry("properties".to_string())
                .or_insert_with(|| json!({}));
            input_schema.insert("additionalProperties".to_string(), json!(false));

            let metadata = mcp_provider_metadata(client_name.clone(), definition)
                .map_err(|error| McpClientError::new(error.to_string()))?;
            let client = self.clone();
            let tool_name = definition.name.clone();
            let output_tool_name = definition.name.clone();
            let mut tool = Tool::dynamic(definition.name.clone(), input_schema)
                .with_metadata(metadata)
                .with_execute(move |input, _options| {
                    let client = client.clone();
                    let tool_name = tool_name.clone();
                    let output_schema = output_schema.clone();
                    async move {
                        let result = client
                            .call_tool(
                                McpCallToolRequest::new(tool_name.clone()).with_arguments(input),
                            )
                            .map_err(|error| ToolExecutionError::new(error.to_string()))?;
                        if result.is_error == Some(true) {
                            return to_json_value(result)
                                .map_err(|error| ToolExecutionError::new(error.message));
                        }
                        match output_schema {
                            Some(output_schema) => {
                                extract_mcp_structured_content(&result, output_schema, &tool_name)
                                    .map_err(|error| ToolExecutionError::new(error.message))
                            }
                            None => to_json_value(result)
                                .map_err(|error| ToolExecutionError::new(error.message)),
                        }
                    }
                })
                .with_to_model_output(|options: ToolModelOutputOptions| async move {
                    serde_json::from_value::<CallToolResult>(options.output)
                        .map(|result| mcp_to_model_output(&result))
                        .unwrap_or_else(|error| {
                            LanguageModelToolResultOutput::json(json!({
                                "error": error.to_string(),
                            }))
                        })
                });

            if let Some(description) = &definition.description {
                tool = tool.with_description(description.clone());
            }
            if let Some(output_json_schema) = output_json_schema {
                tool = tool.with_output_schema(output_json_schema);
            }
            if let Some(title) = definition.title.clone().or_else(|| {
                definition
                    .annotations
                    .as_ref()
                    .and_then(|annotations| annotations.title.clone())
            }) {
                tool = tool.with_title(title);
            }

            tools.insert(output_tool_name, tool);
        }

        Ok(tools)
    }

    /// Lists MCP resources.
    pub fn list_resources(
        &self,
        params: Option<PaginatedRequestParams>,
    ) -> McpClientResult<ListResourcesResult> {
        self.with_inner(|inner| inner.request("resources/list", optional_params_value(params)?))
    }

    /// Reads one MCP resource.
    pub fn read_resource(&self, uri: impl Into<String>) -> McpClientResult<ReadResourceResult> {
        self.with_inner(|inner| inner.request("resources/read", Some(json!({ "uri": uri.into() }))))
    }

    /// Lists MCP resource templates.
    pub fn list_resource_templates(&self) -> McpClientResult<ListResourceTemplatesResult> {
        self.with_inner(|inner| inner.request("resources/templates/list", None))
    }

    /// Lists MCP prompts.
    pub fn list_prompts(
        &self,
        params: Option<PaginatedRequestParams>,
    ) -> McpClientResult<ListPromptsResult> {
        self.with_inner(|inner| inner.request("prompts/list", optional_params_value(params)?))
    }

    /// Gets one MCP prompt.
    pub fn get_prompt(&self, request: McpGetPromptRequest) -> McpClientResult<GetPromptResult> {
        self.with_inner(|inner| inner.request("prompts/get", Some(to_json_value(request)?)))
    }

    /// Registers a handler for server `elicitation/create` requests.
    ///
    /// This mirrors upstream `onElicitationRequest` with a synchronous Rust
    /// callback. The client replies to the server with the returned
    /// [`ElicitResult`] while continuing to wait for the original request's
    /// response.
    pub fn on_elicitation_request(
        &self,
        handler: impl Fn(ElicitationRequest) -> McpClientResult<ElicitResult> + Send + Sync + 'static,
    ) -> McpClientResult<()> {
        let handler = Arc::new(handler);
        self.with_inner(|inner| {
            inner.elicitation_request_handler = Some(handler);
            Ok(())
        })
    }

    /// Closes the client transport.
    pub fn close(&self) -> McpClientResult<()> {
        self.with_inner(|inner| {
            if inner.is_closed {
                return Ok(());
            }
            inner.transport.close()?;
            inner.on_close();
            Ok(())
        })
    }

    fn init(&self) -> McpClientResult<()> {
        self.with_inner(|inner| {
            inner.transport.start()?;
            inner.is_closed = false;
            let result: InitializeResult = inner.request(
                "initialize",
                Some(json!({
                    "protocolVersion": LATEST_PROTOCOL_VERSION,
                    "capabilities": inner.client_capabilities.clone(),
                    "clientInfo": inner.client_info.clone(),
                })),
            )?;

            if !SUPPORTED_PROTOCOL_VERSIONS.contains(&result.protocol_version.as_str()) {
                return Err(McpClientError::new(format!(
                    "Server's protocol version is not supported: {}",
                    result.protocol_version
                )));
            }

            inner.server_capabilities = result.capabilities;
            inner.server_info = result.server_info;
            inner.instructions = result.instructions;
            inner
                .transport
                .set_protocol_version(result.protocol_version.clone());
            inner.notification("notifications/initialized", None)
        })
    }

    fn with_inner<T>(
        &self,
        action: impl FnOnce(&mut McpClientInner) -> McpClientResult<T>,
    ) -> McpClientResult<T> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| McpClientError::new("MCP client mutex is poisoned"))?;
        action(&mut inner)
    }
}

impl McpClientInner {
    fn assert_capability(&self, method: &str) -> McpClientResult<()> {
        match method {
            "initialize" => Ok(()),
            "tools/list" | "tools/call" => {
                if self.server_capabilities.tools.is_some() {
                    Ok(())
                } else {
                    Err(McpClientError::new("Server does not support tools"))
                }
            }
            "resources/list" | "resources/read" | "resources/templates/list" => {
                if self.server_capabilities.resources.is_some() {
                    Ok(())
                } else {
                    Err(McpClientError::new("Server does not support resources"))
                }
            }
            "prompts/list" | "prompts/get" => {
                if self.server_capabilities.prompts.is_some() {
                    Ok(())
                } else {
                    Err(McpClientError::new("Server does not support prompts"))
                }
            }
            _ => Err(McpClientError::new(format!("Unsupported method: {method}"))),
        }
    }

    fn request<T: DeserializeOwned>(
        &mut self,
        method: &str,
        params: Option<JsonValue>,
    ) -> McpClientResult<T> {
        if self.is_closed {
            return Err(McpClientError::new(
                "Attempted to send a request from a closed client",
            ));
        }
        self.assert_capability(method)?;

        let message_id = self.request_message_id;
        self.request_message_id += 1;
        let response_id = json!(message_id);
        let messages = self
            .transport
            .send(JsonRpcMessage::Request(JsonRpcRequest {
                jsonrpc: "2.0".to_string(),
                id: response_id.clone(),
                method: method.to_string(),
                params,
            }))?;

        if messages.is_empty() {
            return Err(McpClientError::new(format!(
                "No response received for MCP request: {method}"
            )));
        };

        let mut matched_response = None;
        for message in messages {
            match message {
                JsonRpcMessage::Response(response) if response.id == response_id => {
                    matched_response = Some(Self::parse_response_result(response)?);
                }
                JsonRpcMessage::Response(response) => {
                    return Err(McpClientError::new(format!(
                        "Protocol error: Received a response for an unknown message ID: {}",
                        serde_json::to_string(&response).expect("response serializes")
                    )));
                }
                JsonRpcMessage::Request(request) => self.on_request_message(request)?,
                JsonRpcMessage::Notification(notification) => {
                    return Err(McpClientError::new(format!(
                        "Unsupported notification method: {}",
                        notification.method
                    )));
                }
            }
        }

        matched_response.ok_or_else(|| {
            McpClientError::new(format!("No response received for MCP request: {method}"))
        })
    }

    fn parse_response_result<T: DeserializeOwned>(response: JsonRpcResponse) -> McpClientResult<T> {
        if let Some(error) = response.error {
            return Err(McpClientError::from_json_rpc(error));
        }
        let result = response
            .result
            .ok_or_else(|| McpClientError::new("Server response did not include a result"))?;
        serde_json::from_value::<T>(result).map_err(|error| {
            McpClientError::new(format!("Failed to parse server response: {error}"))
        })
    }

    fn on_request_message(&mut self, request: JsonRpcRequest) -> McpClientResult<()> {
        if request.method != "elicitation/create" {
            return self.send_response(JsonRpcResponse::error(
                request.id,
                JsonRpcError::new(
                    -32601,
                    format!("Unsupported request method: {}", request.method),
                ),
            ));
        }

        let Some(handler) = self.elicitation_request_handler.clone() else {
            return self.send_response(JsonRpcResponse::error(
                request.id,
                JsonRpcError::new(-32601, "No elicitation handler registered on client"),
            ));
        };

        let parsed_request = serde_json::from_value::<ElicitationRequest>(json!({
            "method": request.method,
            "params": request.params,
        }));
        let parsed_request = match parsed_request {
            Ok(parsed_request) => parsed_request,
            Err(error) => {
                return self.send_response(JsonRpcResponse::error(
                    request.id,
                    JsonRpcError::new(-32602, format!("Invalid elicitation request: {error}"))
                        .with_data(json!({ "error": error.to_string() })),
                ));
            }
        };

        match handler(parsed_request) {
            Ok(result) => self.send_response(JsonRpcResponse::success(request.id, result)),
            Err(error) => self.send_response(JsonRpcResponse::error(
                request.id,
                JsonRpcError::new(-32603, error.message),
            )),
        }
    }

    fn send_response(&mut self, response: JsonRpcResponse) -> McpClientResult<()> {
        let messages = self.transport.send(JsonRpcMessage::Response(response))?;
        if messages.is_empty() {
            Ok(())
        } else {
            Err(McpClientError::new(
                "Transport returned messages for a response",
            ))
        }
    }

    fn notification(&mut self, method: &str, params: Option<JsonValue>) -> McpClientResult<()> {
        if self.is_closed {
            return Err(McpClientError::new(
                "Attempted to send a notification from a closed client",
            ));
        }
        let messages = self
            .transport
            .send(JsonRpcMessage::Notification(JsonRpcNotification {
                jsonrpc: "2.0".to_string(),
                method: method.to_string(),
                params,
            }))?;
        if messages.is_empty() {
            Ok(())
        } else {
            Err(McpClientError::new(
                "Transport returned messages for a notification",
            ))
        }
    }

    fn on_close(&mut self) {
        self.is_closed = true;
    }
}

fn to_json_value(value: impl Serialize) -> McpClientResult<JsonValue> {
    serde_json::to_value(value)
        .map_err(|error| McpClientError::new(format!("Failed to serialize MCP value: {error}")))
}

fn optional_params_value<T: Serialize>(value: Option<T>) -> McpClientResult<Option<JsonValue>> {
    value.map(to_json_value).transpose()
}

fn extract_mcp_structured_content(
    result: &CallToolResult,
    output_schema: FlexibleSchema<JsonValue>,
    tool_name: &str,
) -> McpClientResult<JsonValue> {
    if let Some(structured_content) = &result.structured_content {
        return match safe_validate_types(structured_content.clone(), output_schema, None) {
            ValidateTypesResult::Success { value, .. } => Ok(value),
            ValidateTypesResult::Failure { .. } => Err(McpClientError::new(format!(
                "Tool \"{tool_name}\" returned structuredContent that does not match the expected outputSchema"
            ))),
        };
    }

    if let Some(content) = &result.content {
        if let Some(text) = content
            .iter()
            .find(|part| part.get("type").and_then(JsonValue::as_str) == Some("text"))
            .and_then(|part| part.get("text"))
            .and_then(JsonValue::as_str)
        {
            return match safe_parse_json_with_schema(text, output_schema) {
                ParseJsonResult::Success { value, .. } => Ok(value),
                ParseJsonResult::Failure { .. } => Err(McpClientError::new(format!(
                    "Tool \"{tool_name}\" returned content that does not match the expected outputSchema"
                ))),
            };
        }
    }

    Err(McpClientError::new(format!(
        "Tool \"{tool_name}\" did not return structuredContent or parseable text content"
    )))
}

/// Streamable HTTP MCP transport.
///
/// This synchronous Rust transport covers the portable HTTP request/response
/// contract: POST JSON-RPC, protocol/session headers, JSON and SSE response
/// payload parsing, 202 notification acceptance, and DELETE session cleanup.
#[derive(Clone, Debug)]
pub struct McpHttpTransport {
    url: String,
    headers: BTreeMap<String, String>,
    session_id: Option<String>,
    protocol_version: Option<String>,
    started: bool,
}

impl McpHttpTransport {
    /// Creates a Streamable HTTP MCP transport.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: BTreeMap::new(),
            session_id: None,
            protocol_version: None,
            started: false,
        }
    }

    /// Adds a request header sent with transport requests.
    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Adds request headers sent with transport requests.
    pub fn with_headers<K, V, I>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.headers.extend(
            headers
                .into_iter()
                .map(|(key, value)| (key.into(), value.into())),
        );
        self
    }

    /// Returns the current MCP session id, when the server has provided one.
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Returns the protocol version used for request headers.
    pub fn protocol_version(&self) -> &str {
        self.protocol_version
            .as_deref()
            .unwrap_or(LATEST_PROTOCOL_VERSION)
    }

    fn common_headers(
        &self,
        base: impl IntoIterator<Item = (&'static str, &'static str)>,
    ) -> BTreeMap<String, String> {
        let mut headers = self.headers.clone();
        for (key, value) in base {
            headers.insert(key.to_string(), value.to_string());
        }
        headers.insert(
            "mcp-protocol-version".to_string(),
            self.protocol_version().to_string(),
        );
        if let Some(session_id) = &self.session_id {
            headers.insert("mcp-session-id".to_string(), session_id.clone());
        }

        with_user_agent_suffix(
            Some(headers.into_iter().map(|(key, value)| (key, Some(value)))),
            [format!("ai-sdk/{}", env!("CARGO_PKG_VERSION"))],
        )
        .into_iter()
        .collect()
    }

    fn execute_post(&mut self, message: &JsonRpcMessage) -> McpClientResult<Vec<JsonRpcMessage>> {
        let body = serde_json::to_string(message).map_err(|error| {
            McpClientError::new(format!("Failed to serialize MCP message: {error}"))
        })?;
        let mut builder = ureq::post(&self.url);
        for (name, value) in self.common_headers([
            ("Content-Type", "application/json"),
            ("Accept", "application/json, text/event-stream"),
        ]) {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let response = builder
            .config()
            .http_status_as_error(false)
            .build()
            .send(body)
            .map_err(|error| {
                McpClientError::new(format!("MCP HTTP Transport Error: fetch failed: {error}"))
            })?;

        self.handle_post_response(message, response)
    }

    fn handle_post_response(
        &mut self,
        message: &JsonRpcMessage,
        mut response: ureq::http::Response<ureq::Body>,
    ) -> McpClientResult<Vec<JsonRpcMessage>> {
        if let Some(session_id) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
        {
            self.session_id = Some(session_id.to_string());
        }

        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let body = response.body_mut().read_to_string().map_err(|error| {
            McpClientError::new(format!(
                "MCP HTTP Transport Error: failed to read response body: {error}"
            ))
        })?;

        if status == 202 {
            return Ok(Vec::new());
        }

        if !(200..300).contains(&status) {
            let mut message =
                format!("MCP HTTP Transport Error: POSTing to endpoint (HTTP {status}): {body}");
            if status == 404 {
                message.push_str(
                    ". This server does not support HTTP transport. Try using `sse` transport instead",
                );
            }
            return Err(McpClientError::new(message));
        }

        let is_notification = matches!(message, JsonRpcMessage::Notification(_));
        if is_notification {
            return Ok(Vec::new());
        }

        if content_type.contains("application/json") || content_type.is_empty() {
            parse_json_rpc_http_body(&body)
        } else if content_type.contains("text/event-stream") {
            parse_json_rpc_sse_body(&body)
        } else {
            Err(McpClientError::new(format!(
                "MCP HTTP Transport Error: unsupported response content-type: {content_type}"
            )))
        }
    }
}

impl McpTransport for McpHttpTransport {
    fn start(&mut self) -> McpClientResult<()> {
        if self.started {
            return Err(McpClientError::new(
                "MCP HTTP Transport Error: Transport already started",
            ));
        }
        self.started = true;
        Ok(())
    }

    fn send(&mut self, message: JsonRpcMessage) -> McpClientResult<Vec<JsonRpcMessage>> {
        if !self.started {
            self.start()?;
        }
        self.execute_post(&message)
    }

    fn close(&mut self) -> McpClientResult<()> {
        if let Some(session_id) = self.session_id.clone() {
            let mut builder = ureq::delete(&self.url);
            for (name, value) in self.common_headers([]) {
                builder = builder.header(name.as_str(), value.as_str());
            }
            builder = builder.header("mcp-session-id", session_id.as_str());
            let _ = builder.config().http_status_as_error(false).build().call();
        }
        self.started = false;
        Ok(())
    }

    fn set_protocol_version(&mut self, protocol_version: String) {
        self.protocol_version = Some(protocol_version);
    }
}

fn parse_json_rpc_http_body(body: &str) -> McpClientResult<Vec<JsonRpcMessage>> {
    let value = serde_json::from_str::<JsonValue>(body).map_err(|error| {
        McpClientError::new(format!(
            "MCP HTTP Transport Error: Failed to parse JSON response: {error}"
        ))
    })?;
    if let Some(messages) = value.as_array() {
        messages
            .iter()
            .cloned()
            .map(parse_json_rpc_value)
            .collect::<McpClientResult<Vec<_>>>()
    } else {
        Ok(vec![parse_json_rpc_value(value)?])
    }
}

fn parse_json_rpc_value(value: JsonValue) -> McpClientResult<JsonRpcMessage> {
    serde_json::from_value(value).map_err(|error| {
        McpClientError::new(format!(
            "MCP HTTP Transport Error: Failed to parse message: {error}"
        ))
    })
}

fn parse_json_rpc_sse_body(body: &str) -> McpClientResult<Vec<JsonRpcMessage>> {
    let mut messages = Vec::new();
    let mut event_name = String::new();
    let mut data_lines = Vec::new();

    for line in body.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if event_name == "message" && !data_lines.is_empty() {
                messages.push(parse_json_rpc_http_body(&data_lines.join("\n"))?);
            }
            event_name.clear();
            data_lines.clear();
            continue;
        }

        if let Some(event) = line.strip_prefix("event:") {
            event_name = event.trim().to_string();
        } else if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.trim_start().to_string());
        }
    }

    Ok(messages.into_iter().flatten().collect())
}

/// Deterministic in-process MCP transport used by tests and examples.
#[derive(Clone)]
pub struct MockMcpTransport {
    state: Arc<Mutex<MockMcpTransportState>>,
}

#[derive(Clone, Debug)]
struct MockMcpTransportState {
    tools: Vec<McpTool>,
    resources: Vec<McpResource>,
    resource_templates: Vec<McpResourceTemplate>,
    resource_contents: Vec<ResourceContent>,
    prompts: Vec<McpPrompt>,
    prompt_results: BTreeMap<String, GetPromptResult>,
    fail_on_invalid_tool_params: bool,
    initialize_result: Option<InitializeResult>,
    send_error: Option<McpClientError>,
    tool_call_results: BTreeMap<String, CallToolResult>,
    sent_messages: Vec<JsonRpcMessage>,
    closed: bool,
    protocol_version: Option<String>,
}

impl Default for MockMcpTransport {
    fn default() -> Self {
        let prompt_result = GetPromptResult {
            description: Some("Code review prompt".to_string()),
            messages: vec![McpPromptMessage {
                role: "user".to_string(),
                content: json!({
                    "type": "text",
                    "text": "Please review this code:\nfunction add(a, b) { return a + b; }",
                }),
                extra: JsonObject::new(),
            }],
            meta: None,
        };

        Self {
            state: Arc::new(Mutex::new(MockMcpTransportState {
                tools: vec![
                    McpTool {
                        description: Some("A mock tool for testing".to_string()),
                        ..McpTool::new("mock-tool", default_tool_schema())
                    },
                    McpTool {
                        description: Some("A mock tool for testing".to_string()),
                        ..McpTool::new(
                            "mock-tool-no-args",
                            JsonObject::from_iter([("type".to_string(), json!("object"))]),
                        )
                    },
                ],
                resources: vec![McpResource {
                    uri: "file:///mock/resource.txt".to_string(),
                    name: "resource.txt".to_string(),
                    title: None,
                    description: Some("Mock resource".to_string()),
                    mime_type: Some("text/plain".to_string()),
                    size: None,
                    extra: JsonObject::new(),
                }],
                resource_templates: vec![McpResourceTemplate {
                    uri_template: "file:///{path}".to_string(),
                    name: "mock-template".to_string(),
                    title: None,
                    description: Some("Mock template".to_string()),
                    mime_type: None,
                    extra: JsonObject::new(),
                }],
                resource_contents: vec![ResourceContent::Text(TextResourceContent {
                    uri: "file:///mock/resource.txt".to_string(),
                    name: None,
                    title: None,
                    mime_type: Some("text/plain".to_string()),
                    meta: None,
                    text: "Mock resource content".to_string(),
                    extra: JsonObject::new(),
                })],
                prompts: vec![McpPrompt {
                    name: "code_review".to_string(),
                    title: Some("Request Code Review".to_string()),
                    description: Some(
                        "Asks the LLM to analyze code quality and suggest improvements".to_string(),
                    ),
                    arguments: Some(vec![McpPromptArgument {
                        name: "code".to_string(),
                        description: Some("The code to review".to_string()),
                        required: Some(true),
                        extra: JsonObject::new(),
                    }]),
                    extra: JsonObject::new(),
                }],
                prompt_results: BTreeMap::from([("code_review".to_string(), prompt_result)]),
                fail_on_invalid_tool_params: false,
                initialize_result: None,
                send_error: None,
                tool_call_results: BTreeMap::new(),
                sent_messages: Vec::new(),
                closed: false,
                protocol_version: None,
            })),
        }
    }
}

impl MockMcpTransport {
    /// Creates a mock transport with upstream-like default fixtures.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the available tool definitions.
    pub fn with_tools(self, tools: impl IntoIterator<Item = McpTool>) -> Self {
        self.state.lock().expect("mock transport state").tools = tools.into_iter().collect();
        self
    }

    /// Replaces the available resource definitions.
    pub fn with_resources(self, resources: impl IntoIterator<Item = McpResource>) -> Self {
        self.state.lock().expect("mock transport state").resources =
            resources.into_iter().collect();
        self
    }

    /// Replaces resource contents returned by `resources/read`.
    pub fn with_resource_contents(
        self,
        resource_contents: impl IntoIterator<Item = ResourceContent>,
    ) -> Self {
        self.state
            .lock()
            .expect("mock transport state")
            .resource_contents = resource_contents.into_iter().collect();
        self
    }

    /// Replaces the initialize result returned by the mock server.
    pub fn with_initialize_result(self, initialize_result: InitializeResult) -> Self {
        self.state
            .lock()
            .expect("mock transport state")
            .initialize_result = Some(initialize_result);
        self
    }

    /// Causes `tools/call` to return invalid-parameters errors.
    pub fn with_fail_on_invalid_tool_params(self, fail_on_invalid_tool_params: bool) -> Self {
        self.state
            .lock()
            .expect("mock transport state")
            .fail_on_invalid_tool_params = fail_on_invalid_tool_params;
        self
    }

    /// Configures a custom result for a named tool.
    pub fn with_tool_call_result(
        self,
        tool_name: impl Into<String>,
        result: CallToolResult,
    ) -> Self {
        self.state
            .lock()
            .expect("mock transport state")
            .tool_call_results
            .insert(tool_name.into(), result);
        self
    }

    /// Returns messages sent by the client.
    pub fn sent_messages(&self) -> Vec<JsonRpcMessage> {
        self.state
            .lock()
            .expect("mock transport state")
            .sent_messages
            .clone()
    }

    /// Returns the protocol version negotiated by the client.
    pub fn negotiated_protocol_version(&self) -> Option<String> {
        self.state
            .lock()
            .expect("mock transport state")
            .protocol_version
            .clone()
    }

    /// Returns whether the transport has been closed.
    pub fn is_closed(&self) -> bool {
        self.state.lock().expect("mock transport state").closed
    }
}

impl McpTransport for MockMcpTransport {
    fn send(&mut self, message: JsonRpcMessage) -> McpClientResult<Vec<JsonRpcMessage>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| McpClientError::new("Mock MCP transport mutex is poisoned"))?;
        if let Some(error) = &state.send_error {
            return Err(error.clone());
        }
        state.sent_messages.push(message.clone());

        let JsonRpcMessage::Request(request) = message else {
            return Ok(Vec::new());
        };

        match request.method.as_str() {
            "initialize" => Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::success(
                request.id,
                state
                    .initialize_result
                    .clone()
                    .unwrap_or_else(|| mock_initialize_result(&state)),
            ))]),
            "tools/list" => {
                if state.tools.is_empty() {
                    return Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::error(
                        request.id,
                        JsonRpcError::new(-32000, "Method not supported"),
                    ))]);
                }
                Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::success(
                    request.id,
                    ListToolsResult {
                        tools: state.tools.clone(),
                        next_cursor: None,
                        meta: None,
                    },
                ))])
            }
            "tools/call" => mock_call_tool_response(request, &state),
            "resources/list" => Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::success(
                request.id,
                ListResourcesResult {
                    resources: state.resources.clone(),
                    next_cursor: None,
                    meta: None,
                },
            ))]),
            "resources/read" => mock_read_resource_response(request, &state),
            "resources/templates/list" => {
                Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::success(
                    request.id,
                    ListResourceTemplatesResult {
                        resource_templates: state.resource_templates.clone(),
                        meta: None,
                    },
                ))])
            }
            "prompts/list" => Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::success(
                request.id,
                ListPromptsResult {
                    prompts: state.prompts.clone(),
                    next_cursor: None,
                    meta: None,
                },
            ))]),
            "prompts/get" => mock_get_prompt_response(request, &state),
            _ => Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::error(
                request.id,
                JsonRpcError::new(-32601, format!("Unsupported method: {}", request.method)),
            ))]),
        }
    }

    fn close(&mut self) -> McpClientResult<()> {
        self.state
            .lock()
            .map_err(|_| McpClientError::new("Mock MCP transport mutex is poisoned"))?
            .closed = true;
        Ok(())
    }

    fn set_protocol_version(&mut self, protocol_version: String) {
        self.state
            .lock()
            .expect("mock transport state")
            .protocol_version = Some(protocol_version);
    }
}

fn default_tool_schema() -> JsonSchema {
    JsonObject::from_iter([
        ("type".to_string(), json!("object")),
        (
            "properties".to_string(),
            json!({
                "foo": { "type": "string" },
            }),
        ),
    ])
}

fn mock_initialize_result(state: &MockMcpTransportState) -> InitializeResult {
    let mut capabilities = ServerCapabilities::default();
    if !state.tools.is_empty() {
        capabilities.tools = Some(JsonObject::new());
    }
    if !state.resources.is_empty() {
        capabilities.resources = Some(JsonObject::new());
    }
    if !state.prompts.is_empty() {
        capabilities.prompts = Some(JsonObject::new());
    }

    InitializeResult {
        protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
        capabilities,
        server_info: Configuration::new("mock-mcp-server", "1.0.0"),
        instructions: None,
        meta: None,
    }
}

fn mock_call_tool_response(
    request: JsonRpcRequest,
    state: &MockMcpTransportState,
) -> McpClientResult<Vec<JsonRpcMessage>> {
    let tool_name = request
        .params
        .as_ref()
        .and_then(|params| params.get("name"))
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    if !state.tools.iter().any(|tool| tool.name == tool_name) {
        return Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::error(
            request.id,
            JsonRpcError::new(-32601, format!("Tool {tool_name} not found")).with_data(json!({
                "availableTools": state.tools.iter().map(|tool| tool.name.clone()).collect::<Vec<_>>(),
                "requestedTool": tool_name,
            })),
        ))]);
    }

    if state.fail_on_invalid_tool_params {
        return Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::error(
            request.id,
            JsonRpcError::new(
                -32602,
                format!(
                    "Invalid tool inputSchema: {}",
                    request
                        .params
                        .as_ref()
                        .and_then(|params| params.get("arguments"))
                        .map(JsonValue::to_string)
                        .unwrap_or_else(|| "null".to_string())
                ),
            ),
        ))]);
    }

    let result = state
        .tool_call_results
        .get(tool_name)
        .cloned()
        .unwrap_or_else(|| CallToolResult {
            content: Some(vec![json!({
                "type": "text",
                "text": "Mock tool call result",
            })]),
            is_error: Some(false),
            ..CallToolResult::default()
        });

    Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::success(
        request.id, result,
    ))])
}

fn mock_read_resource_response(
    request: JsonRpcRequest,
    state: &MockMcpTransportState,
) -> McpClientResult<Vec<JsonRpcMessage>> {
    let uri = request
        .params
        .as_ref()
        .and_then(|params| params.get("uri"))
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    let contents = state
        .resource_contents
        .iter()
        .filter(|content| content.uri() == uri)
        .cloned()
        .collect::<Vec<_>>();

    if contents.is_empty() {
        return Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::error(
            request.id,
            JsonRpcError::new(-32002, format!("Resource {uri} not found")),
        ))]);
    }

    Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::success(
        request.id,
        ReadResourceResult {
            contents,
            meta: None,
        },
    ))])
}

fn mock_get_prompt_response(
    request: JsonRpcRequest,
    state: &MockMcpTransportState,
) -> McpClientResult<Vec<JsonRpcMessage>> {
    let name = request
        .params
        .as_ref()
        .and_then(|params| params.get("name"))
        .and_then(JsonValue::as_str)
        .unwrap_or_default();
    let Some(result) = state.prompt_results.get(name) else {
        return Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::error(
            request.id,
            JsonRpcError::new(-32602, format!("Invalid params: Unknown prompt {name}")),
        ))]);
    };

    Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::success(
        request.id,
        result.clone(),
    ))])
}

/// Reads and validates MCP Apps metadata from a tool definition.
pub fn get_mcp_app_tool_meta(tool: &McpTool) -> Result<Option<McpAppToolMeta>, McpAppError> {
    let ui_meta = tool
        .meta
        .as_ref()
        .and_then(|meta| meta.get("ui"))
        .and_then(JsonValue::as_object)
        .cloned();
    let resource_uri_value = ui_meta
        .as_ref()
        .and_then(|meta| meta.get("resourceUri"))
        .or_else(|| {
            tool.meta
                .as_ref()
                .and_then(|meta| meta.get(MCP_APP_LEGACY_RESOURCE_URI_META_KEY))
        });
    let resource_uri = match resource_uri_value {
        Some(JsonValue::String(uri)) if uri.starts_with("ui://") => Some(uri.clone()),
        Some(value) => return Err(McpAppError::InvalidResourceUri(value.to_string())),
        None => None,
    };

    if resource_uri.is_none() && ui_meta.is_none() {
        return Ok(None);
    }

    let visibility = ui_meta
        .as_ref()
        .and_then(|meta| meta.get("visibility"))
        .and_then(JsonValue::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| match value.as_str() {
                    Some("model") => Some(McpAppToolVisibility::Model),
                    Some("app") => Some(McpAppToolVisibility::App),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty());

    let mut extra = ui_meta.unwrap_or_default();
    extra.remove("resourceUri");
    extra.remove("visibility");

    Ok(Some(McpAppToolMeta {
        resource_uri,
        visibility,
        extra,
    }))
}

/// Returns the `ui://` app resource URI attached to a tool, if present.
pub fn get_mcp_app_resource_uri(tool: &McpTool) -> Result<Option<String>, McpAppError> {
    Ok(get_mcp_app_tool_meta(tool)?.and_then(|meta| meta.resource_uri))
}

/// Checks whether a tool has an MCP App resource attached.
pub fn is_mcp_app_tool(tool: &McpTool) -> Result<bool, McpAppError> {
    Ok(get_mcp_app_resource_uri(tool)?.is_some())
}

/// Splits tool definitions into model-visible tools and app-visible tools.
pub fn split_mcp_app_tools(definitions: &ListToolsResult) -> Result<SplitMcpAppTools, McpAppError> {
    let mut model_visible_tools = Vec::new();
    let mut app_visible_tools = Vec::new();

    for tool in &definitions.tools {
        let visibility = get_mcp_app_tool_meta(tool)?.and_then(|meta| meta.visibility);

        if visibility.is_none()
            || visibility
                .as_ref()
                .is_some_and(|values| values.contains(&McpAppToolVisibility::Model))
        {
            model_visible_tools.push(tool.clone());
        }

        if visibility
            .as_ref()
            .is_some_and(|values| values.contains(&McpAppToolVisibility::App))
        {
            app_visible_tools.push(tool.clone());
        }
    }

    let mut model_visible = definitions.clone();
    model_visible.tools = model_visible_tools;
    let mut app_visible = definitions.clone();
    app_visible.tools = app_visible_tools;

    Ok(SplitMcpAppTools {
        model_visible,
        app_visible,
    })
}

/// Returns the unique MCP App resource URIs referenced by tool definitions.
pub fn get_mcp_app_resource_uris(
    definitions: &ListToolsResult,
) -> Result<Vec<String>, McpAppError> {
    let mut seen = BTreeSet::new();
    let mut uris = Vec::new();

    for tool in &definitions.tools {
        if let Some(uri) = get_mcp_app_resource_uri(tool)? {
            if seen.insert(uri.clone()) {
                uris.push(uri);
            }
        }
    }

    Ok(uris)
}

/// Extracts app HTML and rendering metadata from a `resources/read` result.
pub fn get_mcp_app_resource_from_read_result(
    uri: &str,
    resource: &ReadResourceResult,
) -> Result<McpAppResource, McpAppError> {
    let content = resource
        .contents
        .iter()
        .find(|content| content.uri() == uri)
        .ok_or_else(|| McpAppError::ResourceNotFound(uri.to_string()))?;

    if content.mime_type() != Some(MCP_APP_MIME_TYPE) {
        return Err(McpAppError::UnsupportedResourceMimeType {
            uri: uri.to_string(),
            mime_type: content.mime_type().map(str::to_string),
        });
    }

    let html = match content {
        ResourceContent::Text(content) => content.text.clone(),
        ResourceContent::Blob(content) => {
            let bytes = convert_base64_to_bytes(&content.blob).map_err(|source| {
                McpAppError::InvalidResourceBlob {
                    uri: uri.to_string(),
                    source,
                }
            })?;
            String::from_utf8(bytes).map_err(|source| McpAppError::InvalidResourceUtf8 {
                uri: uri.to_string(),
                source,
            })?
        }
    };
    let meta = content
        .meta()
        .and_then(|meta| meta.get("ui"))
        .and_then(JsonValue::as_object)
        .cloned();

    Ok(McpAppResource {
        uri: uri.to_string(),
        mime_type: MCP_APP_MIME_TYPE.to_string(),
        html,
        meta,
    })
}

/// Reads a `ui://` resource from an MCP server-like callback and normalizes it.
pub fn read_mcp_app_resource(
    uri: &str,
    read_resource: impl FnOnce(&str) -> ReadResourceResult,
) -> Result<McpAppResource, McpAppError> {
    if !uri.starts_with("ui://") {
        return Err(McpAppError::UnsupportedResourceUri(uri.to_string()));
    }

    get_mcp_app_resource_from_read_result(uri, &read_resource(uri))
}

/// Converts an MCP tool result into model-facing AI SDK tool output.
pub fn mcp_to_model_output(result: &CallToolResult) -> LanguageModelToolResultOutput {
    let Some(content) = &result.content else {
        return LanguageModelToolResultOutput::json(
            serde_json::to_value(result).expect("MCP tool result serializes"),
        );
    };

    LanguageModelToolResultOutput::content(
        content
            .iter()
            .map(|part| match part.get("type").and_then(JsonValue::as_str) {
                Some("text") => part
                    .get("text")
                    .and_then(JsonValue::as_str)
                    .map(|text| {
                        LanguageModelToolResultContentPart::Text(LanguageModelTextPart::new(text))
                    })
                    .unwrap_or_else(|| unknown_mcp_content_part(part)),
                Some("image") => {
                    let data = part.get("data").and_then(JsonValue::as_str);
                    let mime_type = part.get("mimeType").and_then(JsonValue::as_str);
                    match (data, mime_type) {
                        (Some(data), Some(mime_type)) => {
                            LanguageModelToolResultContentPart::File(LanguageModelFilePart::new(
                                FileData::Data {
                                    data: FileDataContent::Base64(data.to_string()),
                                },
                                mime_type,
                            ))
                        }
                        _ => unknown_mcp_content_part(part),
                    }
                }
                _ => unknown_mcp_content_part(part),
            })
            .collect(),
    )
}

fn unknown_mcp_content_part(part: &JsonValue) -> LanguageModelToolResultContentPart {
    LanguageModelToolResultContentPart::Text(LanguageModelTextPart::new(
        serde_json::to_string(part).expect("MCP content part serializes"),
    ))
}

/// Builds provider metadata attached to AI SDK tools created from MCP tools.
pub fn mcp_provider_metadata(
    client_name: impl Into<String>,
    tool: &McpTool,
) -> Result<JsonObject, McpAppError> {
    let mut metadata = JsonObject::from_iter([
        (
            "clientName".to_string(),
            JsonValue::String(client_name.into()),
        ),
        ("toolName".to_string(), JsonValue::String(tool.name.clone())),
    ]);
    let title = tool.title.clone().or_else(|| {
        tool.annotations
            .as_ref()
            .and_then(|annotations| annotations.title.clone())
    });
    if let Some(title) = title {
        metadata.insert("title".to_string(), JsonValue::String(title));
    }
    if let Some(app_meta) = get_mcp_app_tool_meta(tool)? {
        if app_meta.resource_uri.is_some() {
            let mut app = app_meta.extra;
            if let Some(resource_uri) = app_meta.resource_uri {
                app.insert("resourceUri".to_string(), JsonValue::String(resource_uri));
            }
            if let Some(visibility) = app_meta.visibility {
                app.insert(
                    "visibility".to_string(),
                    JsonValue::Array(
                        visibility
                            .iter()
                            .map(|value| match value {
                                McpAppToolVisibility::Model => JsonValue::String("model".into()),
                                McpAppToolVisibility::App => JsonValue::String("app".into()),
                            })
                            .collect(),
                    ),
                );
            }
            app.insert(
                "mimeType".to_string(),
                JsonValue::String(MCP_APP_MIME_TYPE.to_string()),
            );
            metadata.insert("app".to_string(), JsonValue::Object(app));
        }
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::future::Future;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::{Context, Poll, Wake, Waker};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    use ai_sdk_provider_utils::{Schema, ToolExecutionOptions, ValidationResult};

    fn object_schema() -> JsonSchema {
        JsonObject::from_iter([
            ("type".to_string(), json!("object")),
            (
                "properties".to_string(),
                json!({
                    "foo": { "type": "string" },
                }),
            ),
        ])
    }

    fn weather_output_schema() -> Schema {
        Schema::new(
            json!({
                "type": "object",
                "properties": {
                    "temperature": { "type": "number" },
                    "conditions": { "type": "string" }
                },
                "required": ["temperature", "conditions"]
            })
            .as_object()
            .expect("weather output schema is an object")
            .clone(),
        )
        .with_validator(|value| {
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

    fn app_tool(name: &str, visibility: Vec<&str>) -> McpTool {
        let mut ui = JsonObject::from_iter([
            (
                "resourceUri".to_string(),
                json!("ui://ai-sdk-e2e/dashboard"),
            ),
            ("theme".to_string(), json!("dark")),
        ]);
        ui.insert("visibility".to_string(), json!(visibility));
        let mut meta = JsonObject::new();
        meta.insert("ui".to_string(), JsonValue::Object(ui));

        McpTool {
            name: name.to_string(),
            title: Some("Dashboard".to_string()),
            description: Some("Show dashboard".to_string()),
            input_schema: object_schema(),
            output_schema: None,
            annotations: None,
            meta: Some(meta),
            extra: JsonObject::new(),
        }
    }

    #[test]
    fn protocol_constants_match_upstream_mcp_package() {
        assert_eq!(LATEST_PROTOCOL_VERSION, "2025-11-25");
        assert_eq!(
            SUPPORTED_PROTOCOL_VERSIONS,
            ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"]
        );
        assert_eq!(MCP_APP_MIME_TYPE, "text/html;profile=mcp-app");
    }

    #[test]
    fn json_rpc_message_shapes_match_mcp_transport_boundary() {
        let request =
            JsonRpcRequest::new(json!(7), "tools/list").with_params(json!({ "cursor": "next" }));
        assert_eq!(
            serde_json::to_value(&request).expect("request serializes"),
            json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "tools/list",
                "params": { "cursor": "next" },
            })
        );

        let response = serde_json::from_value::<JsonRpcMessage>(json!({
            "jsonrpc": "2.0",
            "id": 7,
            "result": { "tools": [] },
        }))
        .expect("response deserializes");
        assert!(matches!(response, JsonRpcMessage::Response(_)));
    }

    #[test]
    fn list_tools_result_is_serializable_cache_data() {
        let definitions = ListToolsResult {
            tools: vec![McpTool::new("mock-tool", object_schema())],
            next_cursor: Some("next-page".to_string()),
            meta: None,
        };
        let cached = serde_json::to_string(&definitions).expect("definitions serialize");
        let restored = serde_json::from_str::<ListToolsResult>(&cached).expect("definitions parse");

        assert_eq!(restored.tools[0].name, "mock-tool");
        assert_eq!(restored.next_cursor.as_deref(), Some("next-page"));
        assert_eq!(
            restored.tools[0].input_schema["properties"]["foo"]["type"],
            "string"
        );
    }

    #[test]
    fn mcp_http_transport_posts_json_and_cleans_up_session() {
        let server = LocalHttpServer::new(vec![
            LocalHttpResponse::json(json!({
                "jsonrpc": "2.0",
                "id": 0,
                "result": {
                    "protocolVersion": LATEST_PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "http-mcp-server", "version": "1.0.0" }
                }
            }))
            .with_header("mcp-session-id", "session-123"),
            LocalHttpResponse::empty(202),
            LocalHttpResponse::empty(200),
        ]);

        let client = create_mcp_client(McpClientConfig::new(
            McpHttpTransport::new(server.url()).with_header("x-custom-header", "test-value"),
        ))
        .expect("client initializes over HTTP");
        assert_eq!(
            client.server_info().expect("server info").name,
            "http-mcp-server"
        );
        client.close().expect("client closes");

        let requests = server.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(
            requests[0].headers.get("mcp-protocol-version"),
            Some(&LATEST_PROTOCOL_VERSION.to_string())
        );
        assert_eq!(
            requests[0].headers.get("accept"),
            Some(&"application/json, text/event-stream".to_string())
        );
        assert_eq!(
            requests[0].headers.get("x-custom-header"),
            Some(&"test-value".to_string())
        );
        assert_eq!(requests[0].body["method"], "initialize");
        assert_eq!(
            requests[1].body["method"], "notifications/initialized",
            "client sends initialized notification"
        );
        assert_eq!(requests[2].method, "DELETE");
        assert_eq!(
            requests[2].headers.get("mcp-session-id"),
            Some(&"session-123".to_string())
        );
    }

    #[test]
    fn mcp_http_transport_parses_sse_message_responses() {
        let server = LocalHttpServer::new(vec![LocalHttpResponse::new(
            200,
            [("content-type", "text/event-stream")],
            format!(
                "event: message\ndata: {}\n\n",
                json!({
                    "jsonrpc": "2.0",
                    "id": 7,
                    "result": { "ok": true }
                })
            ),
        )]);
        let mut transport = McpHttpTransport::new(server.url());
        transport.start().expect("transport starts");

        let messages = transport
            .send(JsonRpcMessage::Request(JsonRpcRequest::new(
                json!(7),
                "tools/list",
            )))
            .expect("SSE response parses");

        assert_eq!(
            serde_json::to_value(&messages).expect("messages serialize"),
            json!([{ "jsonrpc": "2.0", "id": 7, "result": { "ok": true } }])
        );
    }

    #[test]
    fn mcp_http_transport_reports_http_errors_with_sse_hint() {
        let server = LocalHttpServer::new(vec![LocalHttpResponse::new(
            404,
            [("content-type", "text/plain")],
            "missing",
        )]);
        let mut transport = McpHttpTransport::new(server.url());
        transport.start().expect("transport starts");

        let error = transport
            .send(JsonRpcMessage::Request(JsonRpcRequest::new(
                json!(1),
                "initialize",
            )))
            .expect_err("HTTP error is reported");

        assert!(error.message.contains("POSTing to endpoint (HTTP 404)"));
        assert!(error.message.contains("Try using `sse` transport instead"));
    }

    #[test]
    fn stdio_environment_copies_custom_env_and_inherits_safe_defaults() {
        let custom_env = BTreeMap::from([
            ("CUSTOM_VAR".to_string(), "custom_value".to_string()),
            ("PATH".to_string(), "custom_path".to_string()),
        ]);

        let result = get_stdio_environment_from(
            Some(custom_env.clone()),
            [
                ("HOME", "/home/test"),
                ("PATH", "/usr/bin"),
                ("SHELL", "() { ignored; }"),
                ("UNRELATED", "ignored"),
            ],
        );

        assert_eq!(
            custom_env.get("CUSTOM_VAR").map(String::as_str),
            Some("custom_value")
        );
        assert_eq!(
            result.get("CUSTOM_VAR").map(String::as_str),
            Some("custom_value")
        );
        assert_eq!(result.get("HOME").map(String::as_str), Some("/home/test"));
        assert_eq!(
            result.get("PATH").map(String::as_str),
            Some("/usr/bin"),
            "safe inherited defaults overlay custom env values"
        );
        assert_eq!(
            result.get("SHELL"),
            None,
            "function-like shell env values are skipped"
        );
        assert_eq!(result.get("UNRELATED"), None);
    }

    #[test]
    fn stdio_message_framing_serializes_deserializes_and_buffers_lines() {
        let message =
            JsonRpcRequest::new(json!(7), "tools/list").with_params(json!({ "cursor": "next" }));
        let framed = serialize_stdio_message(&JsonRpcMessage::Request(message.clone()));

        assert!(framed.ends_with('\n'));
        assert_eq!(
            deserialize_stdio_message(framed.trim_end()).expect("message parses"),
            JsonRpcMessage::Request(message)
        );

        let mut buffer = StdioReadBuffer::default();
        buffer.append(&framed.as_bytes()[..8]);
        assert_eq!(buffer.read_line(), None);
        buffer.append(&framed.as_bytes()[8..]);
        assert_eq!(buffer.read_line(), Some(framed.trim_end().to_string()));
        assert_eq!(buffer.read_line(), None);
    }

    #[test]
    fn stdio_transport_errors_when_not_connected() {
        let mut transport = StdioMcpTransport::new(StdioConfig::new("unused"));

        assert_eq!(
            transport
                .send(JsonRpcMessage::Request(JsonRpcRequest::new(
                    json!(0),
                    "initialize"
                )))
                .expect_err("not connected")
                .message,
            "StdioClientTransport not connected"
        );
    }

    #[cfg(unix)]
    #[test]
    fn stdio_transport_writes_message_and_reads_response_line() {
        let script = r#"while IFS= read -r line; do printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"ok":true}}'; done"#;
        let mut transport =
            StdioMcpTransport::new(StdioConfig::new("sh").with_arg("-c").with_arg(script));

        transport.start().expect("stdio transport starts");
        let messages = transport
            .send(JsonRpcMessage::Request(JsonRpcRequest::new(
                json!(0),
                "initialize",
            )))
            .expect("stdio response reads");
        transport.close().expect("stdio transport closes");

        assert_eq!(
            serde_json::to_value(messages).expect("messages serialize"),
            json!([{ "jsonrpc": "2.0", "id": 0, "result": { "ok": true } }])
        );
    }

    #[test]
    fn mcp_client_initializes_and_sends_initialized_notification() {
        let transport = MockMcpTransport::new();
        let client = create_mcp_client(
            McpClientConfig::new(transport.clone())
                .with_client_name("MyMCPClient")
                .with_version("2.0.0"),
        )
        .expect("client initializes");

        assert_eq!(
            client.server_info().expect("server info").name,
            "mock-mcp-server"
        );
        assert_eq!(
            transport.negotiated_protocol_version().as_deref(),
            Some(LATEST_PROTOCOL_VERSION)
        );

        let sent_messages = transport.sent_messages();
        assert_eq!(sent_messages.len(), 2);
        match &sent_messages[0] {
            JsonRpcMessage::Request(request) => {
                assert_eq!(request.method, "initialize");
                assert_eq!(
                    request
                        .params
                        .as_ref()
                        .and_then(|params| params.get("clientInfo"))
                        .and_then(|client_info| client_info.get("name")),
                    Some(&json!("MyMCPClient"))
                );
                assert_eq!(
                    request
                        .params
                        .as_ref()
                        .and_then(|params| params.get("clientInfo"))
                        .and_then(|client_info| client_info.get("version")),
                    Some(&json!("2.0.0"))
                );
            }
            _ => panic!("expected initialize request"),
        }
        assert!(matches!(
            &sent_messages[1],
            JsonRpcMessage::Notification(notification)
                if notification.method == "notifications/initialized"
        ));

        client.close().expect("client closes");
        assert!(transport.is_closed());
    }

    #[test]
    fn mcp_client_lists_calls_reads_resources_and_prompts() {
        let client = create_mcp_client(McpClientConfig::new(MockMcpTransport::new()))
            .expect("client initializes");

        let definitions = client.list_tools(None).expect("tools list");
        assert_eq!(
            definitions
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["mock-tool", "mock-tool-no-args"]
        );
        assert_eq!(
            client
                .call_tool(
                    McpCallToolRequest::new("mock-tool").with_arguments(json!({ "foo": "bar" }))
                )
                .expect("tool calls")
                .content,
            Some(vec![json!({
                "type": "text",
                "text": "Mock tool call result",
            })])
        );

        assert_eq!(
            client
                .list_resources(None)
                .expect("resources list")
                .resources[0]
                .uri,
            "file:///mock/resource.txt"
        );
        let resource = client
            .read_resource("file:///mock/resource.txt")
            .expect("resource reads");
        assert!(matches!(
            &resource.contents[0],
            ResourceContent::Text(content) if content.text == "Mock resource content"
        ));
        assert_eq!(
            client
                .list_resource_templates()
                .expect("resource templates list")
                .resource_templates[0]
                .uri_template,
            "file:///{path}"
        );
        assert_eq!(
            client.list_prompts(None).expect("prompts list").prompts[0].name,
            "code_review"
        );
        assert_eq!(
            client
                .get_prompt(McpGetPromptRequest::new("code_review"))
                .expect("prompt gets")
                .messages[0]
                .role,
            "user"
        );
    }

    #[test]
    fn mcp_client_reports_capability_protocol_and_json_rpc_errors() {
        let no_tools = MockMcpTransport::new().with_tools([]);
        let client = create_mcp_client(McpClientConfig::new(no_tools)).expect("client initializes");
        assert_eq!(
            client
                .list_tools(None)
                .expect_err("tools capability is missing")
                .message,
            "Server does not support tools"
        );

        let client = create_mcp_client(McpClientConfig::new(MockMcpTransport::new()))
            .expect("client initializes");
        let error = client
            .call_tool(McpCallToolRequest::new("missing"))
            .expect_err("missing tool fails");
        assert_eq!(error.code, Some(-32601));
        assert_eq!(
            error
                .data
                .as_ref()
                .and_then(|data| data.get("requestedTool")),
            Some(&json!("missing"))
        );

        let unsupported_protocol = InitializeResult {
            protocol_version: "1900-01-01".to_string(),
            capabilities: ServerCapabilities::default(),
            server_info: Configuration::new("old-server", "0.1.0"),
            instructions: None,
            meta: None,
        };
        let error = match create_mcp_client(McpClientConfig::new(
            MockMcpTransport::new().with_initialize_result(unsupported_protocol),
        )) {
            Ok(_) => panic!("unsupported protocol should fail"),
            Err(error) => error,
        };
        assert_eq!(
            error.message,
            "Server's protocol version is not supported: 1900-01-01"
        );
    }

    #[test]
    fn mcp_client_handles_elicitation_request_messages() {
        let transport = ElicitationScenarioTransport::new(
            JsonRpcRequest::new(json!(42), "elicitation/create").with_params(json!({
                "message": "Please provide your GitHub username",
                "requestedSchema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    },
                    "required": ["name"]
                }
            })),
        );
        let saw_request = Arc::new(Mutex::new(false));
        let saw_request_clone = Arc::clone(&saw_request);
        let client = create_mcp_client(McpClientConfig::new(transport.clone()).with_capabilities(
            ClientCapabilities {
                elicitation: Some(ElicitationCapability::default()),
                ..ClientCapabilities::default()
            },
        ))
        .expect("client initializes");

        client
            .on_elicitation_request(move |request| {
                assert_eq!(request.method, "elicitation/create");
                assert_eq!(
                    request.params.message,
                    "Please provide your GitHub username"
                );
                assert_eq!(
                    request
                        .params
                        .requested_schema
                        .get("properties")
                        .and_then(JsonValue::as_object)
                        .and_then(|properties| properties.get("name"))
                        .and_then(|name| name.get("type")),
                    Some(&json!("string"))
                );
                *saw_request_clone.lock().expect("saw request lock") = true;
                Ok(ElicitResult {
                    action: ElicitAction::Accept,
                    content: Some(JsonObject::from_iter([(
                        "name".to_string(),
                        json!("octocat"),
                    )])),
                    meta: None,
                })
            })
            .expect("handler registers");

        let tools = client.list_tools(None).expect("tools list completes");
        assert!(tools.tools.is_empty());
        assert!(*saw_request.lock().expect("saw request lock"));

        let sent_messages = transport.sent_messages();
        assert!(sent_messages.iter().any(|message| {
            matches!(
                message,
                JsonRpcMessage::Request(request)
                    if request.method == "initialize"
                        && request.params.as_ref()
                            .and_then(|params| params.get("capabilities"))
                            .and_then(|capabilities| capabilities.get("elicitation"))
                            .is_some()
            )
        }));
        assert!(sent_messages.iter().any(|message| {
            matches!(
                message,
                JsonRpcMessage::Response(response)
                    if response.id == json!(42)
                        && response.result.as_ref()
                            .and_then(|result| result.get("action"))
                            == Some(&json!("accept"))
                        && response.result.as_ref()
                            .and_then(|result| result.get("content"))
                            .and_then(|content| content.get("name"))
                            == Some(&json!("octocat"))
            )
        }));
    }

    #[test]
    fn mcp_client_reports_elicitation_request_errors_to_server() {
        let no_handler_transport = ElicitationScenarioTransport::new(
            JsonRpcRequest::new(json!("no-handler"), "elicitation/create").with_params(json!({
                "message": "Name?",
                "requestedSchema": { "type": "object" }
            })),
        );
        let client = create_mcp_client(McpClientConfig::new(no_handler_transport.clone()))
            .expect("client initializes");
        client.list_tools(None).expect("tools list completes");
        assert!(no_handler_transport.sent_messages().iter().any(|message| {
            matches!(
                message,
                JsonRpcMessage::Response(response)
                    if response.id == json!("no-handler")
                        && response.error.as_ref().map(|error| error.code) == Some(-32601)
                        && response.error.as_ref().map(|error| error.message.as_str())
                            == Some("No elicitation handler registered on client")
            )
        }));

        let invalid_transport = ElicitationScenarioTransport::new(
            JsonRpcRequest::new(json!("invalid"), "elicitation/create")
                .with_params(json!({ "requestedSchema": { "type": "object" } })),
        );
        let client = create_mcp_client(McpClientConfig::new(invalid_transport.clone()))
            .expect("client initializes");
        client
            .on_elicitation_request(|_| {
                Ok(ElicitResult {
                    action: ElicitAction::Accept,
                    content: None,
                    meta: None,
                })
            })
            .expect("handler registers");
        client.list_tools(None).expect("tools list completes");
        assert!(invalid_transport.sent_messages().iter().any(|message| {
            matches!(
                message,
                JsonRpcMessage::Response(response)
                    if response.id == json!("invalid")
                        && response.error.as_ref().map(|error| error.code) == Some(-32602)
                        && response.error.as_ref()
                            .map(|error| error.message.starts_with("Invalid elicitation request:"))
                            == Some(true)
            )
        }));

        let failing_transport = ElicitationScenarioTransport::new(
            JsonRpcRequest::new(json!("failing"), "elicitation/create").with_params(json!({
                "message": "Name?",
                "requestedSchema": { "type": "object" }
            })),
        );
        let client = create_mcp_client(McpClientConfig::new(failing_transport.clone()))
            .expect("client initializes");
        client
            .on_elicitation_request(|_| Err(McpClientError::new("user declined in host UI")))
            .expect("handler registers");
        client.list_tools(None).expect("tools list completes");
        assert!(failing_transport.sent_messages().iter().any(|message| {
            matches!(
                message,
                JsonRpcMessage::Response(response)
                    if response.id == json!("failing")
                        && response.error.as_ref().map(|error| error.code) == Some(-32603)
                        && response.error.as_ref().map(|error| error.message.as_str())
                            == Some("user declined in host UI")
            )
        }));
    }

    #[test]
    fn mcp_client_builds_executable_dynamic_tools_from_definitions() {
        let tool = app_tool("showDashboard", vec!["model", "app"]);
        let result = CallToolResult {
            content: Some(vec![json!({ "type": "text", "text": "Dashboard ready" })]),
            is_error: Some(false),
            ..CallToolResult::default()
        };
        let transport = MockMcpTransport::new()
            .with_tools([tool])
            .with_tool_call_result("showDashboard", result.clone());
        let client =
            create_mcp_client(McpClientConfig::new(transport).with_client_name("MyMCPClient"))
                .expect("client initializes");

        let tools = client.tools().expect("tools build");
        let dynamic_tool = tools.get("showDashboard").expect("tool exists");
        assert!(dynamic_tool.is_dynamic());
        assert!(dynamic_tool.is_executable());
        assert!(dynamic_tool.has_to_model_output());
        assert_eq!(dynamic_tool.title(), Some("Dashboard"));
        assert_eq!(dynamic_tool.description.as_deref(), Some("Show dashboard"));
        assert_eq!(
            dynamic_tool.input_schema.get("additionalProperties"),
            Some(&json!(false))
        );
        assert_eq!(
            dynamic_tool
                .metadata()
                .and_then(|metadata| metadata.get("clientName")),
            Some(&json!("MyMCPClient"))
        );
        assert_eq!(
            dynamic_tool
                .metadata()
                .and_then(|metadata| metadata.get("app"))
                .and_then(JsonValue::as_object)
                .and_then(|app| app.get("mimeType")),
            Some(&json!(MCP_APP_MIME_TYPE))
        );

        let output = block_on(
            dynamic_tool
                .execute(
                    json!({ "topic": "latency" }),
                    ToolExecutionOptions::new("tool-call-1", Vec::new()),
                )
                .expect("tool is executable"),
        )
        .expect("tool execution succeeds");
        assert_eq!(
            output,
            serde_json::to_value(result).expect("result serializes")
        );

        let model_output = block_on(
            dynamic_tool
                .model_output(ToolModelOutputOptions::new(
                    "tool-call-1",
                    json!({ "topic": "latency" }),
                    output,
                ))
                .expect("model output converter exists"),
        );
        assert_eq!(
            serde_json::to_value(model_output).expect("model output serializes"),
            json!({
                "type": "content",
                "value": [{ "type": "text", "text": "Dashboard ready" }]
            })
        );
    }

    #[test]
    fn mcp_client_builds_schema_typed_tools_from_structured_content() {
        let schema = weather_output_schema();
        let output_json_schema = schema.json_schema().clone();
        let result = CallToolResult {
            content: Some(vec![json!({
                "type": "text",
                "text": "{\"temperature\": 22.5, \"conditions\": \"Sunny\"}"
            })]),
            structured_content: Some(json!({
                "temperature": 22.5,
                "conditions": "Sunny"
            })),
            ..CallToolResult::default()
        };
        let transport = MockMcpTransport::new()
            .with_tools([
                McpTool::new("weather-tool", object_schema()),
                McpTool::new("ignored-tool", object_schema()),
            ])
            .with_tool_call_result("weather-tool", result);
        let client =
            create_mcp_client(McpClientConfig::new(transport)).expect("client initializes");
        let schemas = McpToolSchemas::from([(
            "weather-tool".to_string(),
            McpToolSchema::new()
                .with_input_schema(object_schema())
                .with_output_schema(schema),
        )]);

        let tools = client
            .tools_with_schemas(&schemas)
            .expect("schema tools build");

        assert_eq!(
            tools.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["weather-tool"]
        );
        let tool = tools.get("weather-tool").expect("tool exists");
        assert_eq!(tool.output_schema(), Some(&output_json_schema));

        let output = block_on(
            tool.execute(
                json!({}),
                ToolExecutionOptions::new("tool-call-1", Vec::new()),
            )
            .expect("tool is executable"),
        )
        .expect("tool execution succeeds");
        assert_eq!(
            output,
            json!({
                "temperature": 22.5,
                "conditions": "Sunny"
            })
        );
    }

    #[test]
    fn mcp_client_schema_typed_tools_parse_text_content_fallback() {
        let transport = MockMcpTransport::new()
            .with_tools([McpTool::new("json-tool", object_schema())])
            .with_tool_call_result(
                "json-tool",
                CallToolResult {
                    content: Some(vec![json!({
                        "type": "text",
                        "text": "{\"temperature\": 18.0, \"conditions\": \"Cloudy\"}"
                    })]),
                    ..CallToolResult::default()
                },
            );
        let client =
            create_mcp_client(McpClientConfig::new(transport)).expect("client initializes");
        let schemas = McpToolSchemas::from([(
            "json-tool".to_string(),
            McpToolSchema::new().with_output_schema(weather_output_schema()),
        )]);
        let tools = client
            .tools_with_schemas(&schemas)
            .expect("schema tools build");

        let output = block_on(
            tools["json-tool"]
                .execute(
                    json!({}),
                    ToolExecutionOptions::new("tool-call-1", Vec::new()),
                )
                .expect("tool is executable"),
        )
        .expect("tool execution succeeds");

        assert_eq!(
            output,
            json!({
                "temperature": 18.0,
                "conditions": "Cloudy"
            })
        );
    }

    #[test]
    fn mcp_client_schema_typed_tools_report_output_validation_errors() {
        let transport = MockMcpTransport::new()
            .with_tools([
                McpTool::new("bad-structured-tool", object_schema()),
                McpTool::new("bad-text-tool", object_schema()),
                McpTool::new("empty-tool", object_schema()),
                McpTool::new("error-tool", object_schema()),
            ])
            .with_tool_call_result(
                "bad-structured-tool",
                CallToolResult {
                    structured_content: Some(json!({ "wrong": "data" })),
                    ..CallToolResult::default()
                },
            )
            .with_tool_call_result(
                "bad-text-tool",
                CallToolResult {
                    content: Some(vec![json!({ "type": "text", "text": "not json" })]),
                    ..CallToolResult::default()
                },
            )
            .with_tool_call_result("empty-tool", CallToolResult::default())
            .with_tool_call_result(
                "error-tool",
                CallToolResult {
                    content: Some(vec![json!({ "type": "text", "text": "not json" })]),
                    is_error: Some(true),
                    ..CallToolResult::default()
                },
            );
        let client =
            create_mcp_client(McpClientConfig::new(transport)).expect("client initializes");
        let schemas = McpToolSchemas::from([
            (
                "bad-structured-tool".to_string(),
                McpToolSchema::new().with_output_schema(weather_output_schema()),
            ),
            (
                "bad-text-tool".to_string(),
                McpToolSchema::new().with_output_schema(weather_output_schema()),
            ),
            (
                "empty-tool".to_string(),
                McpToolSchema::new().with_output_schema(weather_output_schema()),
            ),
            (
                "error-tool".to_string(),
                McpToolSchema::new().with_output_schema(weather_output_schema()),
            ),
        ]);
        let tools = client
            .tools_with_schemas(&schemas)
            .expect("schema tools build");

        let structured_error = block_on(
            tools["bad-structured-tool"]
                .execute(
                    json!({}),
                    ToolExecutionOptions::new("tool-call-1", Vec::new()),
                )
                .expect("tool is executable"),
        )
        .expect_err("structured content validation fails");
        assert_eq!(
            structured_error.message(),
            "Tool \"bad-structured-tool\" returned structuredContent that does not match the expected outputSchema"
        );

        let text_error = block_on(
            tools["bad-text-tool"]
                .execute(
                    json!({}),
                    ToolExecutionOptions::new("tool-call-2", Vec::new()),
                )
                .expect("tool is executable"),
        )
        .expect_err("text content validation fails");
        assert_eq!(
            text_error.message(),
            "Tool \"bad-text-tool\" returned content that does not match the expected outputSchema"
        );

        let empty_error = block_on(
            tools["empty-tool"]
                .execute(
                    json!({}),
                    ToolExecutionOptions::new("tool-call-3", Vec::new()),
                )
                .expect("tool is executable"),
        )
        .expect_err("missing structured content fails");
        assert_eq!(
            empty_error.message(),
            "Tool \"empty-tool\" did not return structuredContent or parseable text content"
        );

        let error_output = block_on(
            tools["error-tool"]
                .execute(
                    json!({}),
                    ToolExecutionOptions::new("tool-call-4", Vec::new()),
                )
                .expect("tool is executable"),
        )
        .expect("error tool result bypasses output validation");
        assert_eq!(
            error_output,
            json!({
                "content": [{ "type": "text", "text": "not json" }],
                "isError": true
            })
        );
    }

    #[test]
    fn mcp_to_model_output_converts_text_images_and_unknown_content() {
        let result = CallToolResult {
            content: Some(vec![
                json!({ "type": "text", "text": "Hello world" }),
                json!({ "type": "image", "data": "base64data", "mimeType": "image/png" }),
                json!({ "type": "custom", "data": { "foo": "bar" } }),
            ]),
            is_error: Some(false),
            ..CallToolResult::default()
        };

        assert_eq!(
            serde_json::to_value(mcp_to_model_output(&result)).expect("output serializes"),
            json!({
                "type": "content",
                "value": [
                    { "type": "text", "text": "Hello world" },
                    {
                        "type": "file",
                        "data": { "type": "data", "data": "base64data" },
                        "mediaType": "image/png"
                    },
                    { "type": "text", "text": "{\"data\":{\"foo\":\"bar\"},\"type\":\"custom\"}" }
                ]
            })
        );
    }

    #[test]
    fn mcp_to_model_output_falls_back_to_json_without_content_array() {
        let result = CallToolResult {
            tool_result: Some(json!({ "answer": 42 })),
            ..CallToolResult::default()
        };

        assert_eq!(
            serde_json::to_value(mcp_to_model_output(&result)).expect("output serializes"),
            json!({
                "type": "json",
                "value": { "toolResult": { "answer": 42 } }
            })
        );
    }

    #[test]
    fn mcp_app_client_capabilities_match_upstream_extension_shape() {
        assert_eq!(
            mcp_app_client_capabilities(),
            json!({
                "extensions": {
                    "io.modelcontextprotocol/ui": {
                        "mimeTypes": ["text/html;profile=mcp-app"],
                    },
                },
            })
        );
    }

    #[test]
    fn mcp_app_tool_meta_reads_ui_and_legacy_resource_uris() {
        let tool = app_tool("showDashboard", vec!["model", "app"]);
        let meta = get_mcp_app_tool_meta(&tool)
            .expect("metadata is valid")
            .expect("metadata is present");
        assert_eq!(
            meta.resource_uri.as_deref(),
            Some("ui://ai-sdk-e2e/dashboard")
        );
        assert_eq!(
            meta.visibility,
            Some(vec![McpAppToolVisibility::Model, McpAppToolVisibility::App])
        );
        assert_eq!(meta.extra.get("theme"), Some(&json!("dark")));

        let mut legacy_meta = JsonObject::new();
        legacy_meta.insert(
            MCP_APP_LEGACY_RESOURCE_URI_META_KEY.to_string(),
            json!("ui://legacy/app"),
        );
        let legacy_tool = McpTool {
            meta: Some(legacy_meta),
            ..McpTool::new("legacy", object_schema())
        };
        assert_eq!(
            get_mcp_app_resource_uri(&legacy_tool).expect("legacy URI is valid"),
            Some("ui://legacy/app".to_string())
        );
    }

    #[test]
    fn mcp_app_tool_meta_rejects_invalid_resource_uri() {
        let mut meta = JsonObject::new();
        meta.insert(
            MCP_APP_LEGACY_RESOURCE_URI_META_KEY.to_string(),
            json!("https://example.com/app"),
        );
        let tool = McpTool {
            meta: Some(meta),
            ..McpTool::new("bad", object_schema())
        };

        assert!(matches!(
            get_mcp_app_tool_meta(&tool),
            Err(McpAppError::InvalidResourceUri(_))
        ));
    }

    #[test]
    fn split_mcp_app_tools_respects_model_and_app_visibility() {
        let plain_tool = McpTool::new("plain", object_schema());
        let app_only = app_tool("appOnly", vec!["app"]);
        let both = app_tool("both", vec!["model", "app"]);
        let definitions = ListToolsResult {
            tools: vec![plain_tool, app_only, both],
            ..ListToolsResult::default()
        };

        let split = split_mcp_app_tools(&definitions).expect("tools split");
        assert_eq!(
            split
                .model_visible
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["plain", "both"]
        );
        assert_eq!(
            split
                .app_visible
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["appOnly", "both"]
        );
        assert_eq!(
            get_mcp_app_resource_uris(&definitions).expect("URIs collect"),
            vec!["ui://ai-sdk-e2e/dashboard"]
        );
    }

    #[test]
    fn mcp_app_resource_from_read_result_extracts_text_html_and_meta() {
        let resource = ReadResourceResult {
            contents: vec![ResourceContent::Text(TextResourceContent {
                uri: "ui://ai-sdk-e2e/dashboard".to_string(),
                name: None,
                title: None,
                mime_type: Some(MCP_APP_MIME_TYPE.to_string()),
                meta: Some(JsonObject::from_iter([(
                    "ui".to_string(),
                    json!({ "prefersBorder": true }),
                )])),
                text: "<h1>Dashboard</h1>".to_string(),
                extra: JsonObject::new(),
            })],
            meta: None,
        };

        let app = get_mcp_app_resource_from_read_result("ui://ai-sdk-e2e/dashboard", &resource)
            .expect("resource is extracted");
        assert_eq!(app.html, "<h1>Dashboard</h1>");
        assert_eq!(app.mime_type, MCP_APP_MIME_TYPE);
        assert_eq!(
            app.meta
                .expect("resource metadata is present")
                .get("prefersBorder"),
            Some(&json!(true))
        );
    }

    #[test]
    fn mcp_app_resource_from_read_result_decodes_blob_html() {
        let resource = ReadResourceResult {
            contents: vec![ResourceContent::Blob(BlobResourceContent {
                uri: "ui://ai-sdk-e2e/dashboard".to_string(),
                name: None,
                title: None,
                mime_type: Some(MCP_APP_MIME_TYPE.to_string()),
                meta: None,
                blob: "PGgxPkRhc2hib2FyZDwvaDE+".to_string(),
                extra: JsonObject::new(),
            })],
            meta: None,
        };

        assert_eq!(
            get_mcp_app_resource_from_read_result("ui://ai-sdk-e2e/dashboard", &resource)
                .expect("resource is extracted")
                .html,
            "<h1>Dashboard</h1>"
        );
    }

    #[test]
    fn read_mcp_app_resource_rejects_non_ui_uri() {
        assert!(matches!(
            read_mcp_app_resource("file:///tmp/app.html", |_| ReadResourceResult::default()),
            Err(McpAppError::UnsupportedResourceUri(_))
        ));
    }

    #[test]
    fn mcp_provider_metadata_includes_title_and_app_metadata() {
        let metadata = mcp_provider_metadata(
            "MyMCPClient",
            &app_tool("showDashboard", vec!["model", "app"]),
        )
        .expect("metadata builds");

        assert_eq!(metadata.get("clientName"), Some(&json!("MyMCPClient")));
        assert_eq!(metadata.get("toolName"), Some(&json!("showDashboard")));
        assert_eq!(metadata.get("title"), Some(&json!("Dashboard")));
        assert_eq!(
            metadata
                .get("app")
                .and_then(JsonValue::as_object)
                .and_then(|app| app.get("mimeType")),
            Some(&json!(MCP_APP_MIME_TYPE))
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
            Poll::Pending => panic!("test future unexpectedly pending"),
        }
    }

    #[derive(Clone)]
    struct ElicitationScenarioTransport {
        state: Arc<Mutex<ElicitationScenarioState>>,
    }

    #[derive(Debug)]
    struct ElicitationScenarioState {
        server_request: Option<JsonRpcRequest>,
        sent_messages: Vec<JsonRpcMessage>,
    }

    impl ElicitationScenarioTransport {
        fn new(server_request: JsonRpcRequest) -> Self {
            Self {
                state: Arc::new(Mutex::new(ElicitationScenarioState {
                    server_request: Some(server_request),
                    sent_messages: Vec::new(),
                })),
            }
        }

        fn sent_messages(&self) -> Vec<JsonRpcMessage> {
            self.state
                .lock()
                .expect("elicitation scenario state")
                .sent_messages
                .clone()
        }
    }

    impl McpTransport for ElicitationScenarioTransport {
        fn send(&mut self, message: JsonRpcMessage) -> McpClientResult<Vec<JsonRpcMessage>> {
            let mut state = self
                .state
                .lock()
                .map_err(|_| McpClientError::new("Elicitation scenario transport is poisoned"))?;
            state.sent_messages.push(message.clone());

            let JsonRpcMessage::Request(request) = message else {
                return Ok(Vec::new());
            };

            match request.method.as_str() {
                "initialize" => Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::success(
                    request.id,
                    InitializeResult {
                        protocol_version: LATEST_PROTOCOL_VERSION.to_string(),
                        capabilities: ServerCapabilities {
                            tools: Some(JsonObject::new()),
                            ..ServerCapabilities::default()
                        },
                        server_info: Configuration::new("elicitation-server", "1.0.0"),
                        instructions: None,
                        meta: None,
                    },
                ))]),
                "tools/list" => {
                    let mut messages = Vec::new();
                    if let Some(server_request) = state.server_request.take() {
                        messages.push(JsonRpcMessage::Request(server_request));
                    }
                    messages.push(JsonRpcMessage::Response(JsonRpcResponse::success(
                        request.id,
                        ListToolsResult {
                            tools: Vec::new(),
                            next_cursor: None,
                            meta: None,
                        },
                    )));
                    Ok(messages)
                }
                _ => Ok(vec![JsonRpcMessage::Response(JsonRpcResponse::error(
                    request.id,
                    JsonRpcError::new(-32601, format!("Unsupported method: {}", request.method)),
                ))]),
            }
        }
    }

    #[derive(Clone, Debug)]
    struct LocalHttpRequest {
        method: String,
        headers: BTreeMap<String, String>,
        body: JsonValue,
    }

    struct LocalHttpResponse {
        status: u16,
        headers: BTreeMap<String, String>,
        body: String,
    }

    impl LocalHttpResponse {
        fn new<K, V, I>(status: u16, headers: I, body: impl Into<String>) -> Self
        where
            I: IntoIterator<Item = (K, V)>,
            K: Into<String>,
            V: Into<String>,
        {
            Self {
                status,
                headers: headers
                    .into_iter()
                    .map(|(key, value)| (key.into(), value.into()))
                    .collect(),
                body: body.into(),
            }
        }

        fn json(body: JsonValue) -> Self {
            Self::new(
                200,
                [("content-type", "application/json")],
                body.to_string(),
            )
        }

        fn empty(status: u16) -> Self {
            Self::new(status, [("content-type", "text/plain")], "")
        }

        fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
            self.headers.insert(key.into(), value.into());
            self
        }
    }

    struct LocalHttpServer {
        url: String,
        requests: Arc<Mutex<Vec<LocalHttpRequest>>>,
        stop: Arc<AtomicBool>,
        handle: Option<JoinHandle<()>>,
    }

    impl LocalHttpServer {
        fn new(responses: Vec<LocalHttpResponse>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind local HTTP server");
            listener
                .set_nonblocking(true)
                .expect("set local HTTP server nonblocking");
            let url = format!("http://{}", listener.local_addr().expect("local address"));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
            let stop = Arc::new(AtomicBool::new(false));
            let handle_requests = Arc::clone(&requests);
            let handle_responses = Arc::clone(&responses);
            let handle_stop = Arc::clone(&stop);

            let handle = thread::spawn(move || {
                while !handle_stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            handle_local_connection(stream, &handle_requests, &handle_responses);
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

    impl Drop for LocalHttpServer {
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
        responses: &Arc<Mutex<VecDeque<LocalHttpResponse>>>,
    ) {
        let mut buffer = Vec::new();
        let mut chunk = [0; 1024];
        let request = loop {
            let read = stream.read(&mut chunk).expect("read request");
            if read == 0 {
                return;
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(request) = parse_local_request(&buffer) {
                break request;
            }
        };
        requests.lock().expect("local requests lock").push(request);

        let response = responses
            .lock()
            .expect("local responses lock")
            .pop_front()
            .unwrap_or_else(|| LocalHttpResponse::empty(200));
        let body = response.body;
        let mut response_text = format!(
            "HTTP/1.1 {} OK\r\ncontent-length: {}\r\nconnection: close\r\n",
            response.status,
            body.len()
        );
        for (key, value) in response.headers {
            response_text.push_str(&format!("{key}: {value}\r\n"));
        }
        response_text.push_str("\r\n");
        response_text.push_str(&body);
        stream
            .write_all(response_text.as_bytes())
            .expect("write response");
    }

    fn parse_local_request(buffer: &[u8]) -> Option<LocalHttpRequest> {
        let header_end = buffer.windows(4).position(|window| window == b"\r\n\r\n")?;
        let head = String::from_utf8_lossy(&buffer[..header_end]);
        let mut lines = head.lines();
        let request_line = lines.next()?;
        let method = request_line.split_whitespace().next()?.to_string();
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
}
