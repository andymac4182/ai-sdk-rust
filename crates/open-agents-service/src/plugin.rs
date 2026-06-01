use std::collections::BTreeSet;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use ai_sdk_rust::{
    AgentSkillContext, AgentSkillMetadata, AgentSkillOptions, parse_agent_skill_metadata,
};
use open_agents_runtime::{
    OpenAgentPluginMcpServer, OpenAgentSkillMetadata, OpenAgentSkillOptions,
};
use serde_json::{Map as JsonMap, Value as JsonValue};

/// Environment variable containing Open Plugin package roots.
pub const OPEN_AGENTS_PLUGIN_ROOTS_ENV: &str = "OPEN_AGENTS_PLUGIN_ROOTS";

/// Optional environment variable containing a host-managed plugin data root.
pub const OPEN_AGENTS_PLUGIN_DATA_DIR_ENV: &str = "OPEN_AGENTS_PLUGIN_DATA_DIR";

const OPEN_PLUGIN_MANIFEST_PATH: &str = ".plugin/plugin.json";

/// Loaded Open Plugin package catalog for the service.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct OpenPluginCatalog {
    roots: Vec<PathBuf>,
    data_dir: Option<PathBuf>,
    packages: Vec<OpenPluginPackage>,
    diagnostics: Vec<OpenPluginDiagnostic>,
}

impl OpenPluginCatalog {
    /// Load plugin roots from raw environment variable values.
    pub fn from_env_values(
        roots: Option<String>,
        data_dir: Option<String>,
    ) -> Result<Self, OpenPluginError> {
        let roots = parse_plugin_roots(roots.as_deref())?;
        let data_dir = data_dir.map(PathBuf::from).map(absolutize);
        Self::load(roots, data_dir)
    }

    /// Load plugins from explicit package roots.
    pub fn load(roots: Vec<PathBuf>, data_dir: Option<PathBuf>) -> Result<Self, OpenPluginError> {
        let mut catalog = Self {
            roots: Vec::new(),
            data_dir: data_dir.map(absolutize),
            packages: Vec::new(),
            diagnostics: Vec::new(),
        };

        for root in roots {
            let root = canonical_plugin_root(&root)?;
            let data_dir = catalog.data_dir.clone();
            let package = load_plugin_package(&root, data_dir.as_deref(), &mut catalog)?;
            catalog.roots.push(root);
            catalog.packages.push(package);
        }

        Ok(catalog)
    }

    /// Configured plugin package roots.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Optional host-managed plugin data directory.
    pub fn data_dir(&self) -> Option<&Path> {
        self.data_dir.as_deref()
    }

    /// Loaded plugin packages.
    pub fn packages(&self) -> &[OpenPluginPackage] {
        &self.packages
    }

    /// Non-fatal loader diagnostics.
    pub fn diagnostics(&self) -> &[OpenPluginDiagnostic] {
        &self.diagnostics
    }

    /// Namespaced plugin skills in runtime metadata form.
    pub fn runtime_skills(&self) -> Vec<OpenAgentSkillMetadata> {
        self.packages
            .iter()
            .flat_map(|package| package.skills.iter().map(|skill| skill.runtime.clone()))
            .collect()
    }

    /// Sanitized plugin MCP server planning surfaces.
    pub fn runtime_mcp_servers(&self) -> Vec<OpenAgentPluginMcpServer> {
        self.packages
            .iter()
            .flat_map(|package| {
                package
                    .mcp_servers
                    .iter()
                    .map(OpenPluginMcpServer::runtime_surface)
            })
            .collect()
    }
}

impl fmt::Debug for OpenPluginCatalog {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenPluginCatalog")
            .field("roots", &self.roots)
            .field("data_dir", &self.data_dir)
            .field("packages", &self.packages.len())
            .field("diagnostics", &self.diagnostics)
            .finish()
    }
}

