//! Open Plugin v1 MCP discovery contracts for Open Agents.
//!
//! This module intentionally stops at deterministic configuration discovery:
//! it never starts MCP subprocesses or network transports. Runtime launchers can
//! map the returned stdio/http/sse configs onto `ai-sdk-mcp`.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Standard Open Plugin v1 manifest path.
pub const OPEN_PLUGIN_MANIFEST_PATH: &str = ".plugin/plugin.json";

/// Standard Open Plugin v1 MCP config path.
pub const OPEN_PLUGIN_DEFAULT_MCP_CONFIG_PATH: &str = ".mcp.json";

/// Suggested diagnostic event for an unrecognized object-shaped manifest field.
pub const OPEN_PLUGIN_INVALID_OBJECT_EVENT: &str = "open_plugin.manifest.invalid_object";

/// Suggested diagnostic event for duplicate MCP server names across sources.
pub const OPEN_PLUGIN_MCP_NAME_CONFLICT_EVENT: &str = "open_plugin.mcp.name_conflict";

const OPEN_PLUGIN_INVALID_FIELD_EVENT: &str = "open_plugin.manifest.invalid_field";
const OPEN_PLUGIN_INVALID_PATH_EVENT: &str = "open_plugin.path.invalid";
const OPEN_PLUGIN_MCP_CONFIG_READ_FAILED_EVENT: &str = "open_plugin.mcp.config_read_failed";
const OPEN_PLUGIN_MCP_CONFIG_PARSE_FAILED_EVENT: &str = "open_plugin.mcp.config_parse_failed";
const OPEN_PLUGIN_MCP_INVALID_CONFIG_EVENT: &str = "open_plugin.mcp.invalid_config";
const OPEN_PLUGIN_MCP_INVALID_SERVER_EVENT: &str = "open_plugin.mcp.invalid_server";

/// Open Plugin manifest subset needed by OP-03.
///
/// OP-01 may replace this with a richer manifest type. Until then this adapter
/// keeps MCP discovery isolated to the Open Plugin fields it needs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPluginManifest {
    /// Plugin package name, used for MCP tool namespacing.
    pub name: String,

    /// Raw Open Plugin `mcpServers` field.
    #[serde(
        default,
        rename = "mcpServers",
        skip_serializing_if = "Option::is_none"
    )]
    pub mcp_servers: Option<Value>,

    /// Other manifest fields are intentionally preserved for future OP buckets.
    #[serde(default, flatten, skip_serializing_if = "Map::is_empty")]
    pub extra: Map<String, Value>,
}

impl OpenPluginManifest {
    /// Converts the manifest into the narrow MCP discovery adapter.
    pub fn into_mcp_adapter(self) -> OpenPluginMcpManifestAdapter {
        OpenPluginMcpManifestAdapter {
            plugin_name: self.name,
            mcp_servers: self.mcp_servers,
        }
    }
}

/// Narrow adapter seam for OP-01 manifest integration.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenPluginMcpManifestAdapter {
    /// Plugin package name, used for MCP tool namespacing.
    pub plugin_name: String,

    /// Raw Open Plugin `mcpServers` field.
    pub mcp_servers: Option<Value>,
}

impl OpenPluginMcpManifestAdapter {
    /// Creates an adapter from manifest parts owned by a richer manifest parser.
    pub fn new(plugin_name: impl Into<String>, mcp_servers: Option<Value>) -> Self {
        Self {
            plugin_name: plugin_name.into(),
            mcp_servers,
        }
    }
}

/// Options for Open Plugin MCP discovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenPluginMcpDiscoveryOptions {
    /// Absolute plugin package root.
    pub plugin_root: PathBuf,

    /// Optional host-managed persistent data directory for the plugin.
    pub plugin_data: Option<PathBuf>,
}

impl OpenPluginMcpDiscoveryOptions {
    /// Creates discovery options for a plugin root.
    pub fn new(plugin_root: impl Into<PathBuf>) -> Self {
        Self {
            plugin_root: plugin_root.into(),
            plugin_data: None,
        }
    }

    /// Sets the persistent data directory used for `${PLUGIN_DATA}` expansion.
    pub fn with_plugin_data(mut self, plugin_data: impl Into<PathBuf>) -> Self {
        self.plugin_data = Some(plugin_data.into());
        self
    }
}

/// Result of discovering MCP servers in one Open Plugin package.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPluginMcpDiscovery {
    /// Plugin package name used for namespacing.
    pub plugin_name: String,

    /// Winning MCP server configs keyed by server name in deterministic order.
    pub servers: BTreeMap<String, OpenPluginMcpServerConfig>,

    /// Non-fatal diagnostics emitted while loading MCP declarations.
    pub diagnostics: Vec<OpenPluginDiagnostic>,
}

impl OpenPluginMcpDiscovery {
    /// Returns the discovered servers as a deterministic, name-sorted list.
    pub fn server_configs(&self) -> Vec<&OpenPluginMcpServerConfig> {
        self.servers.values().collect()
    }
}

