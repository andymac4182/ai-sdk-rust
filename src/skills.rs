pub use ai_sdk_provider::skills::*;

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::json::{JsonObject, JsonValue};
use crate::provider_utils::{
    ExecuteToolOutput, ExperimentalSandbox, SandboxCommandOptions, Tool, ToolExecutionOptions,
};

const BUILT_IN_SKILL_NAMES: &[&str] = &["model", "resume", "new"];
const SKILLS_CONTEXT_KEY: &str = "skills";
const OPEN_PLUGIN_MANIFEST_PATH: &str = ".plugin/plugin.json";
const OPEN_PLUGIN_DEFAULT_SKILLS_PATH: &str = "./skills/";

/// Skill frontmatter parsed from `SKILL.md`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkillOptions {
    /// If true, the model cannot invoke this skill automatically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable_model_invocation: Option<bool>,

    /// If false, users cannot invoke this skill via slash command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_invocable: Option<bool>,

    /// Tool names or tool patterns allowed while this skill is active.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,

    /// Execution context for the skill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<AgentSkillContext>,

    /// Agent type to use for execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

/// Supported skill execution contexts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AgentSkillContext {
    /// Run the skill in a forked context.
    Fork,
}

/// Metadata discovered for one skill directory.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkillMetadata {
    /// Unique skill name.
    pub name: String,

    /// Short model-facing skill description.
    pub description: String,

    /// Absolute or sandbox-root-relative skill directory path.
    pub path: String,

    /// Skill file name, usually `SKILL.md`.
    pub filename: String,

    /// Normalized skill options.
    pub options: AgentSkillOptions,
}

impl AgentSkillMetadata {
    /// Returns the full path to this skill's markdown file.
    pub fn file_path(&self) -> String {
        join_sandbox_path(&self.path, &self.filename)
    }

    fn normalized_name(&self) -> String {
        self.name.to_lowercase()
    }
}

/// One loaded skill instruction body ready to attach to an agent call.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkillInstruction {
    /// Skill metadata used to load the instruction.
    pub metadata: AgentSkillMetadata,

    /// Model-visible skill body after directory injection and argument substitution.
    pub content: String,

    /// Arguments used for `$ARGUMENTS` substitution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

impl AgentSkillInstruction {
    /// Returns this loaded skill wrapped in the upstream command-name tag format.
    pub fn tagged_content(&self) -> String {
        format!(
            "<{name}>\n{content}\n</{name}>",
            name = self.metadata.name,
            content = self.content
        )
    }
}

/// Skill discovery result plus non-fatal skip diagnostics.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentSkillDiscovery {
    /// Discovered and invocable skill metadata in deterministic scan order.
    pub skills: Vec<AgentSkillMetadata>,

    /// Non-fatal discovery diagnostics for skipped directories or files.
    pub diagnostics: Vec<AgentSkillDiscoveryDiagnostic>,
}

/// Non-fatal discovery diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentSkillDiscoveryDiagnostic {
    /// Stable diagnostic kind.
    pub kind: AgentSkillDiscoveryDiagnosticKind,

    /// Sandbox path associated with this diagnostic.
    pub path: String,

    /// Skill name when it was parsed before the diagnostic was emitted.
    pub name: Option<String>,

    /// Human-readable diagnostic message.
    pub message: String,
}

impl AgentSkillDiscoveryDiagnostic {
    fn new(
        kind: AgentSkillDiscoveryDiagnosticKind,
        path: impl Into<String>,
        name: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            path: path.into(),
            name,
            message: message.into(),
        }
    }
}

/// Stable discovery diagnostic kinds.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentSkillDiscoveryDiagnosticKind {
    /// A listed skill directory did not contain `SKILL.md` or `skill.md`.
    MissingSkillFile,

    /// A skill directory or skill file was hidden.
    HiddenSkill,

    /// The skill frontmatter was missing or invalid.
    InvalidFrontmatter,

    /// The skill was explicitly disabled.
    DisabledSkill,

    /// The skill shadows a built-in command.
    BuiltInName,

    /// A previous skill with the same case-insensitive name already won.
    DuplicateName,

    /// An Open Plugin manifest could not be read or parsed.
    InvalidPluginManifest,

    /// An Open Plugin manifest declared a path outside the plugin package.
    InvalidPluginPath,

    /// The sandbox failed while listing a skill root.
    SandboxListError,

    /// The sandbox failed while reading a skill file.
    SandboxReadError,
}

/// Options for sandbox-backed skill discovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentSkillDiscoveryOptions {
    /// Sandbox project root.
    pub project_root: String,

    /// Sandbox home directory for global skills. If omitted, discovery asks the sandbox for `$HOME`.
    pub home_dir: Option<String>,

    /// Whether to include `~/.agents/skills`.
    pub include_global: bool,

    /// Extra sandbox directories to scan after the default roots.
    pub extra_directories: Vec<String>,

    /// Open Plugin package roots to scan after project/global skill roots.
    pub plugin_roots: Vec<String>,
}

impl AgentSkillDiscoveryOptions {
    /// Creates discovery options for a sandbox project root.
    pub fn new(project_root: impl Into<String>) -> Self {
        Self {
            project_root: project_root.into(),
            home_dir: None,
            include_global: true,
            extra_directories: Vec::new(),
            plugin_roots: Vec::new(),
        }
    }

    /// Sets the sandbox home directory.
    pub fn with_home_dir(mut self, home_dir: impl Into<String>) -> Self {
        self.home_dir = Some(home_dir.into());
        self
    }

    /// Sets whether global `~/.agents/skills` should be scanned.
    pub const fn with_include_global(mut self, include_global: bool) -> Self {
        self.include_global = include_global;
        self
    }

    /// Adds an extra skill root to scan after defaults.
    pub fn with_extra_directory(mut self, directory: impl Into<String>) -> Self {
        self.extra_directories.push(directory.into());
        self
    }

    /// Adds an Open Plugin package root to scan after project/global skill roots.
    pub fn with_plugin_root(mut self, plugin_root: impl Into<String>) -> Self {
        self.plugin_roots.push(plugin_root.into());
        self
    }
}

/// Source that requested a skill invocation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentSkillInvocationSource {
    /// The model invoked the skill tool.
    Model,

    /// The user invoked a slash-command style skill directly.
    User,
}

/// Parsed slash-command style skill invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentSkillSlashCommand {
    /// Skill name without the leading slash.
    pub skill: String,

    /// Trailing arguments, when present.
    pub arguments: Option<String>,
}

/// Error returned by skill parsing, discovery, or invocation helpers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentSkillError {
    /// The skill frontmatter is missing or invalid.
    InvalidFrontmatter { message: String },

    /// The requested skill does not exist.
    NotFound {
        skill: String,
        available_skills: Vec<String>,
    },

    /// A model tried to invoke a skill that disabled model invocation.
    ModelInvocationDisabled { skill: String },

    /// A user tried to invoke a model-only skill.
    UserInvocationDisabled { skill: String },

    /// Invocation required a sandbox but none was supplied.
    MissingSandbox,

    /// A sandbox command failed.
    Sandbox {
        path: String,
        exit_code: i32,
        stderr: String,
    },
}

impl fmt::Display for AgentSkillError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFrontmatter { message } => formatter.write_str(message),
            Self::NotFound {
                skill,
                available_skills,
            } => write!(
                formatter,
                "skill '{skill}' not found. Available skills: {}",
                if available_skills.is_empty() {
                    "none".to_string()
                } else {
                    available_skills.join(", ")
                }
            ),
            Self::ModelInvocationDisabled { skill } => write!(
                formatter,
                "skill '{skill}' cannot be invoked by the model (disable-model-invocation is set)"
            ),
            Self::UserInvocationDisabled { skill } => {
                write!(formatter, "skill '{skill}' cannot be invoked by users")
            }
            Self::MissingSandbox => formatter.write_str("skill invocation requires a sandbox"),
            Self::Sandbox {
                path,
                exit_code,
                stderr,
            } => write!(
                formatter,
                "sandbox command failed for {path} with exit code {exit_code}: {stderr}"
            ),
        }
    }
}