/// One loaded Open Plugin package.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenPluginPackage {
    /// Manifest name used for namespacing.
    pub name: String,
    /// Canonical plugin root.
    pub root: PathBuf,
    /// Manifest path used for loading.
    pub manifest_path: PathBuf,
    /// Optional manifest version.
    pub version: Option<String>,
    /// Optional manifest description.
    pub description: Option<String>,
    /// Discovered plugin skills.
    pub skills: Vec<OpenPluginSkill>,
    /// Discovered plugin MCP server configs.
    pub mcp_servers: Vec<OpenPluginMcpServer>,
}

/// One namespaced plugin skill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenPluginSkill {
    /// Package name.
    pub plugin_name: String,
    /// Namespaced skill name surfaced to the runtime.
    pub name: String,
    /// Original skill frontmatter name.
    pub source_name: String,
    /// Skill directory.
    pub path: PathBuf,
    /// Skill markdown filename.
    pub filename: String,
    /// Runtime-facing skill metadata.
    pub runtime: OpenAgentSkillMetadata,
}

/// One discovered plugin MCP server config.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenPluginMcpServer {
    /// Package name.
    pub plugin_name: String,
    /// Server name in the MCP config.
    pub server_name: String,
    /// Discovery source label.
    pub source: String,
    /// Expanded command, when present.
    pub command: Option<String>,
    /// Expanded argv entries. Kept out of the runtime prompt.
    pub args: Vec<String>,
    /// Expanded cwd, when present.
    pub cwd: Option<String>,
    /// Configured environment variable names. Values are not retained.
    pub env_keys: Vec<String>,
}

impl OpenPluginMcpServer {
    fn runtime_surface(&self) -> OpenAgentPluginMcpServer {
        let mut surface = OpenAgentPluginMcpServer::new(
            self.plugin_name.clone(),
            self.server_name.clone(),
            self.source.clone(),
        )
        .with_env_keys(self.env_keys.clone())
        .with_has_args(!self.args.is_empty());
        if let Some(command) = &self.command {
            surface = surface.with_command(command.clone());
        }
        if let Some(cwd) = &self.cwd {
            surface = surface.with_cwd(cwd.clone());
        }
        surface
    }
}

/// Non-fatal Open Plugin loader diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenPluginDiagnostic {
    /// Diagnostic severity.
    pub level: OpenPluginDiagnosticLevel,
    /// Stable event identifier.
    pub event: &'static str,
    /// Plugin name if known.
    pub plugin: Option<String>,
    /// Related path if available.
    pub path: Option<PathBuf>,
    /// Human-readable diagnostic.
    pub message: String,
}

impl OpenPluginDiagnostic {
    fn warn(
        event: &'static str,
        plugin: impl Into<Option<String>>,
        path: impl Into<Option<PathBuf>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            level: OpenPluginDiagnosticLevel::Warn,
            event,
            plugin: plugin.into(),
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for OpenPluginDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.level {
            OpenPluginDiagnosticLevel::Warn => "WARN",
        };
        write!(formatter, "{level} {}", self.event)?;
        if let Some(plugin) = &self.plugin {
            write!(formatter, " plugin={plugin}")?;
        }
        if let Some(path) = &self.path {
            write!(formatter, " path={}", path.display())?;
        }
        write!(formatter, ": {}", self.message)
    }
}

/// Open Plugin diagnostic severity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenPluginDiagnosticLevel {
    /// Loader warning; the package continues with remaining components.
    Warn,
}

/// Fatal Open Plugin configuration error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenPluginError {
    path: Option<PathBuf>,
    message: String,
}

impl OpenPluginError {
    fn new(path: impl Into<Option<PathBuf>>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for OpenPluginError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(path) => write!(formatter, "{}: {}", path.display(), self.message),
            None => formatter.write_str(&self.message),
        }
    }
}

impl std::error::Error for OpenPluginError {}