/// Machine-readable Open Plugin diagnostic.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPluginDiagnostic {
    /// Diagnostic severity.
    pub level: OpenPluginDiagnosticLevel,

    /// Stable event identifier.
    pub event: String,

    /// Plugin package name.
    pub plugin: String,

    /// Human-readable diagnostic message.
    pub message: String,

    /// Manifest field associated with the diagnostic, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,

    /// MCP server associated with the diagnostic, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,

    /// Path associated with the diagnostic, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,

    /// Source associated with the diagnostic, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    /// Deterministic action taken by the loader.
    pub action: String,

    /// Whether plugin loading may continue.
    pub continue_loading: bool,
}

impl OpenPluginDiagnostic {
    fn warn(
        plugin: impl Into<String>,
        event: impl Into<String>,
        message: impl Into<String>,
        action: impl Into<String>,
    ) -> Self {
        Self {
            level: OpenPluginDiagnosticLevel::Warn,
            event: event.into(),
            plugin: plugin.into(),
            message: message.into(),
            field: None,
            server: None,
            path: None,
            source: None,
            action: action.into(),
            continue_loading: true,
        }
    }

    fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }

    fn with_server(mut self, server: impl Into<String>) -> Self {
        self.server = Some(server.into());
        self
    }

    fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

/// Diagnostic severity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenPluginDiagnosticLevel {
    /// Informational diagnostic.
    Info,

    /// Non-fatal warning.
    Warn,

    /// Component-level error. Plugin loading can still continue when
    /// `continue_loading` is true.
    Error,
}

/// MCP server config plus Open Agents namespacing metadata.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPluginMcpServerConfig {
    /// Plugin package name.
    pub plugin_name: String,

    /// MCP server name inside the plugin.
    pub server_name: String,

    /// Source that produced this winning server config.
    pub source: OpenPluginMcpConfigSource,

    /// Transport-oriented config that can later be mapped to `ai-sdk-mcp`.
    pub transport: OpenPluginMcpTransportConfig,

    /// Raw server config with Open Plugin runtime placeholders expanded in the
    /// fields covered by the spec.
    pub raw_config: Value,
}

impl OpenPluginMcpServerConfig {
    /// Returns the Open Plugin v1 recommended model-facing MCP tool id.
    pub fn namespaced_tool_id(&self, tool_name: impl AsRef<str>) -> String {
        open_plugin_mcp_tool_id(&self.plugin_name, &self.server_name, tool_name)
    }
}

/// Discovery source for one MCP server config.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum OpenPluginMcpConfigSource {
    /// Default `.mcp.json` source.
    Default { path: PathBuf },

    /// Manifest-declared path source.
    ManifestPath { index: usize, path: PathBuf },

    /// Inline manifest source.
    ManifestInline,
}

impl OpenPluginMcpConfigSource {
    fn label(&self) -> String {
        match self {
            Self::Default { path } => format!("default:{}", display_path(path)),
            Self::ManifestPath { index, path } => {
                format!("manifest[{index}]:{}", display_path(path))
            }
            Self::ManifestInline => "manifest:inline".to_string(),
        }
    }
}

/// Transport-oriented MCP config.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum OpenPluginMcpTransportConfig {
    /// Stdio MCP server process.
    Stdio(OpenPluginMcpStdioConfig),

    /// Streamable HTTP MCP endpoint.
    Http(OpenPluginMcpHttpConfig),

    /// Standalone SSE MCP endpoint.
    Sse(OpenPluginMcpHttpConfig),

    /// Valid JSON object using a transport shape this discovery layer does not
    /// yet know how to launch.
    Unknown,
}

/// Stdio MCP server config.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPluginMcpStdioConfig {
    /// Expanded process command.
    pub command: String,

    /// Expanded process arguments.
    pub args: Vec<String>,

    /// Expanded process environment plus host-provided plugin variables.
    pub env: BTreeMap<String, String>,

    /// Expanded process working directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
}

/// HTTP/SSE MCP endpoint config.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPluginMcpHttpConfig {
    /// Endpoint URL.
    pub url: String,

    /// Request headers from the MCP config.
    pub headers: BTreeMap<String, String>,
}

/// Error returned while loading the required manifest.
#[derive(Debug)]
pub enum OpenPluginMcpLoadError {
    /// Manifest file could not be read.
    ReadManifest { path: PathBuf, source: io::Error },

    /// Manifest JSON could not be parsed.
    ParseManifest {
        path: PathBuf,
        source: serde_json::Error,
    },

    /// Manifest content is missing required data.
    InvalidManifest { path: PathBuf, message: String },
}

impl fmt::Display for OpenPluginMcpLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadManifest { path, source } => {
                write!(
                    formatter,
                    "failed to read Open Plugin manifest {}: {source}",
                    display_path(path)
                )
            }
            Self::ParseManifest { path, source } => {
                write!(
                    formatter,
                    "failed to parse Open Plugin manifest {}: {source}",
                    display_path(path)
                )
            }
            Self::InvalidManifest { path, message } => {
                write!(
                    formatter,
                    "invalid Open Plugin manifest {}: {message}",
                    display_path(path)
                )
            }
        }
    }
}