impl Error for AgentSkillError {}

#[derive(Clone, Debug, Default)]
struct RawSkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
    disable_model_invocation: Option<bool>,
    user_invocable: Option<bool>,
    allowed_tools: Vec<String>,
    context: Option<AgentSkillContext>,
    agent: Option<String>,
    disabled: bool,
}

/// Discovers skills from the default project and global skill roots.
pub async fn discover_agent_skills(
    sandbox: &dyn ExperimentalSandbox,
    options: AgentSkillDiscoveryOptions,
) -> AgentSkillDiscovery {
    let plugin_roots = options.plugin_roots.clone();
    let directories = resolve_skill_directories(sandbox, options).await;
    let roots = directories
        .into_iter()
        .map(AgentSkillRoot::directory)
        .collect::<Vec<_>>();
    let mut discovery = AgentSkillDiscovery::default();
    let mut seen_names = BTreeSet::new();

    discover_agent_skills_from_roots(sandbox, &roots, &mut seen_names, &mut discovery).await;
    discover_open_plugin_agent_skills_into(sandbox, &plugin_roots, &mut seen_names, &mut discovery)
        .await;

    discovery
}

/// Discovers skills from explicit sandbox directories in the supplied order.
pub async fn discover_agent_skills_in_directories(
    sandbox: &dyn ExperimentalSandbox,
    directories: &[String],
) -> AgentSkillDiscovery {
    let roots = directories
        .iter()
        .cloned()
        .map(AgentSkillRoot::directory)
        .collect::<Vec<_>>();
    let mut discovery = AgentSkillDiscovery::default();
    let mut seen_names = BTreeSet::new();

    discover_agent_skills_from_roots(sandbox, &roots, &mut seen_names, &mut discovery).await;

    discovery
}