#[derive(Clone, Debug)]
struct ResolvedPluginPath {
    raw: String,
    absolute: PathBuf,
    is_default: bool,
}

#[derive(Clone, Debug)]
enum McpSource {
    Path(ResolvedPluginPath),
    Inline(JsonValue),
}

struct McpCollectContext<'a> {
    root: &'a Path,
    plugin_name: &'a str,
    plugin_data_dir: Option<&'a Path>,
    seen: &'a mut BTreeSet<String>,
    diagnostics: &'a mut Vec<OpenPluginDiagnostic>,
    servers: &'a mut Vec<OpenPluginMcpServer>,
}

fn parse_plugin_roots(raw: Option<&str>) -> Result<Vec<PathBuf>, OpenPluginError> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(Vec::new());
    };

    let roots = env::split_paths(raw).collect::<Vec<_>>();
    if roots.iter().any(|root| root.as_os_str().is_empty()) {
        return Err(OpenPluginError::new(
            None,
            format!("{OPEN_AGENTS_PLUGIN_ROOTS_ENV} contains an empty path"),
        ));
    }
    Ok(roots)
}

fn canonical_plugin_root(root: &Path) -> Result<PathBuf, OpenPluginError> {
    let canonical = fs::canonicalize(root).map_err(|error| {
        OpenPluginError::new(
            Some(root.to_path_buf()),
            format!("failed to read plugin root: {error}"),
        )
    })?;
    if !canonical.is_dir() {
        return Err(OpenPluginError::new(
            Some(canonical),
            "plugin root must be a directory",
        ));
    }
    Ok(canonical)
}

fn load_plugin_package(
    root: &Path,
    data_dir: Option<&Path>,
    catalog: &mut OpenPluginCatalog,
) -> Result<OpenPluginPackage, OpenPluginError> {
    let manifest_path = root.join(OPEN_PLUGIN_MANIFEST_PATH);
    let manifest = read_json_file(&manifest_path)?;
    let manifest_object = manifest.as_object().ok_or_else(|| {
        OpenPluginError::new(
            Some(manifest_path.clone()),
            "Open Plugin manifest must be a JSON object",
        )
    })?;
    let name = manifest_object
        .get("name")
        .and_then(JsonValue::as_str)
        .ok_or_else(|| {
            OpenPluginError::new(
                Some(manifest_path.clone()),
                "Open Plugin manifest requires string field \"name\"",
            )
        })?
        .to_string();
    validate_plugin_name(&name).map_err(|message| {
        OpenPluginError::new(
            Some(manifest_path.clone()),
            format!("invalid Open Plugin name {name:?}: {message}"),
        )
    })?;

    let skills = discover_plugin_skills(root, &name, manifest_object, &mut catalog.diagnostics);
    let mcp_servers = discover_plugin_mcp_servers(
        root,
        &name,
        data_dir,
        manifest_object,
        &mut catalog.diagnostics,
    );

    Ok(OpenPluginPackage {
        name,
        root: root.to_path_buf(),
        manifest_path,
        version: manifest_object
            .get("version")
            .and_then(JsonValue::as_str)
            .map(ToString::to_string),
        description: manifest_object
            .get("description")
            .and_then(JsonValue::as_str)
            .map(ToString::to_string),
        skills,
        mcp_servers,
    })
}