impl Error for OpenPluginMcpLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadManifest { source, .. } => Some(source),
            Self::ParseManifest { source, .. } => Some(source),
            Self::InvalidManifest { .. } => None,
        }
    }
}

/// Loads an Open Plugin package manifest and discovers deterministic MCP config.
pub fn load_open_plugin_mcp_servers(
    options: OpenPluginMcpDiscoveryOptions,
) -> Result<OpenPluginMcpDiscovery, OpenPluginMcpLoadError> {
    let manifest_path = options.plugin_root.join(OPEN_PLUGIN_MANIFEST_PATH);
    let manifest_text = fs::read_to_string(&manifest_path).map_err(|source| {
        OpenPluginMcpLoadError::ReadManifest {
            path: manifest_path.clone(),
            source,
        }
    })?;
    let manifest: OpenPluginManifest = serde_json::from_str(&manifest_text).map_err(|source| {
        OpenPluginMcpLoadError::ParseManifest {
            path: manifest_path.clone(),
            source,
        }
    })?;

    if manifest.name.trim().is_empty() {
        return Err(OpenPluginMcpLoadError::InvalidManifest {
            path: manifest_path,
            message: "manifest field \"name\" must not be empty".to_string(),
        });
    }

    Ok(discover_open_plugin_mcp_servers_from_manifest(
        &options,
        manifest.into_mcp_adapter(),
    ))
}

/// Discovers deterministic MCP config from an already parsed manifest adapter.
pub fn discover_open_plugin_mcp_servers_from_manifest(
    options: &OpenPluginMcpDiscoveryOptions,
    manifest: OpenPluginMcpManifestAdapter,
) -> OpenPluginMcpDiscovery {
    let mut discovery = OpenPluginMcpDiscovery {
        plugin_name: manifest.plugin_name.clone(),
        ..OpenPluginMcpDiscovery::default()
    };

    let sources = match manifest.mcp_servers {
        None => default_mcp_sources(options),
        Some(field) => manifest_mcp_sources(options, &manifest.plugin_name, field, &mut discovery),
    };

    for source in sources {
        match source {
            PendingMcpSource::File { source, path } => {
                if let Some(config) =
                    read_mcp_config_file(&manifest.plugin_name, &path, &source, &mut discovery)
                {
                    load_mcp_config_value(
                        options,
                        &manifest.plugin_name,
                        &source,
                        config,
                        &mut discovery,
                    );
                }
            }
            PendingMcpSource::Inline { source, value } => {
                load_mcp_config_value(
                    options,
                    &manifest.plugin_name,
                    &source,
                    value,
                    &mut discovery,
                );
            }
        }
    }

    discovery
}

/// Returns the Open Plugin v1 recommended model-facing MCP tool id.
pub fn open_plugin_mcp_tool_id(
    plugin_name: impl AsRef<str>,
    server_name: impl AsRef<str>,
    tool_name: impl AsRef<str>,
) -> String {
    format!(
        "mcp__plugin_{}_{}__{}",
        plugin_name.as_ref(),
        server_name.as_ref(),
        tool_name.as_ref()
    )
}

#[derive(Clone, Debug, PartialEq)]
enum PendingMcpSource {
    File {
        source: OpenPluginMcpConfigSource,
        path: PathBuf,
    },
    Inline {
        source: OpenPluginMcpConfigSource,
        value: Value,
    },
}

fn default_mcp_sources(options: &OpenPluginMcpDiscoveryOptions) -> Vec<PendingMcpSource> {
    let path = options
        .plugin_root
        .join(OPEN_PLUGIN_DEFAULT_MCP_CONFIG_PATH);
    if path.exists() {
        vec![PendingMcpSource::File {
            source: OpenPluginMcpConfigSource::Default { path: path.clone() },
            path,
        }]
    } else {
        Vec::new()
    }
}