/// Discovers namespaced Agent Skills from Open Plugin package roots.
pub async fn discover_open_plugin_agent_skills(
    sandbox: &dyn ExperimentalSandbox,
    plugin_roots: &[String],
) -> AgentSkillDiscovery {
    let mut discovery = AgentSkillDiscovery::default();
    let mut seen_names = BTreeSet::new();

    discover_open_plugin_agent_skills_into(sandbox, plugin_roots, &mut seen_names, &mut discovery)
        .await;

    discovery
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AgentSkillRoot {
    directory: String,
    namespace: Option<String>,
}

impl AgentSkillRoot {
    fn directory(directory: impl Into<String>) -> Self {
        Self {
            directory: directory.into(),
            namespace: None,
        }
    }

    fn namespaced(directory: impl Into<String>, namespace: impl Into<String>) -> Self {
        Self {
            directory: directory.into(),
            namespace: Some(namespace.into()),
        }
    }

    fn skill_name(&self, name: &str) -> String {
        match &self.namespace {
            Some(namespace) => format!("{namespace}:{name}"),
            None => name.to_string(),
        }
    }
}

async fn discover_agent_skills_from_roots(
    sandbox: &dyn ExperimentalSandbox,
    roots: &[AgentSkillRoot],
    seen_names: &mut BTreeSet<String>,
    discovery: &mut AgentSkillDiscovery,
) {
    for root in roots {
        let skill_dirs = match sandbox_list_child_directories(sandbox, &root.directory).await {
            Ok(skill_dirs) => skill_dirs,
            Err(error) => {
                discovery.diagnostics.push(error);
                continue;
            }
        };

        for skill_dir in skill_dirs {
            if is_hidden_path_component(&skill_dir) {
                discovery
                    .diagnostics
                    .push(AgentSkillDiscoveryDiagnostic::new(
                        AgentSkillDiscoveryDiagnosticKind::HiddenSkill,
                        skill_dir,
                        None,
                        "hidden skill directories are skipped",
                    ));
                continue;
            }

            let Some(skill_file) = sandbox_find_skill_file(sandbox, &skill_dir).await else {
                discovery
                    .diagnostics
                    .push(AgentSkillDiscoveryDiagnostic::new(
                        AgentSkillDiscoveryDiagnosticKind::MissingSkillFile,
                        skill_dir,
                        None,
                        "skill directory is missing SKILL.md or skill.md",
                    ));
                continue;
            };

            let content = match sandbox_read_text_file(sandbox, &skill_file).await {
                Ok(content) => content,
                Err(error) => {
                    discovery
                        .diagnostics
                        .push(AgentSkillDiscoveryDiagnostic::new(
                            AgentSkillDiscoveryDiagnosticKind::SandboxReadError,
                            skill_file,
                            None,
                            error.to_string(),
                        ));
                    continue;
                }
            };

            let frontmatter = match parse_raw_skill_frontmatter(&content) {
                Ok(frontmatter) => frontmatter,
                Err(error) => {
                    discovery
                        .diagnostics
                        .push(AgentSkillDiscoveryDiagnostic::new(
                            AgentSkillDiscoveryDiagnosticKind::InvalidFrontmatter,
                            skill_file,
                            None,
                            error.to_string(),
                        ));
                    continue;
                }
            };

            let name = frontmatter.name.clone().unwrap_or_default();
            let skill_name = root.skill_name(&name);
            let normalized_name = skill_name.to_lowercase();

            if frontmatter.disabled {
                discovery
                    .diagnostics
                    .push(AgentSkillDiscoveryDiagnostic::new(
                        AgentSkillDiscoveryDiagnosticKind::DisabledSkill,
                        skill_dir,
                        Some(skill_name),
                        "disabled skills are skipped",
                    ));
                continue;
            }

            if is_hidden_skill_name(&name) {
                discovery
                    .diagnostics
                    .push(AgentSkillDiscoveryDiagnostic::new(
                        AgentSkillDiscoveryDiagnosticKind::HiddenSkill,
                        skill_dir,
                        Some(skill_name),
                        "hidden skill names are skipped",
                    ));
                continue;
            }

            if BUILT_IN_SKILL_NAMES
                .iter()
                .any(|built_in| *built_in == normalized_name)
            {
                discovery
                    .diagnostics
                    .push(AgentSkillDiscoveryDiagnostic::new(
                        AgentSkillDiscoveryDiagnosticKind::BuiltInName,
                        skill_dir,
                        Some(skill_name),
                        "skill shadows a built-in command",
                    ));
                continue;
            }

            if !seen_names.insert(normalized_name) {
                discovery
                    .diagnostics
                    .push(AgentSkillDiscoveryDiagnostic::new(
                        AgentSkillDiscoveryDiagnosticKind::DuplicateName,
                        skill_dir,
                        Some(skill_name),
                        "duplicate skill names are skipped after the first match",
                    ));
                continue;
            }

            discovery.skills.push(AgentSkillMetadata {
                name: skill_name,
                description: frontmatter.description.unwrap_or_default(),
                path: skill_dir,
                filename: basename(&skill_file).to_string(),
                options: AgentSkillOptions {
                    disable_model_invocation: frontmatter.disable_model_invocation,
                    user_invocable: frontmatter.user_invocable,
                    allowed_tools: frontmatter.allowed_tools,
                    context: frontmatter.context,
                    agent: frontmatter.agent,
                },
            });
        }
    }
}

async fn discover_open_plugin_agent_skills_into(
    sandbox: &dyn ExperimentalSandbox,
    plugin_roots: &[String],
    seen_names: &mut BTreeSet<String>,
    discovery: &mut AgentSkillDiscovery,
) {
    for plugin_root in plugin_roots {
        let manifest_path = join_sandbox_path(plugin_root, OPEN_PLUGIN_MANIFEST_PATH);
        let manifest_content = match sandbox_read_text_file(sandbox, &manifest_path).await {
            Ok(content) => content,
            Err(error) => {
                discovery
                    .diagnostics
                    .push(AgentSkillDiscoveryDiagnostic::new(
                        AgentSkillDiscoveryDiagnosticKind::InvalidPluginManifest,
                        manifest_path,
                        None,
                        error.to_string(),
                    ));
                continue;
            }
        };

        let (plugin_name, skill_roots, diagnostics) = match open_plugin_skill_roots_from_manifest(
            plugin_root,
            &manifest_path,
            &manifest_content,
        ) {
            Ok(result) => result,
            Err(error) => {
                discovery.diagnostics.push(error);
                continue;
            }
        };

        discovery.diagnostics.extend(diagnostics);

        let roots = skill_roots
            .into_iter()
            .map(|root| AgentSkillRoot::namespaced(root, plugin_name.clone()))
            .collect::<Vec<_>>();
        discover_agent_skills_from_roots(sandbox, &roots, seen_names, discovery).await;
    }
}

fn open_plugin_skill_roots_from_manifest(
    plugin_root: &str,
    manifest_path: &str,
    manifest_content: &str,
) -> Result<(String, Vec<String>, Vec<AgentSkillDiscoveryDiagnostic>), AgentSkillDiscoveryDiagnostic>
{
    let manifest = serde_json::from_str::<JsonValue>(manifest_content).map_err(|error| {
        AgentSkillDiscoveryDiagnostic::new(
            AgentSkillDiscoveryDiagnosticKind::InvalidPluginManifest,
            manifest_path,
            None,
            format!("Open Plugin manifest is not valid JSON: {error}"),
        )
    })?;

    let Some(manifest_object) = manifest.as_object() else {
        return Err(AgentSkillDiscoveryDiagnostic::new(
            AgentSkillDiscoveryDiagnosticKind::InvalidPluginManifest,
            manifest_path,
            None,
            "Open Plugin manifest must be a JSON object",
        ));
    };

    let Some(plugin_name) = manifest_object
        .get("name")
        .and_then(JsonValue::as_str)
        .filter(|name| is_valid_open_plugin_name(name))
    else {
        return Err(AgentSkillDiscoveryDiagnostic::new(
            AgentSkillDiscoveryDiagnosticKind::InvalidPluginManifest,
            manifest_path,
            manifest_object
                .get("name")
                .and_then(JsonValue::as_str)
                .map(ToString::to_string),
            "Open Plugin manifest name must be 1-64 lowercase alphanumeric, hyphen, or period characters; start and end alphanumeric; and avoid repeated separators",
        ));
    };

    let mut diagnostics = Vec::new();
    let raw_paths = match manifest_object.get("skills") {
        Some(skills) => match parse_open_plugin_skill_paths(skills) {
            Ok(paths) => paths,
            Err(message) => {
                diagnostics.push(AgentSkillDiscoveryDiagnostic::new(
                    AgentSkillDiscoveryDiagnosticKind::InvalidPluginManifest,
                    manifest_path,
                    Some(plugin_name.to_string()),
                    message,
                ));
                Vec::new()
            }
        },
        None => vec![OPEN_PLUGIN_DEFAULT_SKILLS_PATH.to_string()],
    };

    let mut skill_roots = Vec::new();
    for raw_path in raw_paths {
        match normalize_open_plugin_relative_path(plugin_root, &raw_path) {
            Ok(path) => skill_roots.push(path),
            Err(message) => diagnostics.push(AgentSkillDiscoveryDiagnostic::new(
                AgentSkillDiscoveryDiagnosticKind::InvalidPluginPath,
                raw_path,
                Some(plugin_name.to_string()),
                message,
            )),
        }
    }

    Ok((plugin_name.to_string(), skill_roots, diagnostics))
}

fn parse_open_plugin_skill_paths(value: &JsonValue) -> Result<Vec<String>, String> {
    match value {
        JsonValue::String(path) => Ok(vec![path.clone()]),
        JsonValue::Array(paths) => paths
            .iter()
            .map(|path| {
                path.as_str().map(ToString::to_string).ok_or_else(|| {
                    "Open Plugin manifest field \"skills\" must contain only string paths"
                        .to_string()
                })
            })
            .collect(),
        JsonValue::Object(object) => {
            let Some(paths) = object.get("paths") else {
                return Err(
                    "Open Plugin manifest field \"skills\" object must contain a \"paths\" array"
                        .to_string(),
                );
            };
            let Some(paths) = paths.as_array() else {
                return Err(
                    "Open Plugin manifest field \"skills.paths\" must be an array".to_string(),
                );
            };
            paths
                .iter()
                .map(|path| {
                    path.as_str().map(ToString::to_string).ok_or_else(|| {
                        "Open Plugin manifest field \"skills.paths\" must contain only string paths"
                            .to_string()
                    })
                })
                .collect()
        }
        _ => Err(
            "Open Plugin manifest field \"skills\" must be a string, string array, or path config object"
                .to_string(),
        ),
    }
}

fn normalize_open_plugin_relative_path(
    plugin_root: &str,
    relative_path: &str,
) -> Result<String, String> {
    if !relative_path.starts_with("./") {
        return Err("Open Plugin manifest paths must start with \"./\"".to_string());
    }

    let mut parts = Vec::new();
    for part in relative_path.split('/') {
        match part {
            "" | "." => {}
            ".." if parts.pop().is_none() => {
                return Err("Open Plugin manifest path escapes the plugin root".to_string());
            }
            ".." => {}
            part => parts.push(part),
        }
    }

    let root = trim_sandbox_trailing_slash(plugin_root);
    if parts.is_empty() {
        Ok(root.to_string())
    } else {
        Ok(join_sandbox_path(root, &parts.join("/")))
    }
}

fn is_valid_open_plugin_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }

    let mut previous = None;
    for (index, byte) in name.bytes().enumerate() {
        let is_alphanumeric = byte.is_ascii_lowercase() || byte.is_ascii_digit();
        let is_separator = matches!(byte, b'-' | b'.');
        if !is_alphanumeric && !is_separator {
            return false;
        }
        if (index == 0 || index == name.len() - 1) && !is_alphanumeric {
            return false;
        }
        if matches!((previous, byte), (Some(b'-'), b'-') | (Some(b'.'), b'.')) {
            return false;
        }
        previous = Some(byte);
    }

    true
}

/// Parses frontmatter and returns normalized metadata for a skill file at `path`.
pub fn parse_agent_skill_metadata(
    path: impl Into<String>,
    filename: impl Into<String>,
    content: &str,
) -> Result<AgentSkillMetadata, AgentSkillError> {
    let path = path.into();
    let filename = filename.into();
    let frontmatter = parse_raw_skill_frontmatter(content)?;

    if frontmatter.disabled {
        return Err(AgentSkillError::InvalidFrontmatter {
            message: "skill is disabled".to_string(),
        });
    }

    Ok(AgentSkillMetadata {
        name: frontmatter.name.unwrap_or_default(),
        description: frontmatter.description.unwrap_or_default(),
        path,
        filename,
        options: AgentSkillOptions {
            disable_model_invocation: frontmatter.disable_model_invocation,
            user_invocable: frontmatter.user_invocable,
            allowed_tools: frontmatter.allowed_tools,
            context: frontmatter.context,
            agent: frontmatter.agent,
        },
    })
}

/// Extracts the markdown body after YAML frontmatter.
pub fn extract_agent_skill_body(content: &str) -> String {
    let Some(frontmatter_end) = frontmatter_end_index(content) else {
        return content.trim().to_string();
    };

    content[frontmatter_end..].trim().to_string()
}

/// Replaces all `$ARGUMENTS` placeholders in a skill body.
pub fn substitute_agent_skill_arguments(body: &str, arguments: Option<&str>) -> String {
    body.replace("$ARGUMENTS", arguments.unwrap_or_default())
}

/// Prepends the skill directory to model-visible skill instructions.
pub fn inject_agent_skill_directory(body: &str, skill_dir: &str) -> String {
    format!("Skill directory: {skill_dir}\n\n{body}")
}