fn discover_plugin_skills(
    root: &Path,
    plugin_name: &str,
    manifest: &JsonMap<String, JsonValue>,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Vec<OpenPluginSkill> {
    let paths = component_paths(
        root,
        plugin_name,
        "skills",
        manifest.get("skills"),
        "./skills/",
        diagnostics,
    );
    let mut skills = Vec::new();
    let mut seen = BTreeSet::new();

    for path in paths {
        let Ok(metadata) = fs::metadata(&path.absolute) else {
            continue;
        };
        if !metadata.is_dir() {
            diagnostics.push(OpenPluginDiagnostic::warn(
                "open_plugin.skills.invalid_path",
                Some(plugin_name.to_string()),
                Some(path.absolute),
                "skills discovery path is not a directory; skipped",
            ));
            continue;
        }

        let mut child_dirs = match fs::read_dir(&path.absolute) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|entry_path| entry_path.is_dir())
                .collect::<Vec<_>>(),
            Err(error) => {
                diagnostics.push(OpenPluginDiagnostic::warn(
                    "open_plugin.skills.read_failed",
                    Some(plugin_name.to_string()),
                    Some(path.absolute),
                    format!("failed to list skills: {error}"),
                ));
                continue;
            }
        };
        child_dirs.sort();

        for skill_dir in child_dirs {
            if is_hidden_path(&skill_dir) {
                continue;
            }
            let Some(skill_file) = find_skill_file(&skill_dir) else {
                continue;
            };
            let content = match fs::read_to_string(&skill_file) {
                Ok(content) => content,
                Err(error) => {
                    diagnostics.push(OpenPluginDiagnostic::warn(
                        "open_plugin.skills.read_failed",
                        Some(plugin_name.to_string()),
                        Some(skill_file),
                        format!("failed to read skill file: {error}"),
                    ));
                    continue;
                }
            };
            let filename = skill_file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("SKILL.md")
                .to_string();
            let metadata = match parse_agent_skill_metadata(
                skill_dir.display().to_string(),
                &filename,
                &content,
            ) {
                Ok(metadata) => metadata,
                Err(error) => {
                    diagnostics.push(OpenPluginDiagnostic::warn(
                        "open_plugin.skills.invalid_frontmatter",
                        Some(plugin_name.to_string()),
                        Some(skill_file),
                        error.to_string(),
                    ));
                    continue;
                }
            };
            let namespaced_name = format!("{plugin_name}:{}", metadata.name);
            if !seen.insert(namespaced_name.to_ascii_lowercase()) {
                diagnostics.push(OpenPluginDiagnostic::warn(
                    "open_plugin.skills.name_conflict",
                    Some(plugin_name.to_string()),
                    Some(skill_dir),
                    format!("duplicate plugin skill {namespaced_name:?}; using first definition"),
                ));
                continue;
            }

            let runtime = open_agent_skill_metadata(&namespaced_name, &metadata);
            skills.push(OpenPluginSkill {
                plugin_name: plugin_name.to_string(),
                name: namespaced_name,
                source_name: metadata.name,
                path: PathBuf::from(&metadata.path),
                filename: metadata.filename,
                runtime,
            });
        }
    }

    skills
}