fn manifest_mcp_sources(
    options: &OpenPluginMcpDiscoveryOptions,
    plugin_name: &str,
    field: Value,
    discovery: &mut OpenPluginMcpDiscovery,
) -> Vec<PendingMcpSource> {
    match field {
        Value::String(path) => manifest_path_sources(options, plugin_name, vec![path], discovery),
        Value::Array(paths) => {
            let mut valid_paths = Vec::with_capacity(paths.len());
            for path in paths {
                if let Some(path) = path.as_str() {
                    valid_paths.push(path.to_string());
                } else {
                    discovery.diagnostics.push(
                        OpenPluginDiagnostic::warn(
                            plugin_name,
                            OPEN_PLUGIN_INVALID_FIELD_EVENT,
                            "manifest field \"mcpServers\" array entries must be path strings",
                            "ignored",
                        )
                        .with_field("mcpServers"),
                    );
                }
            }
            manifest_path_sources(options, plugin_name, valid_paths, discovery)
        }
        Value::Object(object) => {
            let has_inline = object.contains_key("mcpServers");
            let has_paths = object.contains_key("paths");
            match (has_inline, has_paths) {
                (true, false) => vec![PendingMcpSource::Inline {
                    source: OpenPluginMcpConfigSource::ManifestInline,
                    value: Value::Object(object),
                }],
                (false, true) => {
                    let Some(Value::Array(paths)) = object.get("paths") else {
                        push_invalid_object_diagnostic(plugin_name, discovery);
                        return Vec::new();
                    };
                    let Some(paths) = paths
                        .iter()
                        .map(|path| path.as_str().map(str::to_string))
                        .collect::<Option<Vec<_>>>()
                    else {
                        push_invalid_object_diagnostic(plugin_name, discovery);
                        return Vec::new();
                    };
                    manifest_path_sources(options, plugin_name, paths, discovery)
                }
                _ => {
                    push_invalid_object_diagnostic(plugin_name, discovery);
                    Vec::new()
                }
            }
        }
        _ => {
            discovery.diagnostics.push(
                OpenPluginDiagnostic::warn(
                    plugin_name,
                    OPEN_PLUGIN_INVALID_FIELD_EVENT,
                    "manifest field \"mcpServers\" must be a path string, path string array, path config, or inline MCP config",
                    "ignored",
                )
                .with_field("mcpServers"),
            );
            Vec::new()
        }
    }
}

fn manifest_path_sources(
    options: &OpenPluginMcpDiscoveryOptions,
    plugin_name: &str,
    paths: Vec<String>,
    discovery: &mut OpenPluginMcpDiscovery,
) -> Vec<PendingMcpSource> {
    paths
        .into_iter()
        .enumerate()
        .filter_map(|(index, path)| {
            let resolved = resolve_manifest_mcp_path(options, plugin_name, &path, discovery)?;
            Some(PendingMcpSource::File {
                source: OpenPluginMcpConfigSource::ManifestPath {
                    index,
                    path: resolved.clone(),
                },
                path: resolved,
            })
        })
        .collect()
}

fn push_invalid_object_diagnostic(plugin_name: &str, discovery: &mut OpenPluginMcpDiscovery) {
    discovery.diagnostics.push(
        OpenPluginDiagnostic::warn(
            plugin_name,
            OPEN_PLUGIN_INVALID_OBJECT_EVENT,
            "manifest field \"mcpServers\" is invalid: expected either a path config with \"paths\" key or an inline config with \"mcpServers\" key",
            "ignored",
        )
        .with_field("mcpServers"),
    );
}

fn resolve_manifest_mcp_path(
    options: &OpenPluginMcpDiscoveryOptions,
    plugin_name: &str,
    declared_path: &str,
    discovery: &mut OpenPluginMcpDiscovery,
) -> Option<PathBuf> {
    if !declared_path.starts_with("./") {
        discovery.diagnostics.push(
            OpenPluginDiagnostic::warn(
                plugin_name,
                OPEN_PLUGIN_INVALID_PATH_EVENT,
                format!(
                    "manifest field \"mcpServers\" path \"{declared_path}\" must start with \"./\""
                ),
                "ignored",
            )
            .with_field("mcpServers")
            .with_path(declared_path),
        );
        return None;
    }

    let mut normalized = PathBuf::new();
    for component in Path::new(declared_path).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                discovery.diagnostics.push(
                    OpenPluginDiagnostic::warn(
                        plugin_name,
                        OPEN_PLUGIN_INVALID_PATH_EVENT,
                        format!(
                            "manifest field \"mcpServers\" path \"{declared_path}\" must stay inside the plugin root"
                        ),
                        "ignored",
                    )
                    .with_field("mcpServers")
                    .with_path(declared_path),
                );
                return None;
            }
        }
    }

    let resolved = options.plugin_root.join(&normalized);
    if resolved.is_dir() {
        discovery.diagnostics.push(
            OpenPluginDiagnostic::warn(
                plugin_name,
                OPEN_PLUGIN_INVALID_PATH_EVENT,
                format!(
                    "manifest field \"mcpServers\" path \"{declared_path}\" points to a directory; expected a JSON file"
                ),
                "ignored",
            )
            .with_field("mcpServers")
            .with_path(display_path(&resolved)),
        );
        return None;
    }

    Some(resolved)
}

fn read_mcp_config_file(
    plugin_name: &str,
    path: &Path,
    source: &OpenPluginMcpConfigSource,
    discovery: &mut OpenPluginMcpDiscovery,
) -> Option<Value> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) => {
            discovery.diagnostics.push(
                OpenPluginDiagnostic::warn(
                    plugin_name,
                    OPEN_PLUGIN_MCP_CONFIG_READ_FAILED_EVENT,
                    format!("failed to read MCP config {}: {error}", display_path(path)),
                    "ignored",
                )
                .with_path(display_path(path))
                .with_source(source.label()),
            );
            return None;
        }
    };

    match serde_json::from_str(&contents) {
        Ok(config) => Some(config),
        Err(error) => {
            discovery.diagnostics.push(
                OpenPluginDiagnostic::warn(
                    plugin_name,
                    OPEN_PLUGIN_MCP_CONFIG_PARSE_FAILED_EVENT,
                    format!("failed to parse MCP config {}: {error}", display_path(path)),
                    "ignored",
                )
                .with_path(display_path(path))
                .with_source(source.label()),
            );
            None
        }
    }
}