/// Parses a user message that starts with `/<skill-name>`.
pub fn parse_agent_skill_slash_command(message: &str) -> Option<AgentSkillSlashCommand> {
    let trimmed = message.trim_start();
    let command = trimmed.strip_prefix('/')?;
    let mut parts = command.splitn(2, char::is_whitespace);
    let skill = parts.next()?.trim();

    if skill.is_empty() {
        return None;
    }

    Some(AgentSkillSlashCommand {
        skill: skill.to_string(),
        arguments: parts
            .next()
            .map(str::trim)
            .filter(|arguments| !arguments.is_empty())
            .map(ToString::to_string),
    })
}

/// Loads a skill body through the sandbox and prepares model-visible instructions.
pub async fn invoke_agent_skill(
    sandbox: &dyn ExperimentalSandbox,
    skills: &[AgentSkillMetadata],
    skill: &str,
    arguments: Option<&str>,
    source: AgentSkillInvocationSource,
) -> Result<AgentSkillInstruction, AgentSkillError> {
    let normalized_skill = skill.to_lowercase();
    let metadata = skills
        .iter()
        .find(|candidate| candidate.normalized_name() == normalized_skill)
        .cloned()
        .ok_or_else(|| AgentSkillError::NotFound {
            skill: skill.to_string(),
            available_skills: skills.iter().map(|skill| skill.name.clone()).collect(),
        })?;

    match source {
        AgentSkillInvocationSource::Model
            if metadata.options.disable_model_invocation.unwrap_or(false) =>
        {
            return Err(AgentSkillError::ModelInvocationDisabled {
                skill: skill.to_string(),
            });
        }
        AgentSkillInvocationSource::User if metadata.options.user_invocable == Some(false) => {
            return Err(AgentSkillError::UserInvocationDisabled {
                skill: skill.to_string(),
            });
        }
        _ => {}
    }

    let file_path = metadata.file_path();
    let content = sandbox_read_text_file(sandbox, &file_path).await?;
    let body = extract_agent_skill_body(&content);
    let body = inject_agent_skill_directory(&body, &metadata.path);
    let content = substitute_agent_skill_arguments(&body, arguments);

    Ok(AgentSkillInstruction {
        metadata,
        content,
        arguments: arguments.map(ToString::to_string),
    })
}

/// Loads a skill when the user message begins with a slash command.
pub async fn invoke_agent_skill_slash_command(
    sandbox: &dyn ExperimentalSandbox,
    skills: &[AgentSkillMetadata],
    message: &str,
) -> Option<Result<AgentSkillInstruction, AgentSkillError>> {
    let command = parse_agent_skill_slash_command(message)?;
    Some(
        invoke_agent_skill(
            sandbox,
            skills,
            &command.skill,
            command.arguments.as_deref(),
            AgentSkillInvocationSource::User,
        )
        .await,
    )
}

/// Builds the Open Agents skills section for a system prompt.
pub fn build_agent_skills_prompt(skills: &[AgentSkillMetadata]) -> Option<String> {
    let invocable_skills = skills
        .iter()
        .filter(|skill| !skill.options.disable_model_invocation.unwrap_or(false))
        .collect::<Vec<_>>();

    if invocable_skills.is_empty() {
        return None;
    }

    let skills_list = invocable_skills
        .iter()
        .map(|skill| {
            let suffix = if skill.options.user_invocable == Some(false) {
                " (model-only)"
            } else {
                ""
            };
            format!("- {}: {}{}", skill.name, skill.description, suffix)
        })
        .collect::<Vec<_>>()
        .join("\n");

    Some(format!(
        "## Skills\n\
         - `skill` - Execute a skill to extend your capabilities\n\
         - Use the `skill` tool to invoke skills when relevant to the user's request\n\
         - When a user references \"/<skill-name>\" or \"/<plugin>:<skill-name>\" (e.g., \"/commit\" or \"/devtools:deploy\"), invoke the corresponding skill\n\
         - Some skills may be model-only (not user-invocable) and should be invoked automatically when relevant\n\n\
         Available skills:\n\
         {skills_list}\n\n\
         When a skill is relevant, invoke it IMMEDIATELY using the skill tool.\n\
         If you see a <command-name> tag in the conversation, the skill is already loaded - follow its instructions directly.\n\n\
         IMPORTANT - Slash command detection:\n\
         When the user's message starts with \"/<name>\" or \"/<plugin>:<name>\", they are invoking a skill.\n\
         Check if \"<name>\" matches an available skill above. If it does, your FIRST tool call MUST be the skill tool."
    ))
}

/// Builds a system prompt section containing already-loaded skill instructions.
pub fn build_loaded_agent_skills_prompt(skills: &[AgentSkillInstruction]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }

    let loaded = skills
        .iter()
        .map(AgentSkillInstruction::tagged_content)
        .collect::<Vec<_>>()
        .join("\n\n");

    Some(format!(
        "## Loaded Skills\n\
         The following skill instructions are already loaded. Follow them directly when relevant.\n\n\
         {loaded}"
    ))
}

/// Builds all skill prompt sections for available and already-loaded skills.
pub fn build_agent_skill_prompt_sections(
    available_skills: &[AgentSkillMetadata],
    loaded_skills: &[AgentSkillInstruction],
) -> Option<String> {
    let mut sections = Vec::new();

    if let Some(skills_prompt) = build_agent_skills_prompt(available_skills) {
        sections.push(skills_prompt);
    }
    if let Some(loaded_prompt) = build_loaded_agent_skills_prompt(loaded_skills) {
        sections.push(loaded_prompt);
    }

    if sections.is_empty() {
        None
    } else {
        Some(sections.join("\n\n"))
    }
}

/// Merges skill metadata in first-wins, case-insensitive order.
pub fn merge_agent_skills(
    base: &[AgentSkillMetadata],
    overrides: Vec<AgentSkillMetadata>,
) -> Vec<AgentSkillMetadata> {
    let mut seen = BTreeSet::new();
    let mut merged = Vec::new();

    for skill in base.iter().cloned().chain(overrides) {
        if seen.insert(skill.normalized_name()) {
            merged.push(skill);
        }
    }

    merged
}

/// Merges loaded skill instructions in first-wins, case-insensitive order.
pub fn merge_loaded_agent_skills(
    base: &[AgentSkillInstruction],
    overrides: Vec<AgentSkillInstruction>,
) -> Vec<AgentSkillInstruction> {
    let mut seen = BTreeSet::new();
    let mut merged = Vec::new();

    for skill in base.iter().cloned().chain(overrides) {
        if seen.insert(skill.metadata.normalized_name()) {
            merged.push(skill);
        }
    }

    merged
}

/// Returns the union of `allowed-tools` from loaded skills, if any loaded skill declares them.
pub fn allowed_tools_for_loaded_agent_skills(
    loaded_skills: &[AgentSkillInstruction],
) -> Option<Vec<String>> {
    let mut allowed_tools = BTreeSet::new();

    for skill in loaded_skills {
        for tool in &skill.metadata.options.allowed_tools {
            allowed_tools.insert(tool.clone());
        }
    }

    if allowed_tools.is_empty() {
        None
    } else {
        Some(allowed_tools.into_iter().collect())
    }
}

/// Attaches discovered skill metadata to a JSON runtime context.
pub fn attach_agent_skills_to_context(
    context: &mut JsonObject,
    skills: &[AgentSkillMetadata],
    loaded_skills: &[AgentSkillInstruction],
) {
    if skills.is_empty() && loaded_skills.is_empty() {
        return;
    }

    context.insert(
        SKILLS_CONTEXT_KEY.to_string(),
        json!({
            "available": skills,
            "loaded": loaded_skills,
        }),
    );
}