fn discover_plugin_mcp_servers(
    root: &Path,
    plugin_name: &str,
    data_dir: Option<&Path>,
    manifest: &JsonMap<String, JsonValue>,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Vec<OpenPluginMcpServer> {
    let sources = mcp_sources(root, plugin_name, manifest.get("mcpServers"), diagnostics);
    let plugin_data_dir = data_dir.map(|data_dir| data_dir.join(plugin_name));
    let mut servers = Vec::new();
    let mut seen = BTreeSet::new();
    let mut context = McpCollectContext {
        root,
        plugin_name,
        plugin_data_dir: plugin_data_dir.as_deref(),
        seen: &mut seen,
        diagnostics,
        servers: &mut servers,
    };

    for source in sources {
        match source {
            McpSource::Inline(value) => {
                collect_mcp_servers(&mut context, "manifest:mcpServers", &value)
            }
            McpSource::Path(path) => {
                let Ok(metadata) = fs::metadata(&path.absolute) else {
                    if !path.is_default {
                        context.diagnostics.push(OpenPluginDiagnostic::warn(
                            "open_plugin.mcp.missing_config",
                            Some(plugin_name.to_string()),
                            Some(path.absolute),
                            "manifest-declared MCP config file does not exist; skipped",
                        ));
                    }
                    continue;
                };
                if !metadata.is_file() {
                    context.diagnostics.push(OpenPluginDiagnostic::warn(
                        "open_plugin.mcp.invalid_path",
                        Some(plugin_name.to_string()),
                        Some(path.absolute),
                        "MCP discovery path must be a JSON file; skipped",
                    ));
                    continue;
                }
                match read_json_file(&path.absolute) {
                    Ok(value) => collect_mcp_servers(&mut context, &path.raw, &value),
                    Err(error) => context.diagnostics.push(OpenPluginDiagnostic::warn(
                        "open_plugin.mcp.invalid_config",
                        Some(plugin_name.to_string()),
                        Some(path.absolute),
                        error.to_string(),
                    )),
                }
            }
        }
    }

    servers
}

fn collect_mcp_servers(context: &mut McpCollectContext<'_>, source: &str, value: &JsonValue) {
    let Some(mcp_servers) = value.get("mcpServers").and_then(JsonValue::as_object) else {
        context.diagnostics.push(OpenPluginDiagnostic::warn(
            "open_plugin.mcp.invalid_config",
            Some(context.plugin_name.to_string()),
            None,
            "MCP config must contain a top-level object field \"mcpServers\"; skipped",
        ));
        return;
    };

    let mut server_names = mcp_servers.keys().cloned().collect::<Vec<_>>();
    server_names.sort();

    for server_name in server_names {
        let key = server_name.to_ascii_lowercase();
        if !context.seen.insert(key) {
            context.diagnostics.push(OpenPluginDiagnostic::warn(
                "open_plugin.mcp.name_conflict",
                Some(context.plugin_name.to_string()),
                None,
                format!(
                    "MCP server {server_name:?} is defined more than once; using first definition"
                ),
            ));
            continue;
        }

        let Some(server) = mcp_servers.get(&server_name).and_then(JsonValue::as_object) else {
            context.diagnostics.push(OpenPluginDiagnostic::warn(
                "open_plugin.mcp.invalid_server",
                Some(context.plugin_name.to_string()),
                None,
                format!("MCP server {server_name:?} must be an object; skipped"),
            ));
            continue;
        };

        context.servers.push(OpenPluginMcpServer {
            plugin_name: context.plugin_name.to_string(),
            server_name,
            source: source.to_string(),
            command: string_field(server, "command").map(|value| {
                expand_plugin_placeholders(value, context.root, context.plugin_data_dir)
            }),
            args: string_array_field(server, "args")
                .into_iter()
                .map(|value| {
                    expand_plugin_placeholders(&value, context.root, context.plugin_data_dir)
                })
                .collect(),
            cwd: string_field(server, "cwd").map(|value| {
                expand_plugin_placeholders(value, context.root, context.plugin_data_dir)
            }),
            env_keys: env_keys(server),
        });

        if let Some(env_object) = server.get("env").and_then(JsonValue::as_object) {
            for value in env_object.values().filter_map(JsonValue::as_str) {
                let _ = expand_plugin_placeholders(value, context.root, context.plugin_data_dir);
            }
        }
    }
}

fn component_paths(
    root: &Path,
    plugin_name: &str,
    field_name: &'static str,
    value: Option<&JsonValue>,
    default_path: &str,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Vec<ResolvedPluginPath> {
    let Some(value) = value else {
        return resolve_path_list(
            root,
            plugin_name,
            field_name,
            &[default_path],
            true,
            diagnostics,
        );
    };

    match path_strings_from_value(plugin_name, field_name, value, diagnostics) {
        Some(paths) => resolve_path_list(root, plugin_name, field_name, &paths, false, diagnostics),
        None => Vec::new(),
    }
}

fn mcp_sources(
    root: &Path,
    plugin_name: &str,
    value: Option<&JsonValue>,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Vec<McpSource> {
    let Some(value) = value else {
        return resolve_path_list(
            root,
            plugin_name,
            "mcpServers",
            &["./.mcp.json"],
            true,
            diagnostics,
        )
        .into_iter()
        .map(McpSource::Path)
        .collect();
    };

    if let Some(object) = value.as_object() {
        let has_paths = object.contains_key("paths");
        let has_inline = object.contains_key("mcpServers");
        if has_inline && !has_paths {
            return vec![McpSource::Inline(value.clone())];
        }
        if has_inline && has_paths {
            diagnostics.push(OpenPluginDiagnostic::warn(
                "open_plugin.manifest.invalid_object",
                Some(plugin_name.to_string()),
                None,
                "manifest field \"mcpServers\" cannot contain both \"paths\" and inline \"mcpServers\"",
            ));
            return Vec::new();
        }
    }

    match path_strings_from_value(plugin_name, "mcpServers", value, diagnostics) {
        Some(paths) => {
            resolve_path_list(root, plugin_name, "mcpServers", &paths, false, diagnostics)
                .into_iter()
                .map(McpSource::Path)
                .collect()
        }
        None => Vec::new(),
    }
}

fn path_strings_from_value(
    plugin_name: &str,
    field_name: &'static str,
    value: &JsonValue,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Option<Vec<String>> {
    match value {
        JsonValue::String(path) => Some(vec![path.clone()]),
        JsonValue::Array(paths) => {
            let mut output = Vec::new();
            for path in paths {
                if let Some(path) = path.as_str() {
                    output.push(path.to_string());
                } else {
                    diagnostics.push(OpenPluginDiagnostic::warn(
                        "open_plugin.manifest.invalid_path",
                        Some(plugin_name.to_string()),
                        None,
                        format!("manifest field {field_name:?} arrays must contain only strings"),
                    ));
                    return None;
                }
            }
            Some(output)
        }
        JsonValue::Object(object) => {
            let Some(paths) = object.get("paths").and_then(JsonValue::as_array) else {
                diagnostics.push(OpenPluginDiagnostic::warn(
                    "open_plugin.manifest.invalid_object",
                    Some(plugin_name.to_string()),
                    None,
                    format!("manifest field {field_name:?} has an unrecognized object shape"),
                ));
                return None;
            };
            let mut output = Vec::new();
            for path in paths {
                if let Some(path) = path.as_str() {
                    output.push(path.to_string());
                } else {
                    diagnostics.push(OpenPluginDiagnostic::warn(
                        "open_plugin.manifest.invalid_path",
                        Some(plugin_name.to_string()),
                        None,
                        format!("manifest field {field_name:?}.paths must contain only strings"),
                    ));
                    return None;
                }
            }
            Some(output)
        }
        _ => {
            diagnostics.push(OpenPluginDiagnostic::warn(
                "open_plugin.manifest.invalid_path",
                Some(plugin_name.to_string()),
                None,
                format!(
                    "manifest field {field_name:?} must be a string, array, or path config object"
                ),
            ));
            None
        }
    }
}

fn resolve_path_list(
    root: &Path,
    plugin_name: &str,
    field_name: &'static str,
    paths: &[impl AsRef<str>],
    is_default: bool,
    diagnostics: &mut Vec<OpenPluginDiagnostic>,
) -> Vec<ResolvedPluginPath> {
    let mut output = Vec::new();
    for path in paths {
        let raw = path.as_ref();
        match resolve_plugin_path(root, raw, is_default) {
            Ok(path) => output.push(path),
            Err(message) => diagnostics.push(OpenPluginDiagnostic::warn(
                "open_plugin.path.invalid",
                Some(plugin_name.to_string()),
                None,
                format!("manifest field {field_name:?} path {raw:?} is invalid: {message}"),
            )),
        }
    }
    output
}

fn resolve_plugin_path(
    root: &Path,
    raw: &str,
    is_default: bool,
) -> Result<ResolvedPluginPath, &'static str> {
    if !raw.starts_with("./") {
        return Err("relative plugin paths must start with ./");
    }
    let relative = Path::new(raw);
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => return Err("path must not contain parent traversal"),
            Component::RootDir | Component::Prefix(_) => return Err("path must be relative"),
        }
    }
    Ok(ResolvedPluginPath {
        raw: raw.to_string(),
        absolute: root.join(normalized),
        is_default,
    })
}

fn validate_plugin_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() || name.len() > 64 {
        return Err("must be between 1 and 64 characters");
    }
    if !name.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.'
    }) {
        return Err("must contain only lowercase letters, digits, hyphens, and periods");
    }
    if !name
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        || !name
            .bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Err("must start and end with an alphanumeric character");
    }
    if name.contains("--") || name.contains("..") {
        return Err("must not contain repeated hyphens or periods");
    }
    Ok(())
}

