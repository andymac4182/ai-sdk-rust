//! Open Plugin Specification v1.0.0 manifest contracts.
//!
//! This module owns the reusable parsing, validation, and diagnostic foundation
//! for plugin manifests. Runtime discovery, skill loading, MCP process startup,
//! and host-specific permission decisions are intentionally left to downstream
//! Open Agents crates.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Open Plugin spec version implemented by this manifest loader.
pub const OPEN_PLUGIN_SPEC_VERSION: &str = "1.0.0";

/// Upstream Open Plugin spec repository verified for OP-01.
pub const OPEN_PLUGIN_SPEC_SOURCE_REPOSITORY: &str = "github.com/vercel-labs/open-plugin-spec";

/// Upstream Open Plugin spec ref fetched by OpenSrc.
pub const OPEN_PLUGIN_SPEC_SOURCE_REF: &str = "main";

/// Remote HEAD verified with `git ls-remote` on 2026-06-02.
pub const OPEN_PLUGIN_SPEC_SOURCE_HEAD: &str = "cd5f34e7f1b9398267843d2e32f38e57a58597c2";

/// Required vendor-neutral manifest path relative to a plugin root.
pub const OPEN_PLUGIN_MANIFEST_PATH: &str = ".plugin/plugin.json";

const PLUGIN_RELATIVE_PREFIX: &str = "./";
const MCP_SERVERS_FIELD: &str = "mcpServers";
const PATHS_FIELD: &str = "paths";
const EXTENDED_COMPONENT_FIELDS: &[&str] = &[
    "commands",
    "agents",
    "rules",
    "hooks",
    "lspServers",
    "outputStyles",
];

/// Source metadata for the Open Plugin spec snapshot used by this crate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPluginSpecSourceSnapshot {
    /// Repository identifier.
    pub repository: String,
    /// Ref fetched into the local mirror.
    pub ref_name: String,
    /// Verified remote HEAD.
    pub head: String,
    /// Spec version implemented by this loader.
    pub spec_version: String,
}

impl Default for OpenPluginSpecSourceSnapshot {
    fn default() -> Self {
        Self {
            repository: OPEN_PLUGIN_SPEC_SOURCE_REPOSITORY.to_string(),
            ref_name: OPEN_PLUGIN_SPEC_SOURCE_REF.to_string(),
            head: OPEN_PLUGIN_SPEC_SOURCE_HEAD.to_string(),
            spec_version: OPEN_PLUGIN_SPEC_VERSION.to_string(),
        }
    }
}

/// Options controlling supplemental manifest lookup.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OpenPluginLoadOptions {
    vendor_manifest_dir: Option<String>,
}

impl OpenPluginLoadOptions {
    /// Creates default load options that only select `.plugin/plugin.json`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Selects a host-specific manifest directory, for example `.codex-plugin`.
    pub fn with_vendor_manifest_dir(mut self, directory: impl Into<String>) -> Self {
        self.vendor_manifest_dir = Some(directory.into());
        self
    }

    /// Selects a host-specific manifest from a tool name such as `codex`.
    pub fn with_vendor_prefix(mut self, tool_name: impl Into<String>) -> Self {
        let tool_name = tool_name.into();
        let normalized = tool_name
            .trim()
            .trim_start_matches('.')
            .trim_end_matches("-plugin")
            .to_string();
        self.vendor_manifest_dir = Some(format!(".{normalized}-plugin"));
        self
    }
}

/// Complete result of a manifest load attempt.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenPluginLoadReport {
    /// Canonical plugin root when it could be resolved.
    pub plugin_root: PathBuf,
    /// Manifest path selected for parsing.
    pub manifest_path: Option<PathBuf>,
    /// Parsed manifest when fatal validation passed.
    pub manifest: Option<OpenPluginManifest>,
    /// Deterministic diagnostics produced while loading.
    pub diagnostics: Vec<OpenPluginDiagnostic>,
}

impl OpenPluginLoadReport {
    /// Returns true when the report contains a parsed manifest.
    pub fn loaded(&self) -> bool {
        self.manifest.is_some()
    }

    /// Returns true when at least one diagnostic is fatal.
    pub fn has_fatal_diagnostics(&self) -> bool {
        self.diagnostics.iter().any(|diagnostic| diagnostic.fatal)
    }
}

/// Parsed and normalized Open Plugin manifest.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPluginManifest {
    /// Unique plugin identifier used for namespacing.
    pub name: String,
    /// Optional version string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Optional short description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional author metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<OpenPluginAuthor>,
    /// Optional homepage URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    /// Optional source repository URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    /// Optional SPDX license identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Optional discovery keywords.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// Manifest-declared skill discovery paths.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<OpenPluginComponentPaths>,
    /// Manifest-declared MCP server discovery configuration.
    #[serde(
        default,
        rename = "mcpServers",
        skip_serializing_if = "Option::is_none"
    )]
    pub mcp_servers: Option<OpenPluginMcpServers>,
    /// Unsupported extended component fields that were ignored by this loader.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unsupported_components: Vec<OpenPluginUnsupportedComponent>,
}

/// Manifest author metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPluginAuthor {
    /// Author name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Author email.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Author URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Path field shape used by a manifest component declaration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OpenPluginComponentFieldSource {
    /// A single string path.
    String,
    /// An array of string paths.
    StringArray,
    /// An object with a `paths` array.
    PathConfig,
}