/// Creates the Open Agents `skill` tool.
pub fn agent_skill_tool(skills: Vec<AgentSkillMetadata>) -> Tool {
    let skills = Arc::new(skills);
    Tool::new(
        "skill",
        json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "description": "The skill name to invoke, including a plugin namespace when present"
                },
                "args": {
                    "type": "string",
                    "description": "Optional arguments for the skill"
                }
            },
            "required": ["skill"],
            "additionalProperties": false
        })
        .as_object()
        .cloned()
        .expect("skill tool schema is an object"),
    )
    .with_description(
        "Execute a skill within the main conversation. When a user starts with \
         /<name> or /<plugin>:<name>, call this tool first with the matching skill name.",
    )
    .with_execute_outputs(move |input, options| {
        let skills = Arc::clone(&skills);
        async move { execute_agent_skill_tool(skills, input, options).await }
    })
}

async fn execute_agent_skill_tool(
    skills: Arc<Vec<AgentSkillMetadata>>,
    input: JsonValue,
    options: ToolExecutionOptions,
) -> Result<Vec<ExecuteToolOutput>, crate::provider_utils::ToolExecutionError> {
    let skill = input
        .get("skill")
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_string();
    let arguments = input
        .get("args")
        .and_then(JsonValue::as_str)
        .map(ToString::to_string);

    let mut outputs = vec![ExecuteToolOutput::preliminary(json!({
        "status": format!("Loading /{skill}"),
        "skillName": skill,
    }))];

    let Some(sandbox) = options.experimental_sandbox else {
        outputs.push(ExecuteToolOutput::final_output(json!({
            "success": false,
            "error": AgentSkillError::MissingSandbox.to_string(),
        })));
        return Ok(outputs);
    };

    let result = invoke_agent_skill(
        sandbox.as_ref(),
        &skills,
        &skill,
        arguments.as_deref(),
        AgentSkillInvocationSource::Model,
    )
    .await;

    match result {
        Ok(instruction) => outputs.push(ExecuteToolOutput::final_output(json!({
            "success": true,
            "skillName": instruction.metadata.name,
            "skillPath": instruction.metadata.path,
            "allowedTools": instruction.metadata.options.allowed_tools,
            "content": instruction.content,
        }))),
        Err(error) => outputs.push(ExecuteToolOutput::final_output(json!({
            "success": false,
            "error": error.to_string(),
        }))),
    }

    Ok(outputs)
}

async fn resolve_skill_directories(
    sandbox: &dyn ExperimentalSandbox,
    options: AgentSkillDiscoveryOptions,
) -> Vec<String> {
    let mut directories = vec![
        join_sandbox_path(&options.project_root, ".agents/skills"),
        join_sandbox_path(&options.project_root, ".claude/skills"),
    ];

    if options.include_global {
        let home_dir = match options.home_dir {
            Some(home_dir) => Some(home_dir),
            None => sandbox_home_dir(sandbox).await,
        };

        if let Some(home_dir) = home_dir {
            directories.push(join_sandbox_path(&home_dir, ".agents/skills"));
        }
    }

    directories.extend(options.extra_directories);
    directories
}

async fn sandbox_home_dir(sandbox: &dyn ExperimentalSandbox) -> Option<String> {
    let result = sandbox
        .run_command(SandboxCommandOptions::new("printf '%s' \"$HOME\""))
        .await;

    if result.exit_code == 0 {
        let home = result.stdout.trim().to_string();
        if !home.is_empty() {
            return Some(home);
        }
    }

    None
}

async fn sandbox_list_child_directories(
    sandbox: &dyn ExperimentalSandbox,
    directory: &str,
) -> Result<Vec<String>, AgentSkillDiscoveryDiagnostic> {
    let command = format!(
        "if [ -d {dir} ]; then find {dir} -mindepth 1 -maxdepth 1 -type d -print; fi",
        dir = shell_quote(directory)
    );
    let result = sandbox
        .run_command(SandboxCommandOptions::new(command))
        .await;

    if result.exit_code != 0 {
        return Err(AgentSkillDiscoveryDiagnostic::new(
            AgentSkillDiscoveryDiagnosticKind::SandboxListError,
            directory,
            None,
            result.stderr,
        ));
    }

    Ok(result
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect())
}

async fn sandbox_find_skill_file(
    sandbox: &dyn ExperimentalSandbox,
    skill_dir: &str,
) -> Option<String> {
    let uppercase = join_sandbox_path(skill_dir, "SKILL.md");
    let lowercase = join_sandbox_path(skill_dir, "skill.md");
    let command = format!(
        "if [ -f {upper} ]; then printf '%s' {upper_out}; elif [ -f {lower} ]; then printf '%s' {lower_out}; fi",
        upper = shell_quote(&uppercase),
        upper_out = shell_quote(&uppercase),
        lower = shell_quote(&lowercase),
        lower_out = shell_quote(&lowercase),
    );
    let result = sandbox
        .run_command(SandboxCommandOptions::new(command))
        .await;

    if result.exit_code == 0 {
        let path = result.stdout.trim();
        if !path.is_empty() {
            return Some(path.to_string());
        }
    }

    None
}

async fn sandbox_read_text_file(
    sandbox: &dyn ExperimentalSandbox,
    path: &str,
) -> Result<String, AgentSkillError> {
    let command = format!("cat {}", shell_quote(path));
    let result = sandbox
        .run_command(SandboxCommandOptions::new(command))
        .await;

    if result.exit_code == 0 {
        Ok(result.stdout)
    } else {
        Err(AgentSkillError::Sandbox {
            path: path.to_string(),
            exit_code: result.exit_code,
            stderr: result.stderr,
        })
    }
}

fn parse_raw_skill_frontmatter(content: &str) -> Result<RawSkillFrontmatter, AgentSkillError> {
    let frontmatter =
        extract_frontmatter(content).ok_or_else(|| AgentSkillError::InvalidFrontmatter {
            message: "no frontmatter found".to_string(),
        })?;
    let fields = parse_frontmatter_fields(frontmatter);
    let mut raw = RawSkillFrontmatter::default();

    for (key, value) in fields {
        match key.as_str() {
            "name" => raw.name = Some(expect_string(&key, value)?),
            "description" => raw.description = Some(expect_string(&key, value)?),
            "version" => raw.version = Some(expect_string(&key, value)?),
            "disable-model-invocation" => {
                raw.disable_model_invocation = Some(expect_bool(&key, value)?);
            }
            "user-invocable" => raw.user_invocable = Some(expect_bool(&key, value)?),
            "allowed-tools" => {
                raw.allowed_tools = expect_string(&key, value)?
                    .split(',')
                    .map(str::trim)
                    .filter(|tool| !tool.is_empty())
                    .map(ToString::to_string)
                    .collect();
            }
            "context" => {
                let context = expect_string(&key, value)?;
                raw.context = match context.as_str() {
                    "fork" => Some(AgentSkillContext::Fork),
                    _ => {
                        return Err(AgentSkillError::InvalidFrontmatter {
                            message: format!("unsupported skill context '{context}'"),
                        });
                    }
                };
            }
            "agent" => raw.agent = Some(expect_string(&key, value)?),
            "disabled" => raw.disabled = expect_bool(&key, value)?,
            "enabled" if !expect_bool(&key, value)? => raw.disabled = true,
            _ => {}
        }
    }

    if raw.name.as_deref().is_none_or(str::is_empty) {
        return Err(AgentSkillError::InvalidFrontmatter {
            message: "skill name cannot be empty".to_string(),
        });
    }

    if raw.description.as_deref().is_none_or(str::is_empty) {
        return Err(AgentSkillError::InvalidFrontmatter {
            message: "skill description cannot be empty".to_string(),
        });
    }

    Ok(raw)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FrontmatterValue {
    String(String),
    Bool(bool),
}

fn expect_string(key: &str, value: FrontmatterValue) -> Result<String, AgentSkillError> {
    match value {
        FrontmatterValue::String(value) => Ok(value),
        FrontmatterValue::Bool(_) => Err(AgentSkillError::InvalidFrontmatter {
            message: format!("skill frontmatter '{key}' must be a string"),
        }),
    }
}

fn expect_bool(key: &str, value: FrontmatterValue) -> Result<bool, AgentSkillError> {
    match value {
        FrontmatterValue::Bool(value) => Ok(value),
        FrontmatterValue::String(value) => match value.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(AgentSkillError::InvalidFrontmatter {
                message: format!("skill frontmatter '{key}' must be a boolean"),
            }),
        },
    }
}