fn open_agent_skill_metadata(
    namespaced_name: &str,
    metadata: &AgentSkillMetadata,
) -> OpenAgentSkillMetadata {
    OpenAgentSkillMetadata::new(
        namespaced_name,
        metadata.description.clone(),
        metadata.path.clone(),
        metadata.filename.clone(),
    )
    .with_options(open_agent_skill_options(&metadata.options))
}

fn open_agent_skill_options(options: &AgentSkillOptions) -> OpenAgentSkillOptions {
    let mut runtime = OpenAgentSkillOptions::new();
    if let Some(disabled) = options.disable_model_invocation {
        runtime = runtime.with_disable_model_invocation(disabled);
    }
    if let Some(user_invocable) = options.user_invocable {
        runtime = runtime.with_user_invocable(user_invocable);
    }
    if !options.allowed_tools.is_empty() {
        runtime = runtime.with_allowed_tools(options.allowed_tools.clone());
    }
    if let Some(context) = &options.context {
        runtime = runtime.with_context(match context {
            AgentSkillContext::Fork => "fork",
        });
    }
    if let Some(agent) = &options.agent {
        runtime = runtime.with_agent(agent.clone());
    }
    runtime
}

fn read_json_file(path: &Path) -> Result<JsonValue, OpenPluginError> {
    let content = fs::read_to_string(path).map_err(|error| {
        OpenPluginError::new(
            Some(path.to_path_buf()),
            format!("failed to read JSON: {error}"),
        )
    })?;
    serde_json::from_str(&content).map_err(|error| {
        OpenPluginError::new(
            Some(path.to_path_buf()),
            format!("failed to parse JSON: {error}"),
        )
    })
}