/// Normalized component paths declared by a manifest field.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPluginComponentPaths {
    /// Manifest value shape that produced these paths.
    pub source: OpenPluginComponentFieldSource,
    /// Valid, plugin-root-contained paths.
    pub paths: Vec<OpenPluginPath>,
}

/// A configured path plus its normalized absolute path.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPluginPath {
    /// Path exactly as declared in the manifest.
    pub declared: String,
    /// Absolute path after lexical normalization under the plugin root.
    pub normalized: PathBuf,
}

/// Normalized `mcpServers` manifest declaration.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OpenPluginMcpServers {
    /// Path-backed MCP server config files.
    Paths(OpenPluginComponentPaths),
    /// Inline MCP server config object.
    Inline {
        /// Server entries from the inline `mcpServers` object.
        servers: BTreeMap<String, Value>,
    },
}

/// Unsupported component declaration captured for diagnostics and docs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPluginUnsupportedComponent {
    /// Unsupported manifest field.
    pub component_type: String,
}

/// Diagnostic severity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenPluginDiagnosticLevel {
    /// Informational diagnostic.
    Info,
    /// Non-fatal warning.
    Warn,
    /// Fatal error.
    Error,
}

/// Machine-readable diagnostic record for manifest loading.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPluginDiagnostic {
    /// Diagnostic severity.
    pub level: OpenPluginDiagnosticLevel,
    /// Stable event identifier.
    pub event: String,
    /// Plugin name, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
    /// Manifest field, when relevant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Manifest or component path, when relevant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Action the host took in response.
    pub action: String,
    /// Human-readable diagnostic message.
    pub message: String,
    /// Whether this diagnostic prevented manifest loading.
    pub fatal: bool,
}

impl OpenPluginDiagnostic {
    fn info(
        event: impl Into<String>,
        plugin: Option<&str>,
        field: Option<&str>,
        path: Option<&str>,
        action: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(
            OpenPluginDiagnosticLevel::Info,
            event,
            plugin,
            field,
            path,
            action,
            message,
            false,
        )
    }

    fn warn(
        event: impl Into<String>,
        plugin: Option<&str>,
        field: Option<&str>,
        path: Option<&str>,
        action: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(
            OpenPluginDiagnosticLevel::Warn,
            event,
            plugin,
            field,
            path,
            action,
            message,
            false,
        )
    }