fn extract_frontmatter(content: &str) -> Option<&str> {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return None;
    }

    let start = if content.starts_with("---\r\n") { 5 } else { 4 };
    let rest = &content[start..];
    let mut offset = start;

    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" {
            return Some(&content[start..offset]);
        }
        offset += line.len();
    }

    None
}

fn frontmatter_end_index(content: &str) -> Option<usize> {
    if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
        return None;
    }

    let start = if content.starts_with("---\r\n") { 5 } else { 4 };
    let rest = &content[start..];
    let mut offset = start;

    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        offset += line.len();
        if trimmed == "---" {
            return Some(offset);
        }
    }

    None
}

fn parse_frontmatter_fields(frontmatter: &str) -> Vec<(String, FrontmatterValue)> {
    let lines = frontmatter.lines().collect::<Vec<_>>();
    let mut fields = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let raw_line = lines[index];
        let trimmed = raw_line.trim();
        index += 1;

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let Some(colon_index) = trimmed.find(':') else {
            continue;
        };
        let key = trimmed[..colon_index].trim().to_string();
        let raw_value = trimmed[colon_index + 1..].trim();

        if raw_value == ">" || raw_value == "|" {
            let mut multiline = Vec::new();
            while index < lines.len() {
                let next_line = lines[index];
                if !next_line.trim().is_empty()
                    && !next_line.starts_with(' ')
                    && !next_line.starts_with('\t')
                {
                    break;
                }
                multiline.push(next_line.trim());
                index += 1;
            }

            let value = if raw_value == ">" {
                multiline.join(" ")
            } else {
                multiline.join("\n")
            };
            fields.push((key, FrontmatterValue::String(value.trim().to_string())));
            continue;
        }

        fields.push((key, parse_frontmatter_scalar(raw_value)));
    }

    fields
}

fn parse_frontmatter_scalar(raw_value: &str) -> FrontmatterValue {
    if raw_value.starts_with('"') && raw_value.ends_with('"') && raw_value.len() >= 2 {
        return FrontmatterValue::String(raw_value[1..raw_value.len() - 1].replace("\\\"", "\""));
    }

    if raw_value.starts_with('\'') && raw_value.ends_with('\'') && raw_value.len() >= 2 {
        return FrontmatterValue::String(raw_value[1..raw_value.len() - 1].replace("\\'", "'"));
    }

    match raw_value {
        "true" => FrontmatterValue::Bool(true),
        "false" => FrontmatterValue::Bool(false),
        _ => FrontmatterValue::String(raw_value.to_string()),
    }
}

fn is_hidden_path_component(path: &str) -> bool {
    basename(path).starts_with('.')
}

fn is_hidden_skill_name(name: &str) -> bool {
    name.starts_with('.')
}

fn basename(path: &str) -> &str {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path)
}

fn join_sandbox_path(base: &str, child: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{child}")
    } else {
        format!("{base}/{child}")
    }
}