fn find_skill_file(skill_dir: &Path) -> Option<PathBuf> {
    let uppercase = skill_dir.join("SKILL.md");
    if uppercase.is_file() {
        return Some(uppercase);
    }
    let lowercase = skill_dir.join("skill.md");
    if lowercase.is_file() {
        return Some(lowercase);
    }
    None
}

fn is_hidden_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with('.'))
}

fn string_field<'a>(object: &'a JsonMap<String, JsonValue>, field: &str) -> Option<&'a str> {
    object.get(field).and_then(JsonValue::as_str)
}

fn string_array_field(object: &JsonMap<String, JsonValue>, field: &str) -> Vec<String> {
    object
        .get(field)
        .and_then(JsonValue::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(JsonValue::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn env_keys(object: &JsonMap<String, JsonValue>) -> Vec<String> {
    let mut keys = object
        .get("env")
        .and_then(JsonValue::as_object)
        .map(|env| env.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    keys.sort();
    keys
}

fn expand_plugin_placeholders(value: &str, root: &Path, data_dir: Option<&Path>) -> String {
    let mut expanded = value.replace("${PLUGIN_ROOT}", &root.display().to_string());
    if let Some(data_dir) = data_dir {
        expanded = expanded.replace("${PLUGIN_DATA}", &data_dir.display().to_string());
    }
    expanded
}

fn absolutize(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        env::current_dir()
            .map(|cwd| cwd.join(&path))
            .unwrap_or(path)
    }
}