    fn error(
        event: impl Into<String>,
        plugin: Option<&str>,
        field: Option<&str>,
        path: Option<&str>,
        action: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(
            OpenPluginDiagnosticLevel::Error,
            event,
            plugin,
            field,
            path,
            action,
            message,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        level: OpenPluginDiagnosticLevel,
        event: impl Into<String>,
        plugin: Option<&str>,
        field: Option<&str>,
        path: Option<&str>,
        action: impl Into<String>,
        message: impl Into<String>,
        fatal: bool,
    ) -> Self {
        Self {
            level,
            event: event.into(),
            plugin: plugin.map(str::to_string),
            field: field.map(str::to_string),
            path: path.map(str::to_string),
            action: action.into(),
            message: message.into(),
            fatal,
        }
    }
}

/// Load `.plugin/plugin.json` from a plugin root.
pub fn load_open_plugin_manifest(plugin_root: impl AsRef<Path>) -> OpenPluginLoadReport {
    load_open_plugin_manifest_with_options(plugin_root, &OpenPluginLoadOptions::default())
}

/// Load an Open Plugin manifest with optional vendor-prefixed manifest support.
pub fn load_open_plugin_manifest_with_options(
    plugin_root: impl AsRef<Path>,
    options: &OpenPluginLoadOptions,
) -> OpenPluginLoadReport {
    let input_root = plugin_root.as_ref();
    let mut diagnostics = Vec::new();
    let plugin_root = match fs::canonicalize(input_root) {
        Ok(path) if path.is_dir() => path,
        Ok(path) => {
            diagnostics.push(OpenPluginDiagnostic::error(
                "open_plugin.root.not_directory",
                None,
                None,
                Some(&path.display().to_string()),
                "aborted",
                "Open Plugin root must be a directory.",
            ));
            return OpenPluginLoadReport {
                plugin_root: path,
                manifest_path: None,
                manifest: None,
                diagnostics,
            };
        }
        Err(error) => {
            diagnostics.push(OpenPluginDiagnostic::error(
                "open_plugin.root.missing",
                None,
                None,
                Some(&input_root.display().to_string()),
                "aborted",
                format!("Open Plugin root does not exist: {error}"),
            ));
            return OpenPluginLoadReport {
                plugin_root: input_root.to_path_buf(),
                manifest_path: None,
                manifest: None,
                diagnostics,
            };
        }
    };

    let neutral_path = plugin_root.join(OPEN_PLUGIN_MANIFEST_PATH);
    if !neutral_path.is_file() {
        diagnostics.push(OpenPluginDiagnostic::error(
            "open_plugin.manifest.missing",
            None,
            None,
            Some(OPEN_PLUGIN_MANIFEST_PATH),
            "aborted",
            "Open Plugin packages must include .plugin/plugin.json.",
        ));
        return OpenPluginLoadReport {
            plugin_root,
            manifest_path: Some(neutral_path),
            manifest: None,
            diagnostics,
        };
    }

    let Some(neutral_value) = read_manifest_json(&neutral_path, &mut diagnostics) else {
        return OpenPluginLoadReport {
            plugin_root,
            manifest_path: Some(neutral_path),
            manifest: None,
            diagnostics,
        };
    };

    let mut selected_path = neutral_path;
    let mut selected_value = neutral_value.clone();

    if let Some(vendor_manifest_dir) = &options.vendor_manifest_dir {
        if let Some((vendor_relative, vendor_path)) =
            vendor_manifest_path(&plugin_root, vendor_manifest_dir, &mut diagnostics)
        {
            if vendor_path.is_file() {
                let Some(vendor_value) = read_manifest_json(&vendor_path, &mut diagnostics) else {
                    return OpenPluginLoadReport {
                        plugin_root,
                        manifest_path: Some(vendor_path),
                        manifest: None,
                        diagnostics,
                    };
                };
                if vendor_value != neutral_value {
                    diagnostics.push(OpenPluginDiagnostic::warn(
                        "open_plugin.manifest.inconsistent",
                        manifest_name(&vendor_value).as_deref(),
                        None,
                        Some(&vendor_relative),
                        "used_selected",
                        format!(
                            "Vendor manifest {vendor_relative} differs from {OPEN_PLUGIN_MANIFEST_PATH}; using vendor manifest as authoritative."
                        ),
                    ));
                }
                selected_path = vendor_path;
                selected_value = vendor_value;
            }
        }
    }

    let manifest = parse_manifest(&plugin_root, &selected_value, &mut diagnostics);

    OpenPluginLoadReport {
        plugin_root,
        manifest_path: Some(selected_path),
        manifest,
        diagnostics,
    }
}

fn vendor_manifest_path(
    plugin_root: &Path,
    vendor_manifest_dir: &str,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Option<(String, PathBuf)> {
    let vendor_manifest_dir = vendor_manifest_dir.trim();
    let has_separator = vendor_manifest_dir.contains('/') || vendor_manifest_dir.contains('\\');
    let is_valid = !vendor_manifest_dir.is_empty()
        && vendor_manifest_dir.starts_with('.')
        && !has_separator
        && !vendor_manifest_dir.contains("..")
        && !Path::new(vendor_manifest_dir).is_absolute();

    if !is_valid {
        diagnostics.push(OpenPluginDiagnostic::warn(
            "open_plugin.manifest.invalid_vendor_location",
            None,
            None,
            Some(vendor_manifest_dir),
            "ignored",
            "Open Plugin vendor manifest directories must be single plugin-root-relative metadata directories.",
        ));
        return None;
    }

    let vendor_relative = format!("{vendor_manifest_dir}/plugin.json");
    Some((
        vendor_relative,
        plugin_root.join(vendor_manifest_dir).join("plugin.json"),
    ))
}

/// Returns whether a manifest name satisfies Open Plugin v1 constraints.
pub fn is_valid_open_plugin_name(name: &str) -> bool {
    plugin_name_violation(name).is_none()
}

/// Returns a component identifier in the recommended `{plugin}:{component}` form.
pub fn open_plugin_component_identifier(plugin_name: &str, component_name: &str) -> Option<String> {
    if !is_valid_open_plugin_name(plugin_name) || component_name.is_empty() {
        return None;
    }
    Some(format!("{plugin_name}:{component_name}"))
}

/// Returns an MCP tool identifier using the recommended plugin/server namespace.
pub fn open_plugin_mcp_tool_identifier(
    plugin_name: &str,
    server_name: &str,
    tool_name: &str,
) -> Option<String> {
    if !is_valid_open_plugin_name(plugin_name) || server_name.is_empty() || tool_name.is_empty() {
        return None;
    }
    Some(format!(
        "mcp__plugin_{plugin_name}_{server_name}__{tool_name}"
    ))
}

fn read_manifest_json(
    manifest_path: &Path,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Option<Value> {
    let source = match fs::read_to_string(manifest_path) {
        Ok(source) => source,
        Err(error) => {
            diagnostics.push(OpenPluginDiagnostic::error(
                "open_plugin.manifest.read_failed",
                None,
                None,
                Some(&manifest_path.display().to_string()),
                "aborted",
                format!("Failed to read Open Plugin manifest: {error}"),
            ));
            return None;
        }
    };

    match serde_json::from_str(&source) {
        Ok(value) => Some(value),
        Err(error) => {
            diagnostics.push(OpenPluginDiagnostic::error(
                "open_plugin.manifest.invalid_json",
                None,
                None,
                Some(&manifest_path.display().to_string()),
                "aborted",
                format!("Open Plugin manifest must be valid JSON: {error}"),
            ));
            None
        }
    }
}

fn parse_manifest(
    plugin_root: &Path,
    value: &Value,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Option<OpenPluginManifest> {
    let Some(object) = value.as_object() else {
        diagnostics.push(OpenPluginDiagnostic::error(
            "open_plugin.manifest.invalid_type",
            None,
            None,
            None,
            "aborted",
            "Open Plugin manifest must be a top-level object.",
        ));
        return None;
    };

    let name = required_name(object, diagnostics)?;
    if let Some(message) = plugin_name_violation(&name) {
        diagnostics.push(OpenPluginDiagnostic::error(
            "open_plugin.manifest.invalid_name",
            Some(&name),
            Some("name"),
            None,
            "aborted",
            message,
        ));
        return None;
    }

    let version = optional_string_field(object, &name, "version", diagnostics);
    let description = optional_string_field(object, &name, "description", diagnostics);
    let author = optional_author(object, &name, diagnostics);
    let homepage = optional_string_field(object, &name, "homepage", diagnostics);
    let repository = optional_string_field(object, &name, "repository", diagnostics);
    let license = optional_string_field(object, &name, "license", diagnostics);
    let keywords = optional_string_array_field(object, &name, "keywords", diagnostics);
    let skills = object.get("skills").and_then(|value| {
        parse_component_paths(plugin_root, &name, "skills", value, false, diagnostics)
    });
    let mcp_servers = object.get(MCP_SERVERS_FIELD).and_then(|value| {
        parse_mcp_servers(plugin_root, &name, MCP_SERVERS_FIELD, value, diagnostics)
    });
    let unsupported_components = unsupported_component_diagnostics(object, &name, diagnostics);

    Some(OpenPluginManifest {
        name,
        version,
        description,
        author,
        homepage,
        repository,
        license,
        keywords,
        skills,
        mcp_servers,
        unsupported_components,
    })
}

fn required_name(
    object: &Map<String, Value>,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Option<String> {
    match object.get("name") {
        Some(Value::String(name)) => Some(name.clone()),
        Some(_) => {
            diagnostics.push(OpenPluginDiagnostic::error(
                "open_plugin.manifest.invalid_field",
                None,
                Some("name"),
                None,
                "aborted",
                "Open Plugin manifest field \"name\" must be a string.",
            ));
            None
        }
        None => {
            diagnostics.push(OpenPluginDiagnostic::error(
                "open_plugin.manifest.missing_name",
                None,
                Some("name"),
                None,
                "aborted",
                "Open Plugin manifest must contain a name field.",
            ));
            None
        }
    }
}

fn optional_string_field(
    object: &Map<String, Value>,
    plugin_name: &str,
    field: &'static str,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Option<String> {
    match object.get(field) {
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            diagnostics.push(OpenPluginDiagnostic::warn(
                "open_plugin.manifest.invalid_field",
                Some(plugin_name),
                Some(field),
                None,
                "ignored",
                format!("Open Plugin manifest field \"{field}\" must be a string."),
            ));
            None
        }
        None => None,
    }
}

fn optional_author(
    object: &Map<String, Value>,
    plugin_name: &str,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Option<OpenPluginAuthor> {
    let value = object.get("author")?;
    let Some(author) = value.as_object() else {
        diagnostics.push(OpenPluginDiagnostic::warn(
            "open_plugin.manifest.invalid_field",
            Some(plugin_name),
            Some("author"),
            None,
            "ignored",
            "Open Plugin manifest field \"author\" must be an object.",
        ));
        return None;
    };

    Some(OpenPluginAuthor {
        name: optional_author_string(author, plugin_name, "author.name", "name", diagnostics),
        email: optional_author_string(author, plugin_name, "author.email", "email", diagnostics),
        url: optional_author_string(author, plugin_name, "author.url", "url", diagnostics),
    })
}

fn optional_author_string(
    author: &Map<String, Value>,
    plugin_name: &str,
    diagnostic_field: &'static str,
    field: &'static str,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Option<String> {
    match author.get(field) {
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            diagnostics.push(OpenPluginDiagnostic::warn(
                "open_plugin.manifest.invalid_field",
                Some(plugin_name),
                Some(diagnostic_field),
                None,
                "ignored",
                format!("Open Plugin manifest field \"{diagnostic_field}\" must be a string."),
            ));
            None
        }
        None => None,
    }
}

fn optional_string_array_field(
    object: &Map<String, Value>,
    plugin_name: &str,
    field: &'static str,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Vec<String> {
    let Some(value) = object.get(field) else {
        return Vec::new();
    };
    let Some(values) = value.as_array() else {
        diagnostics.push(OpenPluginDiagnostic::warn(
            "open_plugin.manifest.invalid_field",
            Some(plugin_name),
            Some(field),
            None,
            "ignored",
            format!("Open Plugin manifest field \"{field}\" must be a string array."),
        ));
        return Vec::new();
    };

    values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| match value {
            Value::String(value) => Some(value.clone()),
            _ => {
                diagnostics.push(OpenPluginDiagnostic::warn(
                    "open_plugin.manifest.invalid_field",
                    Some(plugin_name),
                    Some(&format!("{field}[{index}]")),
                    None,
                    "ignored",
                    format!("Open Plugin manifest field \"{field}\" entries must be strings."),
                ));
                None
            }
        })
        .collect()
}

fn parse_component_paths(
    plugin_root: &Path,
    plugin_name: &str,
    field: &'static str,
    value: &Value,
    mcp_paths: bool,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Option<OpenPluginComponentPaths> {
    match value {
        Value::String(path) => component_paths_from_entries(
            plugin_root,
            plugin_name,
            field,
            OpenPluginComponentFieldSource::String,
            [path.as_str()],
            mcp_paths,
            diagnostics,
        ),
        Value::Array(paths) => {
            let declared_paths = collect_string_entries(plugin_name, field, paths, diagnostics);
            component_paths_from_entries(
                plugin_root,
                plugin_name,
                field,
                OpenPluginComponentFieldSource::StringArray,
                declared_paths.iter().map(String::as_str),
                mcp_paths,
                diagnostics,
            )
        }
        Value::Object(object) => {
            let Some(paths) = object.get(PATHS_FIELD) else {
                diagnostics.push(OpenPluginDiagnostic::warn(
                    "open_plugin.manifest.invalid_object",
                    Some(plugin_name),
                    Some(field),
                    None,
                    "ignored",
                    format!("Manifest field \"{field}\" object must contain a \"paths\" array."),
                ));
                return None;
            };
            let Some(paths) = paths.as_array() else {
                diagnostics.push(OpenPluginDiagnostic::warn(
                    "open_plugin.manifest.invalid_field",
                    Some(plugin_name),
                    Some(&format!("{field}.paths")),
                    None,
                    "ignored",
                    format!("Manifest field \"{field}.paths\" must be a string array."),
                ));
                return None;
            };
            let declared_paths =
                collect_string_entries(plugin_name, &format!("{field}.paths"), paths, diagnostics);
            component_paths_from_entries(
                plugin_root,
                plugin_name,
                field,
                OpenPluginComponentFieldSource::PathConfig,
                declared_paths.iter().map(String::as_str),
                mcp_paths,
                diagnostics,
            )
        }
        _ => {
            diagnostics.push(OpenPluginDiagnostic::warn(
                "open_plugin.manifest.invalid_field",
                Some(plugin_name),
                Some(field),
                None,
                "ignored",
                format!(
                    "Manifest field \"{field}\" must be a string, string array, or path config object."
                ),
            ));
            None
        }
    }
}

fn parse_mcp_servers(
    plugin_root: &Path,
    plugin_name: &str,
    field: &'static str,
    value: &Value,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Option<OpenPluginMcpServers> {
    match value {
        Value::Object(object) => {
            let has_paths = object.contains_key(PATHS_FIELD);
            let has_inline = object.contains_key(MCP_SERVERS_FIELD);

            match (has_paths, has_inline) {
                (true, true) => {
                    diagnostics.push(OpenPluginDiagnostic::warn(
                        "open_plugin.manifest.invalid_object",
                        Some(plugin_name),
                        Some(field),
                        None,
                        "ignored",
                        "Manifest field \"mcpServers\" cannot be both a path config and an inline MCP config.",
                    ));
                    None
                }
                (true, false) => {
                    parse_component_paths(plugin_root, plugin_name, field, value, true, diagnostics)
                        .map(OpenPluginMcpServers::Paths)
                }
                (false, true) => {
                    let Some(Value::Object(servers)) = object.get(MCP_SERVERS_FIELD) else {
                        diagnostics.push(OpenPluginDiagnostic::warn(
                            "open_plugin.manifest.invalid_object",
                            Some(plugin_name),
                            Some(field),
                            None,
                            "ignored",
                            "Inline MCP config must contain a top-level \"mcpServers\" object.",
                        ));
                        return None;
                    };
                    Some(OpenPluginMcpServers::Inline {
                        servers: servers
                            .iter()
                            .map(|(name, value)| (name.clone(), value.clone()))
                            .collect(),
                    })
                }
                (false, false) => {
                    diagnostics.push(OpenPluginDiagnostic::warn(
                        "open_plugin.manifest.invalid_object",
                        Some(plugin_name),
                        Some(field),
                        None,
                        "ignored",
                        "Manifest field \"mcpServers\" object must contain either \"paths\" or a nested \"mcpServers\" object.",
                    ));
                    None
                }
            }
        }
        _ => parse_component_paths(plugin_root, plugin_name, field, value, true, diagnostics)
            .map(OpenPluginMcpServers::Paths),
    }
}

fn collect_string_entries(
    plugin_name: &str,
    field: &str,
    values: &[Value],
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Vec<String> {
    values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| match value {
            Value::String(path) => Some(path.clone()),
            _ => {
                diagnostics.push(OpenPluginDiagnostic::warn(
                    "open_plugin.manifest.invalid_field",
                    Some(plugin_name),
                    Some(&format!("{field}[{index}]")),
                    None,
                    "ignored",
                    format!("Manifest field \"{field}\" entries must be strings."),
                ));
                None
            }
        })
        .collect()
}

fn component_paths_from_entries<'a>(
    plugin_root: &Path,
    plugin_name: &str,
    field: &'static str,
    source: OpenPluginComponentFieldSource,
    entries: impl IntoIterator<Item = &'a str>,
    mcp_paths: bool,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Option<OpenPluginComponentPaths> {
    let paths: Vec<_> = entries
        .into_iter()
        .filter_map(|declared| {
            resolve_plugin_relative_path(
                plugin_root,
                plugin_name,
                field,
                declared,
                mcp_paths,
                diagnostics,
            )
        })
        .collect();

    if paths.is_empty() {
        None
    } else {
        Some(OpenPluginComponentPaths { source, paths })
    }
}

fn resolve_plugin_relative_path(
    plugin_root: &Path,
    plugin_name: &str,
    field: &'static str,
    declared: &str,
    mcp_path: bool,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Option<OpenPluginPath> {
    if !declared.starts_with(PLUGIN_RELATIVE_PREFIX) {
        diagnostics.push(OpenPluginDiagnostic::warn(
            "open_plugin.path.invalid",
            Some(plugin_name),
            Some(field),
            Some(declared),
            "ignored",
            "Open Plugin relative paths must start with ./.",
        ));
        return None;
    }

    let mut relative = PathBuf::new();
    for component in Path::new(declared).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => relative.push(part),
            Component::ParentDir => {
                diagnostics.push(OpenPluginDiagnostic::warn(
                    "open_plugin.path.escape",
                    Some(plugin_name),
                    Some(field),
                    Some(declared),
                    "ignored",
                    "Open Plugin paths must not contain ../ traversal.",
                ));
                return None;
            }
            Component::Prefix(_) | Component::RootDir => {
                diagnostics.push(OpenPluginDiagnostic::warn(
                    "open_plugin.path.invalid",
                    Some(plugin_name),
                    Some(field),
                    Some(declared),
                    "ignored",
                    "Open Plugin paths must be relative to the plugin root.",
                ));
                return None;
            }
        }
    }

    let normalized = plugin_root.join(relative);
    if !normalized.starts_with(plugin_root) {
        diagnostics.push(OpenPluginDiagnostic::warn(
            "open_plugin.path.escape",
            Some(plugin_name),
            Some(field),
            Some(declared),
            "ignored",
            "Open Plugin paths must stay inside the plugin root after normalization.",
        ));
        return None;
    }

    if mcp_path
        && normalized
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("json")
    {
        diagnostics.push(OpenPluginDiagnostic::warn(
            "open_plugin.mcp.invalid_path",
            Some(plugin_name),
            Some(field),
            Some(declared),
            "ignored",
            "Manifest-declared MCP server paths must point to explicit JSON files.",
        ));
        return None;
    }

    Some(OpenPluginPath {
        declared: declared.to_string(),
        normalized,
    })
}

fn unsupported_component_diagnostics(
    object: &Map<String, Value>,
    plugin_name: &str,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Vec<OpenPluginUnsupportedComponent> {
    EXTENDED_COMPONENT_FIELDS
        .iter()
        .filter(|field| object.contains_key(**field))
        .map(|field| {
            diagnostics.push(OpenPluginDiagnostic::info(
                "open_plugin.host.unsupported_component",
                Some(plugin_name),
                Some(field),
                None,
                "ignored",
                format!(
                    "Open Plugin component type \"{field}\" is not supported by this host surface."
                ),
            ));
            OpenPluginUnsupportedComponent {
                component_type: (*field).to_string(),
            }
        })
        .collect()
}

fn manifest_name(value: &Value) -> Option<String> {
    value
        .as_object()
        .and_then(|object| object.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn plugin_name_violation(name: &str) -> Option<String> {
    if name.is_empty() || name.len() > 64 {
        return Some("Open Plugin name length must be between 1 and 64 characters.".to_string());
    }

    if !name.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.'
    }) {
        return Some(
            "Open Plugin name must contain only lowercase alphanumeric characters, hyphens, and periods."
                .to_string(),
        );
    }

    let first = name.as_bytes()[0];
    let last = name.as_bytes()[name.len() - 1];
    if !first.is_ascii_alphanumeric() || !last.is_ascii_alphanumeric() {
        return Some(
            "Open Plugin name must start and end with an alphanumeric character.".to_string(),
        );
    }

    if name.contains("--") || name.contains("..") {
        return Some(
            "Open Plugin name must not contain consecutive hyphens or periods.".to_string(),
        );
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempPluginRoot {
        path: PathBuf,
    }

    impl TempPluginRoot {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock is after unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "open-agents-core-open-plugin-{name}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(path.join(".plugin")).expect("create plugin metadata directory");
            Self { path }
        }

        fn write_manifest(&self, value: Value) {
            fs::write(
                self.path.join(OPEN_PLUGIN_MANIFEST_PATH),
                serde_json::to_vec_pretty(&value).expect("manifest serializes"),
            )
            .expect("write plugin manifest");
        }

        fn write_vendor_manifest(&self, directory: &str, value: Value) {
            let manifest_dir = self.path.join(directory);
            fs::create_dir_all(&manifest_dir).expect("create vendor manifest directory");
            fs::write(
                manifest_dir.join("plugin.json"),
                serde_json::to_vec_pretty(&value).expect("vendor manifest serializes"),
            )
            .expect("write vendor plugin manifest");
        }
    }

    impl Drop for TempPluginRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn diagnostic_events(report: &OpenPluginLoadReport) -> BTreeSet<&str> {
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.event.as_str())
            .collect()
    }

    #[test]
    fn open_plugin_manifest_loads_metadata_and_core_component_paths() {
        let root = TempPluginRoot::new("metadata");
        root.write_manifest(json!({
            "name": "devtools",
            "version": "1.2.0",
            "description": "Developer tools.",
            "author": {
                "name": "Open Plugin Examples",
                "email": "plugins@example.com",
                "url": "https://example.com"
            },
            "homepage": "https://docs.example.com/plugin",
            "repository": "https://github.com/example/plugin",
            "license": "Apache-2.0",
            "keywords": ["review", "security"],
            "skills": {
                "paths": ["./skills/", "./extra-skills/"]
            },
            "mcpServers": "./.mcp.json"
        }));

        let report = load_open_plugin_manifest(&root.path);
        let manifest = report.manifest.as_ref().expect("manifest loads");

        assert_eq!(manifest.name, "devtools");
        assert_eq!(manifest.version.as_deref(), Some("1.2.0"));
        assert_eq!(manifest.description.as_deref(), Some("Developer tools."));
        assert_eq!(
            manifest
                .author
                .as_ref()
                .and_then(|author| author.name.as_deref()),
            Some("Open Plugin Examples")
        );
        assert_eq!(manifest.keywords, ["review", "security"]);
        assert_eq!(
            manifest.skills.as_ref().expect("skills paths").paths,
            vec![
                OpenPluginPath {
                    declared: "./skills/".to_string(),
                    normalized: fs::canonicalize(&root.path)
                        .expect("canonical root")
                        .join("skills")
                },
                OpenPluginPath {
                    declared: "./extra-skills/".to_string(),
                    normalized: fs::canonicalize(&root.path)
                        .expect("canonical root")
                        .join("extra-skills")
                },
            ]
        );
        assert!(matches!(
            manifest.mcp_servers.as_ref(),
            Some(OpenPluginMcpServers::Paths(OpenPluginComponentPaths {
                source: OpenPluginComponentFieldSource::String,
                ..
            }))
        ));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn open_plugin_manifest_accepts_string_array_component_paths() {
        let root = TempPluginRoot::new("arrays");
        root.write_manifest(json!({
            "name": "array-paths",
            "skills": ["./skills/", "./review/"],
            "mcpServers": ["./.mcp.json", "./config/extra-mcp.json"]
        }));

        let report = load_open_plugin_manifest(&root.path);
        let manifest = report.manifest.as_ref().expect("manifest loads");

        assert_eq!(
            manifest.skills.as_ref().expect("skills paths").source,
            OpenPluginComponentFieldSource::StringArray
        );
        assert!(matches!(
            manifest.mcp_servers.as_ref(),
            Some(OpenPluginMcpServers::Paths(OpenPluginComponentPaths {
                source: OpenPluginComponentFieldSource::StringArray,
                ..
            }))
        ));
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn open_plugin_manifest_supports_inline_mcp_servers() {
        let root = TempPluginRoot::new("inline-mcp");
        root.write_manifest(json!({
            "name": "inline-mcp",
            "mcpServers": {
                "mcpServers": {
                    "database": {
                        "command": "npx",
                        "args": ["-y", "@modelcontextprotocol/server-postgres"]
                    }
                }
            }
        }));

        let report = load_open_plugin_manifest(&root.path);
        let manifest = report.manifest.as_ref().expect("manifest loads");

        match manifest.mcp_servers.as_ref().expect("mcp servers") {
            OpenPluginMcpServers::Inline { servers } => {
                assert!(servers.contains_key("database"));
            }
            OpenPluginMcpServers::Paths(_) => panic!("expected inline MCP config"),
        }
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn open_plugin_path_safety_rejects_missing_prefix_traversal_and_mcp_directories() {
        let root = TempPluginRoot::new("path-safety");
        root.write_manifest(json!({
            "name": "path-safety",
            "skills": ["skills/", "./../outside", "./safe/"],
            "mcpServers": ["./mcp/", "./config/server.json"]
        }));

        let report = load_open_plugin_manifest(&root.path);
        let manifest = report
            .manifest
            .as_ref()
            .expect("manifest loads despite invalid paths");
        let events = diagnostic_events(&report);

        assert!(events.contains("open_plugin.path.invalid"));
        assert!(events.contains("open_plugin.path.escape"));
        assert!(events.contains("open_plugin.mcp.invalid_path"));
        assert_eq!(
            manifest.skills.as_ref().expect("valid skills").paths.len(),
            1
        );
        assert!(matches!(
            manifest.mcp_servers.as_ref(),
            Some(OpenPluginMcpServers::Paths(OpenPluginComponentPaths { paths, .. }))
                if paths.len() == 1 && paths[0].declared == "./config/server.json"
        ));
    }

    #[test]
    fn open_plugin_name_validation_enforces_spec_constraints() {
        for valid in ["my-plugin", "acme.tools", "lint3r", "a"] {
            assert!(is_valid_open_plugin_name(valid), "{valid} should be valid");
        }

        for invalid in [
            "",
            "My-Plugin",
            "-start",
            "end-",
            ".start",
            "end.",
            "has--double",
            "too.many..dots",
            "name_with_underscore",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            assert!(
                !is_valid_open_plugin_name(invalid),
                "{invalid} should be invalid"
            );
        }
    }

    #[test]
    fn open_plugin_invalid_mcp_object_is_diagnostic_and_non_fatal() {
        let root = TempPluginRoot::new("invalid-mcp");
        root.write_manifest(json!({
            "name": "invalid-mcp",
            "skills": "./skills/",
            "mcpServers": {
                "database": {
                    "command": "npx"
                }
            }
        }));

        let report = load_open_plugin_manifest(&root.path);
        let manifest = report.manifest.as_ref().expect("manifest load continues");

        assert!(manifest.mcp_servers.is_none());
        assert!(diagnostic_events(&report).contains("open_plugin.manifest.invalid_object"));
        assert!(!report.has_fatal_diagnostics());
    }

    #[test]
    fn open_plugin_ambiguous_mcp_object_is_diagnostic_and_ignored() {
        let root = TempPluginRoot::new("ambiguous-mcp");
        root.write_manifest(json!({
            "name": "ambiguous-mcp",
            "mcpServers": {
                "paths": ["./.mcp.json"],
                "mcpServers": {
                    "database": {
                        "command": "npx"
                    }
                }
            }
        }));

        let report = load_open_plugin_manifest(&root.path);
        let manifest = report.manifest.as_ref().expect("manifest load continues");

        assert!(manifest.mcp_servers.is_none());
        assert!(diagnostic_events(&report).contains("open_plugin.manifest.invalid_object"));
        assert!(!report.has_fatal_diagnostics());
    }

    #[test]
    fn open_plugin_unsupported_component_types_are_diagnostic_not_fatal() {
        let root = TempPluginRoot::new("unsupported");
        root.write_manifest(json!({
            "name": "unsupported",
            "commands": "./commands/",
            "agents": "./agents/",
            "skills": "./skills/"
        }));

        let report = load_open_plugin_manifest(&root.path);
        let manifest = report.manifest.as_ref().expect("manifest loads");

        assert_eq!(
            manifest
                .unsupported_components
                .iter()
                .map(|component| component.component_type.as_str())
                .collect::<Vec<_>>(),
            ["commands", "agents"]
        );
        assert!(diagnostic_events(&report).contains("open_plugin.host.unsupported_component"));
        assert!(!report.has_fatal_diagnostics());
    }

    #[test]
    fn open_plugin_invalid_metadata_fields_warn_and_load() {
        let root = TempPluginRoot::new("metadata-warnings");
        root.write_manifest(json!({
            "name": "metadata-warnings",
            "version": 1,
            "author": {
                "name": ["not", "a", "string"]
            },
            "keywords": ["ok", 3],
            "skills": "./skills/"
        }));

        let report = load_open_plugin_manifest(&root.path);
        let manifest = report.manifest.as_ref().expect("manifest loads");

        assert!(manifest.version.is_none());
        assert_eq!(manifest.keywords, ["ok"]);
        assert!(diagnostic_events(&report).contains("open_plugin.manifest.invalid_field"));
        assert!(!report.has_fatal_diagnostics());
    }

    #[test]
    fn open_plugin_vendor_manifest_inconsistency_warns_and_vendor_wins() {
        let root = TempPluginRoot::new("vendor");
        root.write_manifest(json!({
            "name": "neutral-plugin",
            "skills": "./skills/"
        }));
        root.write_vendor_manifest(
            ".codex-plugin",
            json!({
                "name": "vendor-plugin",
                "skills": "./vendor-skills/"
            }),
        );

        let report = load_open_plugin_manifest_with_options(
            &root.path,
            &OpenPluginLoadOptions::new().with_vendor_prefix("codex"),
        );
        let manifest = report.manifest.as_ref().expect("manifest loads");

        assert_eq!(manifest.name, "vendor-plugin");
        assert!(
            report
                .manifest_path
                .as_ref()
                .expect("selected manifest path")
                .ends_with(".codex-plugin/plugin.json")
        );
        assert!(diagnostic_events(&report).contains("open_plugin.manifest.inconsistent"));
        assert!(!report.has_fatal_diagnostics());
    }

    #[test]
    fn open_plugin_invalid_vendor_manifest_location_is_ignored() {
        let root = TempPluginRoot::new("vendor-escape");
        root.write_manifest(json!({
            "name": "neutral-plugin",
            "skills": "./skills/"
        }));

        let report = load_open_plugin_manifest_with_options(
            &root.path,
            &OpenPluginLoadOptions::new().with_vendor_manifest_dir("../outside"),
        );
        let manifest = report.manifest.as_ref().expect("neutral manifest loads");

        assert_eq!(manifest.name, "neutral-plugin");
        assert!(
            diagnostic_events(&report).contains("open_plugin.manifest.invalid_vendor_location")
        );
        assert!(!report.has_fatal_diagnostics());
    }

    #[test]
    fn open_plugin_missing_manifest_is_fatal() {
        let root = TempPluginRoot::new("missing-manifest");

        let report = load_open_plugin_manifest(&root.path);

        assert!(!report.loaded());
        assert!(report.has_fatal_diagnostics());
        assert!(diagnostic_events(&report).contains("open_plugin.manifest.missing"));
    }

    #[test]
    fn open_plugin_namespacing_identifiers_match_recommended_format() {
        assert_eq!(
            open_plugin_component_identifier("devtools", "deploy").as_deref(),
            Some("devtools:deploy")
        );
        assert_eq!(
            open_plugin_mcp_tool_identifier("devtools", "database", "query").as_deref(),
            Some("mcp__plugin_devtools_database__query")
        );
        assert!(open_plugin_component_identifier("BadName", "deploy").is_none());
    }

    #[test]
    fn open_plugin_spec_snapshot_records_verified_remote_head() {
        let snapshot = OpenPluginSpecSourceSnapshot::default();

        assert_eq!(snapshot.spec_version, "1.0.0");
        assert_eq!(snapshot.repository, OPEN_PLUGIN_SPEC_SOURCE_REPOSITORY);
        assert_eq!(snapshot.ref_name, "main");
        assert_eq!(snapshot.head, OPEN_PLUGIN_SPEC_SOURCE_HEAD);
        assert_eq!(snapshot.head.len(), 40);
    }
}
