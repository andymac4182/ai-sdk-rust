//! Portable MCP helpers for the Rust port of upstream `@ai-sdk/mcp`.

#![forbid(unsafe_code)]

use std::collections::BTreeSet;
use std::fmt;
use std::string::FromUtf8Error;

use ai_sdk_provider::{
    FileData, FileDataContent, JsonObject, JsonSchema, JsonValue, LanguageModelFilePart,
    LanguageModelTextPart, LanguageModelToolResultContentPart, LanguageModelToolResultOutput,
};
use ai_sdk_provider_utils::{Base64DecodeError, convert_base64_to_bytes};
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
    pub fn success(id: impl Into<JsonRpcId>, result: impl Into<JsonValue>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: id.into(),
            result: Some(result.into()),
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
}