fn load_mcp_config_value(
    options: &OpenPluginMcpDiscoveryOptions,
    plugin_name: &str,
    source: &OpenPluginMcpConfigSource,
    config: Value,
    discovery: &mut OpenPluginMcpDiscovery,
) {
    let Some(servers) = config
        .as_object()
        .and_then(|object| object.get("mcpServers"))
        .and_then(Value::as_object)
    else {
        discovery.diagnostics.push(
            OpenPluginDiagnostic::warn(
                plugin_name,
                OPEN_PLUGIN_MCP_INVALID_CONFIG_EVENT,
                "MCP config must contain a top-level \"mcpServers\" object",
                "ignored",
            )
            .with_source(source.label()),
        );
        return;
    };

    for (server_name, server_config) in servers {
        if discovery.servers.contains_key(server_name) {
            discovery.diagnostics.push(
                OpenPluginDiagnostic::warn(
                    plugin_name,
                    OPEN_PLUGIN_MCP_NAME_CONFLICT_EVENT,
                    format!(
                        "MCP server name \"{server_name}\" is defined in multiple discovery sources; using first definition"
                    ),
                    "used_first",
                )
                .with_server(server_name)
                .with_source(source.label()),
            );
            continue;
        }

        let Some(server_config) = parse_mcp_server_config(
            options,
            plugin_name,
            server_name,
            source,
            server_config,
            discovery,
        ) else {
            continue;
        };
        discovery
            .servers
            .insert(server_name.to_string(), server_config);
    }
}

fn parse_mcp_server_config(
    options: &OpenPluginMcpDiscoveryOptions,
    plugin_name: &str,
    server_name: &str,
    source: &OpenPluginMcpConfigSource,
    server_config: &Value,
    discovery: &mut OpenPluginMcpDiscovery,
) -> Option<OpenPluginMcpServerConfig> {
    let Some(server_object) = server_config.as_object() else {
        discovery.diagnostics.push(
            OpenPluginDiagnostic::warn(
                plugin_name,
                OPEN_PLUGIN_MCP_INVALID_SERVER_EVENT,
                format!("MCP server \"{server_name}\" config must be a JSON object"),
                "ignored",
            )
            .with_server(server_name)
            .with_source(source.label()),
        );
        return None;
    };

    let expanded = expand_mcp_runtime_fields(server_object, options);
    let transport = parse_transport_config(
        plugin_name,
        server_name,
        &expanded,
        options,
        source,
        discovery,
    )?;

    Some(OpenPluginMcpServerConfig {
        plugin_name: plugin_name.to_string(),
        server_name: server_name.to_string(),
        source: source.clone(),
        transport,
        raw_config: Value::Object(expanded),
    })
}

fn parse_transport_config(
    plugin_name: &str,
    server_name: &str,
    server_object: &Map<String, Value>,
    options: &OpenPluginMcpDiscoveryOptions,
    source: &OpenPluginMcpConfigSource,
    discovery: &mut OpenPluginMcpDiscovery,
) -> Option<OpenPluginMcpTransportConfig> {
    if server_object.contains_key("command") {
        let Some(command) = get_string_field(server_object, "command") else {
            push_invalid_server_field(plugin_name, server_name, "command", source, discovery);
            return None;
        };
        let args = match get_optional_string_array_field(server_object, "args") {
            Ok(args) => args.unwrap_or_default(),
            Err(()) => {
                push_invalid_server_field(plugin_name, server_name, "args", source, discovery);
                return None;
            }
        };
        let mut env = match get_optional_string_map_field(server_object, "env") {
            Ok(env) => env.unwrap_or_default(),
            Err(()) => {
                push_invalid_server_field(plugin_name, server_name, "env", source, discovery);
                return None;
            }
        };
        env.insert(
            "PLUGIN_ROOT".to_string(),
            display_path(&options.plugin_root),
        );
        if let Some(plugin_data) = &options.plugin_data {
            env.insert("PLUGIN_DATA".to_string(), display_path(plugin_data));
        }
        let cwd = match server_object.get("cwd") {
            None | Some(Value::Null) => None,
            Some(Value::String(cwd)) => Some(PathBuf::from(cwd)),
            Some(_) => {
                push_invalid_server_field(plugin_name, server_name, "cwd", source, discovery);
                return None;
            }
        };
        return Some(OpenPluginMcpTransportConfig::Stdio(
            OpenPluginMcpStdioConfig {
                command: command.to_string(),
                args,
                env,
                cwd,
            },
        ));
    }

    if server_object.contains_key("url") {
        let Some(url) = get_string_field(server_object, "url") else {
            push_invalid_server_field(plugin_name, server_name, "url", source, discovery);
            return None;
        };
        let headers = match get_optional_string_map_field(server_object, "headers") {
            Ok(headers) => headers.unwrap_or_default(),
            Err(()) => {
                push_invalid_server_field(plugin_name, server_name, "headers", source, discovery);
                return None;
            }
        };
        let config = OpenPluginMcpHttpConfig {
            url: url.to_string(),
            headers,
        };
        if server_object
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|transport_type| transport_type.eq_ignore_ascii_case("sse"))
        {
            return Some(OpenPluginMcpTransportConfig::Sse(config));
        }
        return Some(OpenPluginMcpTransportConfig::Http(config));
    }

    Some(OpenPluginMcpTransportConfig::Unknown)
}