fn trim_sandbox_trailing_slash(path: &str) -> &str {
    if path == "/" {
        path
    } else {
        path.trim_end_matches('/')
    }
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::future::{Future, ready};
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::process::Command;
    use std::task::{Context, Poll, Waker};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::provider_utils::{SandboxCommandResult, SandboxRunCommandFuture};

    fn poll_ready<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = Box::pin(future);

        match Future::poll(Pin::as_mut(&mut future), &mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("future unexpectedly pending"),
        }
    }

    #[derive(Debug)]
    struct LocalFsSandbox {
        home_dir: String,
    }

    impl LocalFsSandbox {
        fn new(home_dir: impl Into<String>) -> Self {
            Self {
                home_dir: home_dir.into(),
            }
        }
    }

    impl ExperimentalSandbox for LocalFsSandbox {
        fn description(&self) -> &str {
            "local test sandbox"
        }

        fn run_command(&self, options: SandboxCommandOptions) -> SandboxRunCommandFuture {
            if options.command == "printf '%s' \"$HOME\"" {
                return Box::pin(ready(
                    SandboxCommandResult::new(0).with_stdout(self.home_dir.clone()),
                ));
            }

            let output = Command::new("sh")
                .arg("-c")
                .arg(options.command)
                .output()
                .expect("test sandbox command runs");

            Box::pin(ready(
                SandboxCommandResult::new(output.status.code().unwrap_or(1))
                    .with_stdout(String::from_utf8_lossy(&output.stdout).to_string())
                    .with_stderr(String::from_utf8_lossy(&output.stderr).to_string()),
            ))
        }
    }

    #[derive(Debug)]
    struct ReadErrorSandbox;

    impl ExperimentalSandbox for ReadErrorSandbox {
        fn description(&self) -> &str {
            "read error sandbox"
        }

        fn run_command(&self, options: SandboxCommandOptions) -> SandboxRunCommandFuture {
            let result = if options.command.contains("find '/skills'")
                && options.command.contains("-type d")
            {
                SandboxCommandResult::new(0).with_stdout("/skills/broken\n")
            } else if options.command.starts_with("cat ") {
                SandboxCommandResult::new(13).with_stderr("permission denied")
            } else if options.command.contains("/skills/broken/SKILL.md") {
                SandboxCommandResult::new(0).with_stdout("/skills/broken/SKILL.md")
            } else {
                SandboxCommandResult::new(0)
            };

            Box::pin(ready(result))
        }
    }

    fn temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ai-sdk-rust-agent-skills-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp root created");
        path
    }

    fn write_skill(root: &Path, relative: &str, content: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("skill parent")).expect("skill parent created");
        fs::write(path, content).expect("skill file written");
    }

    fn write_plugin_manifest(root: &Path, content: &str) {
        let path = root.join(".plugin/plugin.json");
        fs::create_dir_all(path.parent().expect("manifest parent"))
            .expect("manifest parent created");
        fs::write(path, content).expect("plugin manifest written");
    }

    fn skill_frontmatter(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\nBody")
    }

    #[test]
    fn parse_skill_frontmatter_handles_quotes_booleans_allowed_tools_and_blocks() {
        let content = concat!(
            "---\n",
            "name: \"code-review\"\n",
            "description: >\n",
            "  Reviews code changes\n",
            "  and pull requests.\n",
            "disable-model-invocation: false\n",
            "user-invocable: true\n",
            "allowed-tools: Bash(git:*), Read\n",
            "context: fork\n",
            "agent: reviewer\n",
            "---\n",
            "Use $ARGUMENTS",
        );

        let metadata =
            parse_agent_skill_metadata("/skills/code-review", "SKILL.md", content).unwrap();

        assert_eq!(metadata.name, "code-review");
        assert_eq!(
            metadata.description,
            "Reviews code changes and pull requests."
        );
        assert_eq!(metadata.options.disable_model_invocation, Some(false));
        assert_eq!(metadata.options.user_invocable, Some(true));
        assert_eq!(
            metadata.options.allowed_tools,
            vec!["Bash(git:*)".to_string(), "Read".to_string()]
        );
        assert_eq!(metadata.options.context, Some(AgentSkillContext::Fork));
        assert_eq!(metadata.options.agent.as_deref(), Some("reviewer"));
    }

    #[test]
    fn discovers_project_claude_and_global_skills_with_skip_diagnostics() {
        let root = temp_root();
        let project = root.join("project");
        let home = root.join("home");

        write_skill(
            &project,
            ".agents/skills/review/SKILL.md",
            &skill_frontmatter("review", "Project review"),
        );
        write_skill(
            &project,
            ".agents/skills/.hidden/SKILL.md",
            &skill_frontmatter(".hidden", "Hidden"),
        );
        fs::create_dir_all(project.join(".agents/skills/missing")).expect("missing dir");
        write_skill(
            &project,
            ".claude/skills/commit/skill.md",
            &skill_frontmatter("commit", "Claude commit"),
        );
        write_skill(
            &project,
            ".claude/skills/model/SKILL.md",
            &skill_frontmatter("model", "Builtin shadow"),
        );
        write_skill(
            &project,
            ".claude/skills/disabled/SKILL.md",
            "---\nname: disabled\ndescription: Disabled\ndisabled: true\n---\nBody",
        );
        write_skill(
            &home,
            ".agents/skills/review/SKILL.md",
            &skill_frontmatter("Review", "Duplicate global review"),
        );
        write_skill(
            &home,
            ".agents/skills/global/SKILL.md",
            &skill_frontmatter("global", "Global skill"),
        );

        let sandbox = LocalFsSandbox::new(home.display().to_string());
        let discovery = poll_ready(discover_agent_skills(
            &sandbox,
            AgentSkillDiscoveryOptions::new(project.display().to_string()),
        ));

        assert_eq!(
            discovery
                .skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["review", "commit", "global"]
        );

        let diagnostic_kinds = discovery
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.kind.clone())
            .collect::<Vec<_>>();
        assert!(diagnostic_kinds.contains(&AgentSkillDiscoveryDiagnosticKind::HiddenSkill));
        assert!(diagnostic_kinds.contains(&AgentSkillDiscoveryDiagnosticKind::MissingSkillFile));
        assert!(diagnostic_kinds.contains(&AgentSkillDiscoveryDiagnosticKind::BuiltInName));
        assert!(diagnostic_kinds.contains(&AgentSkillDiscoveryDiagnosticKind::DisabledSkill));
        assert!(diagnostic_kinds.contains(&AgentSkillDiscoveryDiagnosticKind::DuplicateName));

        fs::remove_dir_all(root).expect("temp root removed");
    }

    #[test]
    fn open_plugin_default_discovery_namespaces_skills() {
        let root = temp_root();
        let plugin = root.join("reports-plugin");

        write_plugin_manifest(
            &plugin,
            r#"{"name":"reports-plugin","version":"1.0.0","description":"Reports"}"#,
        );
        write_skill(
            &plugin,
            "skills/summarize/SKILL.md",
            &skill_frontmatter("summarize", "Summarize reports"),
        );

        let sandbox = LocalFsSandbox::new(root.join("home").display().to_string());
        let discovery = poll_ready(discover_open_plugin_agent_skills(
            &sandbox,
            &[plugin.display().to_string()],
        ));

        assert_eq!(discovery.diagnostics, Vec::new());
        assert_eq!(
            discovery
                .skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["reports-plugin:summarize"]
        );
        assert_eq!(
            discovery.skills[0].path,
            plugin.join("skills/summarize").display().to_string()
        );

        fs::remove_dir_all(root).expect("temp root removed");
    }

    #[test]
    fn open_plugin_manifest_skill_paths_override_default() {
        let root = temp_root();
        let plugin = root.join("reports-plugin");

        write_plugin_manifest(
            &plugin,
            r#"{"name":"reports-plugin","skills":"./custom-skills/"}"#,
        );
        write_skill(
            &plugin,
            "skills/summarize/SKILL.md",
            &skill_frontmatter("summarize", "Default summarize"),
        );
        write_skill(
            &plugin,
            "custom-skills/deploy/SKILL.md",
            &skill_frontmatter("deploy", "Custom deploy"),
        );

        let sandbox = LocalFsSandbox::new(root.join("home").display().to_string());
        let discovery = poll_ready(discover_open_plugin_agent_skills(
            &sandbox,
            &[plugin.display().to_string()],
        ));

        assert_eq!(
            discovery
                .skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["reports-plugin:deploy"]
        );

        fs::remove_dir_all(root).expect("temp root removed");
    }

    #[test]
    fn open_plugin_manifest_can_explicitly_retain_default_skills() {
        let root = temp_root();
        let plugin = root.join("reports-plugin");

        write_plugin_manifest(
            &plugin,
            r#"{"name":"reports-plugin","skills":{"paths":["./skills/","./custom-skills/"]}}"#,
        );
        write_skill(
            &plugin,
            "skills/summarize/SKILL.md",
            &skill_frontmatter("summarize", "Default summarize"),
        );
        write_skill(
            &plugin,
            "custom-skills/deploy/SKILL.md",
            &skill_frontmatter("deploy", "Custom deploy"),
        );

        let sandbox = LocalFsSandbox::new(root.join("home").display().to_string());
        let discovery = poll_ready(discover_open_plugin_agent_skills(
            &sandbox,
            &[plugin.display().to_string()],
        ));

        assert_eq!(
            discovery
                .skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["reports-plugin:summarize", "reports-plugin:deploy"]
        );

        fs::remove_dir_all(root).expect("temp root removed");
    }

    #[test]
    fn open_plugin_bad_paths_are_diagnostics_without_default_fallback() {
        let root = temp_root();
        let plugin = root.join("reports-plugin");

        write_plugin_manifest(&plugin, r#"{"name":"reports-plugin","skills":"../shared"}"#);
        write_skill(
            &plugin,
            "skills/summarize/SKILL.md",
            &skill_frontmatter("summarize", "Default summarize"),
        );

        let sandbox = LocalFsSandbox::new(root.join("home").display().to_string());
        let discovery = poll_ready(discover_open_plugin_agent_skills(
            &sandbox,
            &[plugin.display().to_string()],
        ));

        assert!(discovery.skills.is_empty());
        assert_eq!(discovery.diagnostics.len(), 1);
        assert_eq!(
            discovery.diagnostics[0].kind,
            AgentSkillDiscoveryDiagnosticKind::InvalidPluginPath
        );

        fs::remove_dir_all(root).expect("temp root removed");
    }

    #[test]
    fn open_plugin_invalid_manifest_is_non_fatal() {
        let root = temp_root();
        let broken = root.join("broken-plugin");
        let valid = root.join("valid-plugin");

        write_plugin_manifest(&broken, r#"{"name":"Broken Plugin"}"#);
        write_plugin_manifest(&valid, r#"{"name":"valid-plugin"}"#);
        write_skill(
            &valid,
            "skills/deploy/SKILL.md",
            &skill_frontmatter("deploy", "Deploy"),
        );

        let sandbox = LocalFsSandbox::new(root.join("home").display().to_string());
        let discovery = poll_ready(discover_open_plugin_agent_skills(
            &sandbox,
            &[broken.display().to_string(), valid.display().to_string()],
        ));

        assert_eq!(
            discovery
                .skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["valid-plugin:deploy"]
        );
        assert_eq!(discovery.diagnostics.len(), 1);
        assert_eq!(
            discovery.diagnostics[0].kind,
            AgentSkillDiscoveryDiagnosticKind::InvalidPluginManifest
        );

        fs::remove_dir_all(root).expect("temp root removed");
    }

    #[test]
    fn open_plugin_duplicate_names_are_first_wins_after_namespacing() {
        let root = temp_root();
        let first = root.join("first");
        let second = root.join("second");
        let third = root.join("third");

        write_plugin_manifest(&first, r#"{"name":"toolkit"}"#);
        write_plugin_manifest(&second, r#"{"name":"toolkit"}"#);
        write_plugin_manifest(&third, r#"{"name":"other-toolkit"}"#);
        write_skill(
            &first,
            "skills/review/SKILL.md",
            &skill_frontmatter("review", "First review"),
        );
        write_skill(
            &second,
            "skills/review/SKILL.md",
            &skill_frontmatter("review", "Second review"),
        );
        write_skill(
            &third,
            "skills/review/SKILL.md",
            &skill_frontmatter("review", "Third review"),
        );

        let sandbox = LocalFsSandbox::new(root.join("home").display().to_string());
        let discovery = poll_ready(discover_open_plugin_agent_skills(
            &sandbox,
            &[
                first.display().to_string(),
                second.display().to_string(),
                third.display().to_string(),
            ],
        ));

        assert_eq!(
            discovery
                .skills
                .iter()
                .map(|skill| (skill.name.as_str(), skill.description.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("toolkit:review", "First review"),
                ("other-toolkit:review", "Third review")
            ]
        );
        assert_eq!(
            discovery
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.kind.clone())
                .collect::<Vec<_>>(),
            vec![AgentSkillDiscoveryDiagnosticKind::DuplicateName]
        );
        assert_eq!(
            discovery.diagnostics[0].name.as_deref(),
            Some("toolkit:review")
        );

        fs::remove_dir_all(root).expect("temp root removed");
    }

    #[test]
    fn open_plugin_skills_are_additive_to_existing_discovery() {
        let root = temp_root();
        let project = root.join("project");
        let plugin = root.join("plugin");

        write_skill(
            &project,
            ".agents/skills/review/SKILL.md",
            &skill_frontmatter("review", "Project review"),
        );
        write_plugin_manifest(&plugin, r#"{"name":"toolkit"}"#);
        write_skill(
            &plugin,
            "skills/review/SKILL.md",
            &skill_frontmatter("review", "Plugin review"),
        );

        let sandbox = LocalFsSandbox::new(root.join("home").display().to_string());
        let discovery = poll_ready(discover_agent_skills(
            &sandbox,
            AgentSkillDiscoveryOptions::new(project.display().to_string())
                .with_include_global(false)
                .with_plugin_root(plugin.display().to_string()),
        ));

        assert_eq!(
            discovery
                .skills
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            vec!["review", "toolkit:review"]
        );

        fs::remove_dir_all(root).expect("temp root removed");
    }

    #[test]
    fn open_plugin_namespaced_slash_invocation_loads_plugin_skill_directory() {
        let root = temp_root();
        let plugin = root.join("plugin");

        write_plugin_manifest(&plugin, r#"{"name":"deploy-tools"}"#);
        write_skill(
            &plugin,
            "skills/deploy/SKILL.md",
            "---\nname: deploy\ndescription: Deploy app\n---\nDeploy $ARGUMENTS from here.",
        );

        assert_eq!(
            parse_agent_skill_slash_command(" /deploy-tools:deploy production"),
            Some(AgentSkillSlashCommand {
                skill: "deploy-tools:deploy".to_string(),
                arguments: Some("production".to_string()),
            })
        );

        let sandbox = LocalFsSandbox::new(root.join("home").display().to_string());
        let discovery = poll_ready(discover_open_plugin_agent_skills(
            &sandbox,
            &[plugin.display().to_string()],
        ));
        let instruction = poll_ready(invoke_agent_skill_slash_command(
            &sandbox,
            &discovery.skills,
            "/deploy-tools:deploy production",
        ))
        .expect("slash command parsed")
        .unwrap();

        assert_eq!(instruction.metadata.name, "deploy-tools:deploy");
        assert!(instruction.content.starts_with(&format!(
            "Skill directory: {}",
            plugin.join("skills/deploy").display()
        )));
        assert!(instruction.content.contains("Deploy production from here."));

        let loaded_prompt = build_loaded_agent_skills_prompt(&[instruction]).unwrap();
        assert!(loaded_prompt.contains("<deploy-tools:deploy>"));
        assert!(loaded_prompt.contains("Skill directory: "));

        fs::remove_dir_all(root).expect("temp root removed");
    }

    #[test]
    fn invoke_skill_injects_directory_and_substitutes_arguments() {
        let root = temp_root();
        let project = root.join("project");
        write_skill(
            &project,
            ".agents/skills/review/SKILL.md",
            "---\nname: review\ndescription: Review code\n---\nReview $ARGUMENTS now.",
        );

        let sandbox = LocalFsSandbox::new(root.join("home").display().to_string());
        let discovery = poll_ready(discover_agent_skills(
            &sandbox,
            AgentSkillDiscoveryOptions::new(project.display().to_string())
                .with_include_global(false),
        ));
        let instruction = poll_ready(invoke_agent_skill(
            &sandbox,
            &discovery.skills,
            "REVIEW",
            Some("abc123"),
            AgentSkillInvocationSource::User,
        ))
        .unwrap();

        assert!(instruction.content.starts_with("Skill directory: "));
        assert!(instruction.content.contains("Review abc123 now."));
        assert!(instruction.tagged_content().contains("<review>"));

        fs::remove_dir_all(root).expect("temp root removed");
    }

    #[test]
    fn read_errors_are_reported_without_aborting_discovery() {
        let sandbox = ReadErrorSandbox;
        let discovery = poll_ready(discover_agent_skills_in_directories(
            &sandbox,
            &["/skills".to_string()],
        ));

        assert!(discovery.skills.is_empty());
        assert_eq!(discovery.diagnostics.len(), 1);
        assert_eq!(
            discovery.diagnostics[0].kind,
            AgentSkillDiscoveryDiagnosticKind::SandboxReadError
        );
    }

    #[test]
    fn slash_command_parser_extracts_skill_and_arguments() {
        assert_eq!(
            parse_agent_skill_slash_command("   /review PR 123"),
            Some(AgentSkillSlashCommand {
                skill: "review".to_string(),
                arguments: Some("PR 123".to_string()),
            })
        );
        assert_eq!(parse_agent_skill_slash_command("please /review"), None);
    }

    #[test]
    fn skill_tool_emits_status_and_final_skill_content() {
        let root = temp_root();
        let project = root.join("project");
        write_skill(
            &project,
            ".agents/skills/review/SKILL.md",
            "---\nname: review\ndescription: Review code\nallowed-tools: Read\n---\nReview $ARGUMENTS.",
        );

        let sandbox: Arc<dyn ExperimentalSandbox> =
            Arc::new(LocalFsSandbox::new(root.join("home").display().to_string()));
        let discovery = poll_ready(discover_agent_skills(
            sandbox.as_ref(),
            AgentSkillDiscoveryOptions::new(project.display().to_string())
                .with_include_global(false),
        ));
        let tool = agent_skill_tool(discovery.skills);
        let outputs = poll_ready(
            tool.execute_outputs(
                json!({ "skill": "review", "args": "abc123" }),
                ToolExecutionOptions::new("call-1", Vec::new())
                    .with_experimental_sandbox(Arc::clone(&sandbox)),
            )
            .expect("skill tool has executor"),
        )
        .unwrap();

        assert!(matches!(outputs[0], ExecuteToolOutput::Preliminary { .. }));
        let ExecuteToolOutput::Final { output } = &outputs[1] else {
            panic!("expected final output");
        };
        assert_eq!(output["success"], json!(true));
        assert!(
            output["content"]
                .as_str()
                .unwrap()
                .contains("Review abc123.")
        );
        assert_eq!(output["allowedTools"], json!(["Read"]));

        fs::remove_dir_all(root).expect("temp root removed");
    }

    #[test]
    fn loaded_skill_allowed_tools_are_deduplicated() {
        let metadata = AgentSkillMetadata {
            name: "review".to_string(),
            description: "Review".to_string(),
            path: "/skills/review".to_string(),
            filename: "SKILL.md".to_string(),
            options: AgentSkillOptions {
                allowed_tools: vec!["Read".to_string(), "Bash".to_string(), "Read".to_string()],
                ..AgentSkillOptions::default()
            },
        };
        let instruction = AgentSkillInstruction {
            metadata,
            content: "Body".to_string(),
            arguments: None,
        };

        assert_eq!(
            allowed_tools_for_loaded_agent_skills(&[instruction]),
            Some(vec!["Bash".to_string(), "Read".to_string()])
        );
    }
}