fn expand_mcp_runtime_fields(
    server_object: &Map<String, Value>,
    options: &OpenPluginMcpDiscoveryOptions,
) -> Map<String, Value> {
    let mut expanded = server_object.clone();
    if let Some(Value::String(command)) = expanded.get_mut("command") {
        *command = expand_placeholders(command, options);
    }
    if let Some(Value::String(cwd)) = expanded.get_mut("cwd") {
        *cwd = expand_placeholders(cwd, options);
    }
    if let Some(Value::Array(args)) = expanded.get_mut("args") {
        for arg in args {
            if let Value::String(arg) = arg {
                *arg = expand_placeholders(arg, options);
            }
        }
    }
    if let Some(Value::Object(env)) = expanded.get_mut("env") {
        for value in env.values_mut() {
            if let Value::String(value) = value {
                *value = expand_placeholders(value, options);
            }
        }
    }
    expanded
}

fn expand_placeholders(value: &str, options: &OpenPluginMcpDiscoveryOptions) -> String {
    let mut expanded = value.replace("${PLUGIN_ROOT}", &display_path(&options.plugin_root));
    if let Some(plugin_data) = &options.plugin_data {
        expanded = expanded.replace("${PLUGIN_DATA}", &display_path(plugin_data));
    }
    expanded
}

fn get_string_field<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
    object.get(field).and_then(Value::as_str)
}

fn get_optional_string_array_field(
    object: &Map<String, Value>,
    field: &str,
) -> Result<Option<Vec<String>>, ()> {
    let Some(value) = object.get(field) else {
        return Ok(None);
    };
    let Value::Array(values) = value else {
        return Err(());
    };
    values
        .iter()
        .map(|value| value.as_str().map(str::to_string))
        .collect::<Option<Vec<_>>>()
        .ok_or(())
        .map(Some)
}

fn get_optional_string_map_field(
    object: &Map<String, Value>,
    field: &str,
) -> Result<Option<BTreeMap<String, String>>, ()> {
    let Some(value) = object.get(field) else {
        return Ok(None);
    };
    let Value::Object(values) = value else {
        return Err(());
    };
    values
        .iter()
        .map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string())))
        .collect::<Option<BTreeMap<_, _>>>()
        .ok_or(())
        .map(Some)
}

fn push_invalid_server_field(
    plugin_name: &str,
    server_name: &str,
    field: &str,
    source: &OpenPluginMcpConfigSource,
    discovery: &mut OpenPluginMcpDiscovery,
) {
    discovery.diagnostics.push(
        OpenPluginDiagnostic::warn(
            plugin_name,
            OPEN_PLUGIN_MCP_INVALID_SERVER_EVENT,
            format!("MCP server \"{server_name}\" field \"{field}\" has an invalid shape"),
            "ignored",
        )
        .with_server(server_name)
        .with_field(field)
        .with_source(source.label()),
    );
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestPlugin {
        root: PathBuf,
    }

    impl TestPlugin {
        fn new(name: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("test clock is valid")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "open_agents_core_open_plugin_{name}_{}_{}",
                std::process::id(),
                unique
            ));
            fs::create_dir_all(root.join(".plugin")).expect("plugin manifest directory is created");
            Self { root }
        }

        fn write(&self, relative_path: &str, contents: &str) {
            let path = self.root.join(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("parent directory is created");
            }
            fs::write(path, contents).expect("test fixture is written");
        }

        fn write_manifest(&self, manifest: Value) {
            self.write(
                OPEN_PLUGIN_MANIFEST_PATH,
                &serde_json::to_string_pretty(&manifest).expect("manifest serializes"),
            );
        }

        fn discover(&self) -> OpenPluginMcpDiscovery {
            load_open_plugin_mcp_servers(OpenPluginMcpDiscoveryOptions::new(&self.root))
                .expect("MCP discovery loads")
        }

        fn discover_with_data(&self, plugin_data: &Path) -> OpenPluginMcpDiscovery {
            load_open_plugin_mcp_servers(
                OpenPluginMcpDiscoveryOptions::new(&self.root).with_plugin_data(plugin_data),
            )
            .expect("MCP discovery loads")
        }
    }

    impl Drop for TestPlugin {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn open_plugin_mcp_loads_default_mcp_json_when_manifest_field_absent() {
        let plugin = TestPlugin::new("default");
        plugin.write_manifest(json!({ "name": "devtools" }));
        plugin.write(
            OPEN_PLUGIN_DEFAULT_MCP_CONFIG_PATH,
            r#"{
              "mcpServers": {
                "filesystem": {
                  "command": "${PLUGIN_ROOT}/bin/fs-server",
                  "args": ["--root", "${PLUGIN_ROOT}/data"]
                }
              }
            }"#,
        );

        let discovery = plugin.discover();

        let server = discovery
            .servers
            .get("filesystem")
            .expect("server is loaded");
        assert!(matches!(
            server.source,
            OpenPluginMcpConfigSource::Default { .. }
        ));
        let OpenPluginMcpTransportConfig::Stdio(stdio) = &server.transport else {
            panic!("server uses stdio transport");
        };
        assert_eq!(
            stdio.command,
            plugin.root.join("bin/fs-server").to_string_lossy()
        );
        assert_eq!(
            stdio.args,
            vec![
                "--root".to_string(),
                plugin.root.join("data").to_string_lossy().into_owned()
            ]
        );
        assert_eq!(
            stdio.env.get("PLUGIN_ROOT"),
            Some(&plugin.root.to_string_lossy().into_owned())
        );
        assert!(discovery.diagnostics.is_empty());
    }

    #[test]
    fn open_plugin_mcp_manifest_path_override_skips_default_config() {
        let plugin = TestPlugin::new("path_override");
        plugin.write_manifest(json!({
            "name": "deploy-tools",
            "mcpServers": "./config/mcp.json"
        }));
        plugin.write(
            OPEN_PLUGIN_DEFAULT_MCP_CONFIG_PATH,
            r#"{ "mcpServers": { "default": { "command": "default" } } }"#,
        );
        plugin.write(
            "config/mcp.json",
            r#"{ "mcpServers": { "custom": { "command": "custom" } } }"#,
        );

        let discovery = plugin.discover();

        assert!(discovery.servers.contains_key("custom"));
        assert!(!discovery.servers.contains_key("default"));
        assert!(matches!(
            discovery.servers["custom"].source,
            OpenPluginMcpConfigSource::ManifestPath { index: 0, .. }
        ));
    }

    #[test]
    fn open_plugin_mcp_inline_config_uses_manifest_servers() {
        let plugin = TestPlugin::new("inline");
        plugin.write_manifest(json!({
            "name": "database-tools",
            "mcpServers": {
                "mcpServers": {
                    "database": {
                        "type": "sse",
                        "url": "https://example.test/sse"
                    }
                }
            }
        }));

        let discovery = plugin.discover();

        let server = discovery.servers.get("database").expect("server is loaded");
        assert!(matches!(
            server.source,
            OpenPluginMcpConfigSource::ManifestInline
        ));
        assert_eq!(
            server.transport,
            OpenPluginMcpTransportConfig::Sse(OpenPluginMcpHttpConfig {
                url: "https://example.test/sse".to_string(),
                headers: BTreeMap::new()
            })
        );
    }

    #[test]
    fn open_plugin_mcp_invalid_manifest_object_shape_is_non_fatal() {
        let plugin = TestPlugin::new("invalid_object");
        plugin.write_manifest(json!({
            "name": "devtools",
            "mcpServers": {
                "paths": ["./config/mcp.json"],
                "mcpServers": {}
            }
        }));
        plugin.write(
            OPEN_PLUGIN_DEFAULT_MCP_CONFIG_PATH,
            r#"{ "mcpServers": { "default": { "command": "default" } } }"#,
        );

        let discovery = plugin.discover();

        assert!(discovery.servers.is_empty());
        assert_eq!(discovery.diagnostics.len(), 1);
        assert_eq!(
            discovery.diagnostics[0].event,
            OPEN_PLUGIN_INVALID_OBJECT_EVENT
        );
        assert_eq!(discovery.diagnostics[0].action, "ignored");
        assert!(discovery.diagnostics[0].continue_loading);
    }

    #[test]
    fn open_plugin_mcp_expands_plugin_root_and_data_placeholders() {
        let plugin = TestPlugin::new("expansion");
        let plugin_data = plugin.root.with_file_name("expansion-data");
        plugin.write_manifest(json!({ "name": "devtools" }));
        plugin.write(
            OPEN_PLUGIN_DEFAULT_MCP_CONFIG_PATH,
            r#"{
              "mcpServers": {
                "filesystem": {
                  "command": "${PLUGIN_ROOT}/bin/server",
                  "args": ["--cache", "${PLUGIN_DATA}/cache"],
                  "env": {
                    "ROOT": "${PLUGIN_ROOT}",
                    "DATA": "${PLUGIN_DATA}"
                  },
                  "cwd": "${PLUGIN_ROOT}/workspace"
                }
              }
            }"#,
        );

        let discovery = plugin.discover_with_data(&plugin_data);
        let server = discovery
            .servers
            .get("filesystem")
            .expect("server is loaded");
        let OpenPluginMcpTransportConfig::Stdio(stdio) = &server.transport else {
            panic!("server uses stdio transport");
        };

        assert_eq!(
            stdio.command,
            plugin.root.join("bin/server").to_string_lossy()
        );
        assert_eq!(
            stdio.args,
            vec![
                "--cache".to_string(),
                plugin_data.join("cache").to_string_lossy().into_owned()
            ]
        );
        assert_eq!(
            stdio.env.get("ROOT"),
            Some(&plugin.root.to_string_lossy().into_owned())
        );
        assert_eq!(
            stdio.env.get("DATA"),
            Some(&plugin_data.to_string_lossy().into_owned())
        );
        assert_eq!(
            stdio.env.get("PLUGIN_DATA"),
            Some(&plugin_data.to_string_lossy().into_owned())
        );
        assert_eq!(
            stdio.cwd.as_deref(),
            Some(plugin.root.join("workspace").as_path())
        );
        assert_eq!(
            server.raw_config["env"]["DATA"],
            json!(plugin_data.to_string_lossy().into_owned())
        );

        let _ = fs::remove_dir_all(plugin_data);
    }

    #[test]
    fn open_plugin_mcp_duplicate_server_names_emit_conflict_and_first_source_wins() {
        let plugin = TestPlugin::new("duplicates");
        plugin.write_manifest(json!({
            "name": "devtools",
            "mcpServers": ["./a.json", "./b.json"]
        }));
        plugin.write(
            "a.json",
            r#"{ "mcpServers": { "filesystem": { "command": "first" } } }"#,
        );
        plugin.write(
            "b.json",
            r#"{ "mcpServers": { "filesystem": { "command": "second" } } }"#,
        );

        let discovery = plugin.discover();
        let server = discovery
            .servers
            .get("filesystem")
            .expect("server is loaded");
        let OpenPluginMcpTransportConfig::Stdio(stdio) = &server.transport else {
            panic!("server uses stdio transport");
        };

        assert_eq!(stdio.command, "first");
        assert_eq!(discovery.diagnostics.len(), 1);
        assert_eq!(
            discovery.diagnostics[0].event,
            OPEN_PLUGIN_MCP_NAME_CONFLICT_EVENT
        );
        assert_eq!(
            discovery.diagnostics[0].server.as_deref(),
            Some("filesystem")
        );
        assert_eq!(discovery.diagnostics[0].action, "used_first");
    }

    #[test]
    fn open_plugin_mcp_namespaced_tool_id_matches_spec_format() {
        let plugin = TestPlugin::new("namespace");
        plugin.write_manifest(json!({ "name": "deploy-tools" }));
        plugin.write(
            OPEN_PLUGIN_DEFAULT_MCP_CONFIG_PATH,
            r#"{ "mcpServers": { "database": { "command": "db" } } }"#,
        );

        let discovery = plugin.discover();
        let server = discovery.servers.get("database").expect("server is loaded");

        assert_eq!(
            open_plugin_mcp_tool_id("deploy-tools", "database", "query"),
            "mcp__plugin_deploy-tools_database__query"
        );
        assert_eq!(
            server.namespaced_tool_id("query"),
            "mcp__plugin_deploy-tools_database__query"
        );
    }

    #[test]
    fn open_plugin_mcp_rejects_invalid_paths_and_keeps_valid_sources() {
        let plugin = TestPlugin::new("paths");
        plugin.write_manifest(json!({
            "name": "devtools",
            "mcpServers": [
                "config/mcp.json",
                "./../escape.json",
                "./directory",
                "./valid.json"
            ]
        }));
        fs::create_dir_all(plugin.root.join("directory")).expect("directory path fixture exists");
        plugin.write(
            "valid.json",
            r#"{ "mcpServers": { "valid": { "command": "ok" } } }"#,
        );

        let discovery = plugin.discover();

        assert_eq!(discovery.servers.len(), 1);
        assert!(discovery.servers.contains_key("valid"));
        assert_eq!(discovery.diagnostics.len(), 3);
        assert!(
            discovery
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.event == OPEN_PLUGIN_INVALID_PATH_EVENT)
        );
    }

    #[test]
    fn open_plugin_mcp_invalid_config_shape_does_not_block_other_sources() {
        let plugin = TestPlugin::new("invalid_config");
        plugin.write_manifest(json!({
            "name": "devtools",
            "mcpServers": ["./bad.json", "./good.json"]
        }));
        plugin.write("bad.json", r#"{ "mcpServers": [] }"#);
        plugin.write(
            "good.json",
            r#"{ "mcpServers": { "good": { "command": "ok" } } }"#,
        );

        let discovery = plugin.discover();

        assert_eq!(discovery.servers.len(), 1);
        assert!(discovery.servers.contains_key("good"));
        assert_eq!(discovery.diagnostics.len(), 1);
        assert_eq!(
            discovery.diagnostics[0].event,
            OPEN_PLUGIN_MCP_INVALID_CONFIG_EVENT
        );
        assert!(discovery.diagnostics[0].continue_loading);
    }
}
