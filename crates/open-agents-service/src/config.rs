use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;

use open_agents_sandbox::{GitRemoteActionMode, JUST_BASH_DEFAULT_WORKING_DIRECTORY};
use open_agents_slack::SlackIngressMode;

use crate::plugin::{
    OPEN_AGENTS_PLUGIN_DATA_DIR_ENV, OPEN_AGENTS_PLUGIN_ROOTS_ENV, OpenPluginCatalog,
};

const DEFAULT_GATEWAY_MODEL_ID: &str = "openai/gpt-4.1-mini";
const DEFAULT_GATEWAY_MAX_STEPS: usize = 8;
const DEFAULT_GATEWAY_MAX_OUTPUT_TOKENS: u64 = 2048;

/// Storage backend selected for the service.
#[derive(Clone, PartialEq, Eq)]
pub enum StateStore {
    /// In-process memory store for local development and deterministic fixtures.
    Memory,
    /// Postgres-backed durable store. The adapter client is wired by a later slice.
    Postgres { database_url: String },
}

impl StateStore {
    /// Stable operator-facing label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Postgres { .. } => "postgres",
        }
    }
}

impl fmt::Debug for StateStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory => formatter.write_str("Memory"),
            Self::Postgres { .. } => formatter
                .debug_struct("Postgres")
                .field("database_url", &"<redacted>")
                .finish(),
        }
    }
}

/// Sandbox backend selected for agent tool execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxMode {
    /// In-process Just Bash virtual filesystem backend.
    JustBash,
    /// Local filesystem/shell execution boundary for fixtures and local runs.
    Local { root: PathBuf },
    /// Vercel Sandbox cloud backend.
    Vercel {
        base_snapshot_id: Option<String>,
        sandbox_name: Option<String>,
    },
}

impl SandboxMode {
    /// Stable operator-facing label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::JustBash => "just-bash",
            Self::Local { .. } => "local",
            Self::Vercel { .. } => "vercel",
        }
    }
}

/// Runtime agent implementation selected for Slack-started runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRuntimeMode {
    /// Deterministic scripted runtime used by fixtures and no-credential local runs.
    Fixture,
    /// Real model-backed Open Agent using Vercel AI Gateway credentials.
    Gateway,
}

impl AgentRuntimeMode {
    /// Stable operator-facing label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fixture => "fixture",
            Self::Gateway => "gateway",
        }
    }
}

/// Tool approval policy used by the Gateway-backed agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentToolApprovalMode {
    /// Match Open Agents defaults for sensitive file/network/shell operations.
    Sensitive,
    /// Execute model-requested tools without pausing for approval.
    Never,
    /// Ask for approval for every tool that exposes an approval check.
    Always,
}

impl AgentToolApprovalMode {
    /// Stable operator-facing label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Sensitive => "sensitive",
            Self::Never => "never",
            Self::Always => "always",
        }
    }
}

/// Optional sandbox-bound finish automation after a run completes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFinishActions {
    /// Whether to inspect/run git finish automation at all.
    pub git_enabled: bool,
    /// Commit message used when the sandbox repository is dirty.
    pub commit_message: Option<String>,
    /// Optional push mode.
    pub push_mode: GitRemoteActionMode,
    /// Optional pull-request creation mode.
    pub pull_request_mode: GitRemoteActionMode,
    /// Pull-request base branch.
    pub pull_request_base: String,
    /// Pull-request title.
    pub pull_request_title: String,
    /// Pull-request body.
    pub pull_request_body: String,
    /// Optional owner/repo target for pull-request creation.
    pub pull_request_repository: Option<String>,
}

impl Default for AgentFinishActions {
    fn default() -> Self {
        Self {
            git_enabled: false,
            commit_message: None,
            push_mode: GitRemoteActionMode::Disabled,
            pull_request_mode: GitRemoteActionMode::Disabled,
            pull_request_base: "main".to_string(),
            pull_request_title: "Open Agents changes".to_string(),
            pull_request_body: "Created by the Open Agents Slack remote agent.".to_string(),
            pull_request_repository: None,
        }
    }
}

/// Deployable service configuration.
#[derive(Clone, PartialEq, Eq)]
pub struct OpenAgentsServiceConfig {
    bind_addr: SocketAddr,
    state_store: StateStore,
    slack_ingress: SlackIngressMode,
    slack_bot_token: String,
    slack_signing_secret: String,
    slack_app_token: Option<String>,
    slack_api_url: Option<String>,
    sandbox: SandboxMode,
    runtime: AgentRuntimeMode,
    model_api_key: Option<String>,
    model_id: String,
    model_max_steps: usize,
    model_max_output_tokens: u64,
    tool_approval: AgentToolApprovalMode,
    finish_actions: AgentFinishActions,
    github_token: Option<String>,
    plugin_catalog: OpenPluginCatalog,
}

impl fmt::Debug for OpenAgentsServiceConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenAgentsServiceConfig")
            .field("bind_addr", &self.bind_addr)
            .field("state_store", &self.state_store)
            .field("slack_ingress", &self.slack_ingress)
            .field("slack_bot_token", &"<redacted>")
            .field("slack_signing_secret", &"<redacted>")
            .field(
                "slack_app_token",
                &self.slack_app_token.as_ref().map(|_| "<redacted>"),
            )
            .field("slack_api_url", &self.slack_api_url)
            .field("sandbox", &self.sandbox)
            .field("runtime", &self.runtime)
            .field(
                "model_api_key",
                &self.model_api_key.as_ref().map(|_| "<redacted>"),
            )
            .field("model_id", &self.model_id)
            .field("model_max_steps", &self.model_max_steps)
            .field("model_max_output_tokens", &self.model_max_output_tokens)
            .field("tool_approval", &self.tool_approval)
            .field("finish_actions", &self.finish_actions)
            .field(
                "github_token",
                &self.github_token.as_ref().map(|_| "<redacted>"),
            )
            .field("plugin_catalog", &self.plugin_catalog)
            .finish()
    }
}

impl OpenAgentsServiceConfig {
    /// Load and validate configuration from process environment variables.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_reader(|name| std::env::var(name).ok())
    }

    /// Load configuration using a caller-provided variable reader.
    ///
    /// Tests use this to assert validation behavior without mutating the
    /// process environment.
    pub fn from_reader(
        mut read_var: impl FnMut(&'static str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        let bind_addr = match present(read_var("OPEN_AGENTS_BIND_ADDR")) {
            Some(raw) => raw
                .parse::<SocketAddr>()
                .map_err(|_| ConfigError::InvalidVar {
                    name: "OPEN_AGENTS_BIND_ADDR",
                    value: raw,
                    expected: "a socket address such as 127.0.0.1:8080",
                })?,
            None => "127.0.0.1:8080"
                .parse()
                .expect("default bind address should parse"),
        };

        let slack_bot_token = required(&mut read_var, open_agents_slack::SLACK_BOT_TOKEN_ENV)?;
        let slack_signing_secret =
            required(&mut read_var, open_agents_slack::SLACK_SIGNING_SECRET_ENV)?;
        let slack_app_token = present(read_var(open_agents_slack::SLACK_APP_TOKEN_ENV));
        let slack_api_url = present(read_var("OPEN_AGENTS_SLACK_API_URL"))
            .or_else(|| present(read_var("SLACK_API_URL")));

        let slack_ingress =
            parse_slack_ingress(present(read_var("OPEN_AGENTS_SLACK_INGRESS")).as_deref())?;
        if slack_ingress == SlackIngressMode::SocketMode && slack_app_token.is_none() {
            return Err(ConfigError::MissingVar(
                open_agents_slack::SLACK_APP_TOKEN_ENV,
            ));
        }

        let state_store = parse_state_store(
            present(read_var("OPEN_AGENTS_STATE")).as_deref(),
            &mut read_var,
        )?;
        let sandbox = parse_sandbox(
            present(read_var("OPEN_AGENTS_SANDBOX")).as_deref(),
            &mut read_var,
        )?;

        let model_api_key = present(read_var("AI_GATEWAY_API_KEY"))
            .or_else(|| present(read_var("AI_SDK_RUST_AI_GATEWAY_API_KEY")));
        let runtime = parse_runtime(
            present(read_var("OPEN_AGENTS_RUNTIME")).as_deref(),
            model_api_key.is_some(),
        )?;
        if runtime == AgentRuntimeMode::Gateway && model_api_key.is_none() {
            return Err(ConfigError::MissingVar("AI_GATEWAY_API_KEY"));
        }
        let model_id = present(read_var("OPEN_AGENTS_MODEL"))
            .or_else(|| present(read_var("AI_GATEWAY_MODEL")))
            .or_else(|| present(read_var("AI_SDK_RUST_GATEWAY_MODEL")))
            .unwrap_or_else(|| DEFAULT_GATEWAY_MODEL_ID.to_string());
        let model_max_steps = parse_usize(
            present(read_var("OPEN_AGENTS_MODEL_MAX_STEPS")).as_deref(),
            "OPEN_AGENTS_MODEL_MAX_STEPS",
            DEFAULT_GATEWAY_MAX_STEPS,
        )?;
        let model_max_output_tokens = parse_u64(
            present(read_var("OPEN_AGENTS_MODEL_MAX_OUTPUT_TOKENS")).as_deref(),
            "OPEN_AGENTS_MODEL_MAX_OUTPUT_TOKENS",
            DEFAULT_GATEWAY_MAX_OUTPUT_TOKENS,
        )?;
        let tool_approval =
            parse_tool_approval(present(read_var("OPEN_AGENTS_TOOL_APPROVAL")).as_deref())?;
        let finish_actions = parse_finish_actions(&mut read_var)?;
        let github_token = present(read_var("OPEN_AGENTS_GITHUB_TOKEN"))
            .or_else(|| present(read_var("GITHUB_TOKEN")))
            .or_else(|| present(read_var("GH_TOKEN")));
        let plugin_catalog = OpenPluginCatalog::from_env_values(
            present(read_var(OPEN_AGENTS_PLUGIN_ROOTS_ENV)),
            present(read_var(OPEN_AGENTS_PLUGIN_DATA_DIR_ENV)),
        )
        .map_err(|error| ConfigError::PluginConfig(error.to_string()))?;

        Ok(Self {
            bind_addr,
            state_store,
            slack_ingress,
            slack_bot_token,
            slack_signing_secret,
            slack_app_token,
            slack_api_url,
            sandbox,
            runtime,
            model_api_key,
            model_id,
            model_max_steps,
            model_max_output_tokens,
            tool_approval,
            finish_actions,
            github_token,
            plugin_catalog,
        })
    }

    /// Deterministic local fixture configuration that does not require secrets.
    pub fn fixture() -> Self {
        Self {
            bind_addr: "127.0.0.1:0"
                .parse()
                .expect("fixture bind address should parse"),
            state_store: StateStore::Memory,
            slack_ingress: SlackIngressMode::Webhook,
            slack_bot_token: "xoxb-fixture".to_string(),
            slack_signing_secret: "fixture-signing-secret".to_string(),
            slack_app_token: None,
            slack_api_url: None,
            sandbox: SandboxMode::JustBash,
            runtime: AgentRuntimeMode::Fixture,
            model_api_key: None,
            model_id: DEFAULT_GATEWAY_MODEL_ID.to_string(),
            model_max_steps: DEFAULT_GATEWAY_MAX_STEPS,
            model_max_output_tokens: DEFAULT_GATEWAY_MAX_OUTPUT_TOKENS,
            tool_approval: AgentToolApprovalMode::Sensitive,
            finish_actions: AgentFinishActions::default(),
            github_token: None,
            plugin_catalog: OpenPluginCatalog::default(),
        }
    }

    /// Address the health server should bind.
    pub fn bind_addr(&self) -> SocketAddr {
        self.bind_addr
    }

    /// Selected state backend.
    pub fn state_store(&self) -> &StateStore {
        &self.state_store
    }

    /// Selected Slack ingress transport.
    pub fn slack_ingress(&self) -> SlackIngressMode {
        self.slack_ingress
    }

    /// Slack bot token. Callers must not log this value.
    pub fn slack_bot_token(&self) -> &str {
        &self.slack_bot_token
    }

    /// Slack signing secret. Callers must not log this value.
    pub fn slack_signing_secret(&self) -> &str {
        &self.slack_signing_secret
    }

    /// Slack app-level token for Socket Mode. Callers must not log this value.
    pub fn slack_app_token(&self) -> Option<&str> {
        self.slack_app_token.as_deref()
    }

    /// Optional Slack Web API base URL override.
    pub fn slack_api_url(&self) -> Option<&str> {
        self.slack_api_url.as_deref()
    }

    /// Selected sandbox backend.
    pub fn sandbox(&self) -> &SandboxMode {
        &self.sandbox
    }

    /// Selected agent runtime.
    pub fn runtime(&self) -> AgentRuntimeMode {
        self.runtime
    }

    /// Optional model credential. Callers must not log this value.
    pub fn model_api_key(&self) -> Option<&str> {
        self.model_api_key.as_deref()
    }

    /// Gateway model id used by real model-backed runs.
    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Maximum model/tool-loop steps for Gateway-backed runs.
    pub fn model_max_steps(&self) -> usize {
        self.model_max_steps
    }

    /// Maximum model output tokens for Gateway-backed runs.
    pub fn model_max_output_tokens(&self) -> u64 {
        self.model_max_output_tokens
    }

    /// Tool approval policy for Gateway-backed runs.
    pub fn tool_approval(&self) -> AgentToolApprovalMode {
        self.tool_approval
    }

    /// Optional finish automation settings.
    pub fn finish_actions(&self) -> &AgentFinishActions {
        &self.finish_actions
    }

    /// Optional GitHub token exposed only to sandbox commands for clone/push/PR workflows.
    pub fn github_token(&self) -> Option<&str> {
        self.github_token.as_deref()
    }

    /// Open Plugin packages loaded for this service.
    pub fn plugin_catalog(&self) -> &OpenPluginCatalog {
        &self.plugin_catalog
    }

    /// Redacted summary safe to print in operator logs.
    pub fn operator_summary(&self) -> String {
        format!(
            "bind_addr={} state={} slack_ingress={} sandbox={} sandbox_working_directory={} runtime={} slack_bot_token={} signing_secret={} socket_mode_token={} slack_api_url={} model_credential={} model={} model_max_steps={} model_max_output_tokens={} tool_approval={} finish_git={} finish_push={} finish_pr={} github_token={} plugin_roots={} plugins={} plugin_skills={} plugin_mcp_servers={} plugin_data_dir={} plugin_diagnostics={}",
            self.bind_addr,
            self.state_store.label(),
            slack_ingress_label(self.slack_ingress),
            self.sandbox.label(),
            sandbox_working_directory_label(&self.sandbox),
            self.runtime.label(),
            present_label(Some(&self.slack_bot_token)),
            present_label(Some(&self.slack_signing_secret)),
            present_label(self.slack_app_token.as_ref()),
            present_label(self.slack_api_url.as_ref()),
            present_label(self.model_api_key.as_ref()),
            self.model_id,
            self.model_max_steps,
            self.model_max_output_tokens,
            self.tool_approval.label(),
            self.finish_actions.git_enabled,
            git_remote_action_label(self.finish_actions.push_mode),
            git_remote_action_label(self.finish_actions.pull_request_mode),
            present_label(self.github_token.as_ref()),
            self.plugin_catalog.roots().len(),
            self.plugin_catalog.packages().len(),
            self.plugin_catalog.runtime_skills().len(),
            self.plugin_catalog.runtime_mcp_servers().len(),
            if self.plugin_catalog.data_dir().is_some() {
                "present"
            } else {
                "missing"
            },
            self.plugin_catalog.diagnostics().len(),
        )
    }
}

/// Configuration validation error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A required environment variable was absent or blank.
    MissingVar(&'static str),
    /// A variable was present but did not match the expected shape.
    InvalidVar {
        name: &'static str,
        value: String,
        expected: &'static str,
    },
    /// Open Plugin package configuration failed to load.
    PluginConfig(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingVar(name) => {
                write!(formatter, "missing required environment variable {name}")
            }
            Self::InvalidVar {
                name,
                value,
                expected,
            } => write!(
                formatter,
                "{name}={value:?} is invalid: expected {expected}"
            ),
            Self::PluginConfig(message) => {
                write!(formatter, "Open Plugin config is invalid: {message}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

fn required(
    read_var: &mut impl FnMut(&'static str) -> Option<String>,
    name: &'static str,
) -> Result<String, ConfigError> {
    present(read_var(name)).ok_or(ConfigError::MissingVar(name))
}

fn present(value: Option<String>) -> Option<String> {
    value.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn present_label(value: Option<&String>) -> &'static str {
    if value.is_some_and(|raw| !raw.is_empty()) {
        "present"
    } else {
        "missing"
    }
}

fn slack_ingress_label(mode: SlackIngressMode) -> &'static str {
    match mode {
        SlackIngressMode::Webhook => "webhook",
        SlackIngressMode::SocketMode => "socket-mode",
    }
}

fn parse_slack_ingress(raw: Option<&str>) -> Result<SlackIngressMode, ConfigError> {
    match raw.unwrap_or("webhook") {
        "events" | "http" | "http-events" | "webhook" => Ok(SlackIngressMode::Webhook),
        "socket" | "socket-mode" => Ok(SlackIngressMode::SocketMode),
        value => Err(ConfigError::InvalidVar {
            name: "OPEN_AGENTS_SLACK_INGRESS",
            value: value.to_string(),
            expected: "webhook or socket-mode",
        }),
    }
}

fn parse_state_store(
    raw: Option<&str>,
    read_var: &mut impl FnMut(&'static str) -> Option<String>,
) -> Result<StateStore, ConfigError> {
    match raw.unwrap_or("memory") {
        "memory" => Ok(StateStore::Memory),
        "postgres" | "pg" => Ok(StateStore::Postgres {
            database_url: present(read_var("OPEN_AGENTS_STATE_URL"))
                .or_else(|| present(read_var("POSTGRES_URL")))
                .ok_or(ConfigError::MissingVar("OPEN_AGENTS_STATE_URL"))?,
        }),
        value => Err(ConfigError::InvalidVar {
            name: "OPEN_AGENTS_STATE",
            value: value.to_string(),
            expected: "memory or postgres",
        }),
    }
}

fn parse_sandbox(
    raw: Option<&str>,
    read_var: &mut impl FnMut(&'static str) -> Option<String>,
) -> Result<SandboxMode, ConfigError> {
    match raw.unwrap_or("just-bash") {
        "just-bash" | "just_bash" | "justbash" => Ok(SandboxMode::JustBash),
        "local" => Ok(SandboxMode::Local {
            root: present(read_var("OPEN_AGENTS_SANDBOX_ROOT"))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".")),
        }),
        "vercel" => Ok(SandboxMode::Vercel {
            base_snapshot_id: present(read_var(
                open_agents_sandbox::VERCEL_SANDBOX_BASE_SNAPSHOT_ID_ENV,
            )),
            sandbox_name: present(read_var(open_agents_sandbox::VERCEL_SANDBOX_NAME_ENV)),
        }),
        value => Err(ConfigError::InvalidVar {
            name: "OPEN_AGENTS_SANDBOX",
            value: value.to_string(),
            expected: "just-bash, local, or vercel",
        }),
    }
}

fn sandbox_working_directory_label(sandbox: &SandboxMode) -> String {
    match sandbox {
        SandboxMode::JustBash => JUST_BASH_DEFAULT_WORKING_DIRECTORY.to_string(),
        SandboxMode::Local { root } => root.display().to_string(),
        SandboxMode::Vercel { .. } => "/vercel/sandbox".to_string(),
    }
}

fn parse_runtime(
    raw: Option<&str>,
    has_model_api_key: bool,
) -> Result<AgentRuntimeMode, ConfigError> {
    match raw.unwrap_or("auto") {
        "auto" => Ok(if has_model_api_key {
            AgentRuntimeMode::Gateway
        } else {
            AgentRuntimeMode::Fixture
        }),
        "fixture" | "scripted" | "local" => Ok(AgentRuntimeMode::Fixture),
        "gateway" | "ai-gateway" | "model" => Ok(AgentRuntimeMode::Gateway),
        value => Err(ConfigError::InvalidVar {
            name: "OPEN_AGENTS_RUNTIME",
            value: value.to_string(),
            expected: "auto, fixture, or gateway",
        }),
    }
}

fn parse_tool_approval(raw: Option<&str>) -> Result<AgentToolApprovalMode, ConfigError> {
    match raw.unwrap_or("sensitive") {
        "sensitive" | "default" => Ok(AgentToolApprovalMode::Sensitive),
        "never" | "off" | "none" => Ok(AgentToolApprovalMode::Never),
        "always" | "all" => Ok(AgentToolApprovalMode::Always),
        value => Err(ConfigError::InvalidVar {
            name: "OPEN_AGENTS_TOOL_APPROVAL",
            value: value.to_string(),
            expected: "sensitive, never, or always",
        }),
    }
}

fn parse_finish_actions(
    read_var: &mut impl FnMut(&'static str) -> Option<String>,
) -> Result<AgentFinishActions, ConfigError> {
    let raw_git = present(read_var("OPEN_AGENTS_GIT_FINISH"));
    let commit_message = present(read_var("OPEN_AGENTS_GIT_FINISH_COMMIT_MESSAGE"));
    let push_mode = parse_git_remote_action(
        present(read_var("OPEN_AGENTS_GIT_FINISH_PUSH")).as_deref(),
        "OPEN_AGENTS_GIT_FINISH_PUSH",
    )?;
    let pull_request_mode = parse_git_remote_action(
        present(read_var("OPEN_AGENTS_GIT_FINISH_PR")).as_deref(),
        "OPEN_AGENTS_GIT_FINISH_PR",
    )?;
    let mut actions = AgentFinishActions {
        git_enabled: parse_finish_git_enabled(raw_git.as_deref())?,
        commit_message,
        push_mode,
        pull_request_mode,
        pull_request_base: present(read_var("OPEN_AGENTS_GIT_FINISH_PR_BASE"))
            .unwrap_or_else(|| "main".to_string()),
        pull_request_title: present(read_var("OPEN_AGENTS_GIT_FINISH_PR_TITLE"))
            .unwrap_or_else(|| "Open Agents changes".to_string()),
        pull_request_body: present(read_var("OPEN_AGENTS_GIT_FINISH_PR_BODY"))
            .unwrap_or_else(|| "Created by the Open Agents Slack remote agent.".to_string()),
        pull_request_repository: present(read_var("OPEN_AGENTS_GIT_FINISH_PR_REPOSITORY")),
    };

    if actions.commit_message.is_some()
        || actions.push_mode != GitRemoteActionMode::Disabled
        || actions.pull_request_mode != GitRemoteActionMode::Disabled
    {
        actions.git_enabled = true;
    }

    Ok(actions)
}

fn parse_finish_git_enabled(raw: Option<&str>) -> Result<bool, ConfigError> {
    match raw.unwrap_or("disabled") {
        "disabled" | "false" | "off" | "none" => Ok(false),
        "report" | "true" | "on" | "enabled" => Ok(true),
        value => Err(ConfigError::InvalidVar {
            name: "OPEN_AGENTS_GIT_FINISH",
            value: value.to_string(),
            expected: "disabled, report, or true",
        }),
    }
}

fn parse_git_remote_action(
    raw: Option<&str>,
    name: &'static str,
) -> Result<GitRemoteActionMode, ConfigError> {
    match raw.unwrap_or("disabled") {
        "disabled" | "false" | "off" | "none" => Ok(GitRemoteActionMode::Disabled),
        "dry-run" | "dry_run" | "dryrun" => Ok(GitRemoteActionMode::DryRun),
        "execute" | "true" | "on" => Ok(GitRemoteActionMode::Execute),
        value => Err(ConfigError::InvalidVar {
            name,
            value: value.to_string(),
            expected: "disabled, dry-run, or execute",
        }),
    }
}

fn git_remote_action_label(mode: GitRemoteActionMode) -> &'static str {
    match mode {
        GitRemoteActionMode::Disabled => "disabled",
        GitRemoteActionMode::DryRun => "dry-run",
        GitRemoteActionMode::Execute => "execute",
    }
}

fn parse_usize(
    raw: Option<&str>,
    name: &'static str,
    default: usize,
) -> Result<usize, ConfigError> {
    match raw {
        Some(value) => value.parse::<usize>().map_err(|_| ConfigError::InvalidVar {
            name,
            value: value.to_string(),
            expected: "a positive integer",
        }),
        None => Ok(default),
    }
}

fn parse_u64(raw: Option<&str>, name: &'static str, default: u64) -> Result<u64, ConfigError> {
    match raw {
        Some(value) => value.parse::<u64>().map_err(|_| ConfigError::InvalidVar {
            name,
            value: value.to_string(),
            expected: "a positive integer",
        }),
        None => Ok(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::env;
    use std::path::PathBuf;

    fn load(
        pairs: &[(&'static str, &'static str)],
    ) -> Result<OpenAgentsServiceConfig, ConfigError> {
        let vars: HashMap<&'static str, String> = pairs
            .iter()
            .map(|(key, value)| (*key, (*value).to_string()))
            .collect();
        OpenAgentsServiceConfig::from_reader(|name| vars.get(name).cloned())
    }

    fn load_owned(
        pairs: &[(&'static str, String)],
    ) -> Result<OpenAgentsServiceConfig, ConfigError> {
        let vars: HashMap<&'static str, String> = pairs.iter().cloned().collect();
        OpenAgentsServiceConfig::from_reader(|name| vars.get(name).cloned())
    }

    fn plugin_fixture_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/open-plugin/minimal")
    }

    #[test]
    fn from_reader_defaults_to_memory_webhook_and_just_bash_sandbox() {
        let config = load(&[
            ("SLACK_BOT_TOKEN", "xoxb-test"),
            ("SLACK_SIGNING_SECRET", "secret"),
        ])
        .unwrap();

        assert_eq!(config.bind_addr().to_string(), "127.0.0.1:8080");
        assert_eq!(config.state_store().label(), "memory");
        assert_eq!(config.slack_ingress(), SlackIngressMode::Webhook);
        assert_eq!(config.slack_api_url(), None);
        assert_eq!(config.sandbox().label(), "just-bash");
        assert_eq!(config.runtime(), AgentRuntimeMode::Fixture);
        assert_eq!(config.model_id(), DEFAULT_GATEWAY_MODEL_ID);
    }

    #[test]
    fn from_reader_accepts_open_agents_slack_api_url() {
        let config = load(&[
            ("SLACK_BOT_TOKEN", "xoxb-test"),
            ("SLACK_SIGNING_SECRET", "secret"),
            ("OPEN_AGENTS_SLACK_API_URL", "http://127.0.0.1:4003/api"),
        ])
        .unwrap();

        assert_eq!(config.slack_api_url(), Some("http://127.0.0.1:4003/api"));
    }

    #[test]
    fn from_reader_accepts_slack_api_url_alias() {
        let config = load(&[
            ("SLACK_BOT_TOKEN", "xoxb-test"),
            ("SLACK_SIGNING_SECRET", "secret"),
            ("SLACK_API_URL", "http://127.0.0.1:4003/api"),
        ])
        .unwrap();

        assert_eq!(config.slack_api_url(), Some("http://127.0.0.1:4003/api"));
    }

    #[test]
    fn from_reader_requires_slack_bot_token() {
        let err = load(&[("SLACK_SIGNING_SECRET", "secret")]).unwrap_err();
        assert_eq!(err, ConfigError::MissingVar("SLACK_BOT_TOKEN"));
    }

    #[test]
    fn from_reader_requires_signing_secret() {
        let err = load(&[("SLACK_BOT_TOKEN", "xoxb-test")]).unwrap_err();
        assert_eq!(err, ConfigError::MissingVar("SLACK_SIGNING_SECRET"));
    }

    #[test]
    fn socket_mode_requires_app_token() {
        let err = load(&[
            ("SLACK_BOT_TOKEN", "xoxb-test"),
            ("SLACK_SIGNING_SECRET", "secret"),
            ("OPEN_AGENTS_SLACK_INGRESS", "socket-mode"),
        ])
        .unwrap_err();
        assert_eq!(err, ConfigError::MissingVar("SLACK_APP_TOKEN"));
    }

    #[test]
    fn from_reader_auto_selects_gateway_when_gateway_key_is_present() {
        let config = load(&[
            ("SLACK_BOT_TOKEN", "xoxb-test"),
            ("SLACK_SIGNING_SECRET", "secret"),
            ("AI_GATEWAY_API_KEY", "gateway-key"),
            ("AI_GATEWAY_MODEL", "openai/gpt-test"),
            ("OPEN_AGENTS_MODEL_MAX_STEPS", "4"),
            ("OPEN_AGENTS_MODEL_MAX_OUTPUT_TOKENS", "512"),
            ("OPEN_AGENTS_TOOL_APPROVAL", "never"),
            ("GITHUB_TOKEN", "ghp-test"),
        ])
        .unwrap();

        assert_eq!(config.runtime(), AgentRuntimeMode::Gateway);
        assert_eq!(config.model_api_key(), Some("gateway-key"));
        assert_eq!(config.model_id(), "openai/gpt-test");
        assert_eq!(config.model_max_steps(), 4);
        assert_eq!(config.model_max_output_tokens(), 512);
        assert_eq!(config.tool_approval(), AgentToolApprovalMode::Never);
        assert_eq!(config.github_token(), Some("ghp-test"));
    }

    #[test]
    fn from_reader_parses_finish_git_actions() {
        let config = load(&[
            ("SLACK_BOT_TOKEN", "xoxb-test"),
            ("SLACK_SIGNING_SECRET", "secret"),
            ("OPEN_AGENTS_GIT_FINISH", "report"),
            (
                "OPEN_AGENTS_GIT_FINISH_COMMIT_MESSAGE",
                "feat: apply agent changes",
            ),
            ("OPEN_AGENTS_GIT_FINISH_PUSH", "dry-run"),
            ("OPEN_AGENTS_GIT_FINISH_PR", "dry-run"),
            ("OPEN_AGENTS_GIT_FINISH_PR_REPOSITORY", "acme/service"),
        ])
        .unwrap();

        assert!(config.finish_actions().git_enabled);
        assert_eq!(
            config.finish_actions().commit_message.as_deref(),
            Some("feat: apply agent changes")
        );
        assert_eq!(
            config.finish_actions().push_mode,
            GitRemoteActionMode::DryRun
        );
        assert_eq!(
            config.finish_actions().pull_request_mode,
            GitRemoteActionMode::DryRun
        );
        assert_eq!(
            config.finish_actions().pull_request_repository.as_deref(),
            Some("acme/service")
        );
    }

    #[test]
    fn from_reader_accepts_explicit_local_sandbox_for_host_process_backend() {
        let config = load(&[
            ("SLACK_BOT_TOKEN", "xoxb-test"),
            ("SLACK_SIGNING_SECRET", "secret"),
            ("OPEN_AGENTS_SANDBOX", "local"),
            ("OPEN_AGENTS_SANDBOX_ROOT", "/tmp/open-agents-local"),
        ])
        .unwrap();

        assert_eq!(
            config.sandbox(),
            &SandboxMode::Local {
                root: PathBuf::from("/tmp/open-agents-local"),
            }
        );
    }

    #[test]
    fn from_reader_loads_open_plugin_fixture_components() {
        let plugin_root = plugin_fixture_root()
            .canonicalize()
            .expect("fixture plugin root exists");
        let data_dir = env::temp_dir().join("open-agents-service-plugin-data-fixture");
        let plugin_roots = env::join_paths([plugin_root.clone()])
            .expect("plugin roots join")
            .into_string()
            .expect("fixture path is utf-8");
        let config = load_owned(&[
            ("SLACK_BOT_TOKEN", "xoxb-test".to_string()),
            ("SLACK_SIGNING_SECRET", "signing-real-secret".to_string()),
            (OPEN_AGENTS_PLUGIN_ROOTS_ENV, plugin_roots),
            (
                OPEN_AGENTS_PLUGIN_DATA_DIR_ENV,
                data_dir.display().to_string(),
            ),
        ])
        .unwrap();

        let catalog = config.plugin_catalog();
        assert_eq!(catalog.roots(), std::slice::from_ref(&plugin_root));
        assert_eq!(catalog.packages().len(), 1);
        assert!(catalog.diagnostics().is_empty());
        assert_eq!(catalog.runtime_skills()[0].name, "hello-plugin:greet");
        assert_eq!(
            catalog.runtime_skills()[0].description,
            "Greet a Slack user from the Open Plugin fixture"
        );

        let mcp = &catalog.packages()[0].mcp_servers[0];
        assert_eq!(mcp.plugin_name, "hello-plugin");
        assert_eq!(mcp.server_name, "echo");
        let expected_command = format!("{}/bin/echo-mcp", plugin_root.display());
        assert_eq!(mcp.command.as_deref(), Some(expected_command.as_str()));
        assert!(
            mcp.args
                .iter()
                .any(|arg| arg == &data_dir.join("hello-plugin").display().to_string())
        );
        assert_eq!(
            mcp.env_keys,
            vec![
                "PLUGIN_FIXTURE_DATA".to_string(),
                "PLUGIN_FIXTURE_ROOT".to_string()
            ]
        );

        let runtime_mcp = &catalog.runtime_mcp_servers()[0];
        assert_eq!(runtime_mcp.tool_prefix, "mcp__plugin_hello-plugin_echo__");
        assert!(runtime_mcp.has_args);
        assert_eq!(runtime_mcp.env_keys, mcp.env_keys);

        let summary = config.operator_summary();
        assert!(summary.contains("plugin_roots=1"));
        assert!(summary.contains("plugins=1"));
        assert!(summary.contains("plugin_skills=1"));
        assert!(summary.contains("plugin_mcp_servers=1"));
        assert!(summary.contains("plugin_data_dir=present"));
        assert!(summary.contains("sandbox=just-bash"));
        assert!(summary.contains("sandbox_working_directory=/workspace"));
        assert!(!summary.contains("xoxb-test"));
        assert!(!summary.contains("signing-real-secret"));
    }

    #[test]
    fn from_reader_reports_invalid_plugin_root() {
        let missing_root = env::temp_dir().join("open-agents-missing-plugin-root");
        let err = load_owned(&[
            ("SLACK_BOT_TOKEN", "xoxb-test".to_string()),
            ("SLACK_SIGNING_SECRET", "secret".to_string()),
            (
                OPEN_AGENTS_PLUGIN_ROOTS_ENV,
                missing_root.display().to_string(),
            ),
        ])
        .unwrap_err();

        assert!(
            matches!(err, ConfigError::PluginConfig(message) if message.contains("failed to read plugin root"))
        );
    }

    #[test]
    fn from_reader_requires_gateway_key_when_gateway_runtime_is_explicit() {
        let err = load(&[
            ("SLACK_BOT_TOKEN", "xoxb-test"),
            ("SLACK_SIGNING_SECRET", "secret"),
            ("OPEN_AGENTS_RUNTIME", "gateway"),
        ])
        .unwrap_err();

        assert_eq!(err, ConfigError::MissingVar("AI_GATEWAY_API_KEY"));
    }

    #[test]
    fn postgres_requires_state_url() {
        let err = load(&[
            ("SLACK_BOT_TOKEN", "xoxb-test"),
            ("SLACK_SIGNING_SECRET", "secret"),
            ("OPEN_AGENTS_STATE", "postgres"),
        ])
        .unwrap_err();
        assert_eq!(err, ConfigError::MissingVar("OPEN_AGENTS_STATE_URL"));
    }

    #[test]
    fn postgres_accepts_postgres_url_alias() {
        let config = load(&[
            ("SLACK_BOT_TOKEN", "xoxb-test"),
            ("SLACK_SIGNING_SECRET", "secret"),
            ("OPEN_AGENTS_STATE", "postgres"),
            ("POSTGRES_URL", "postgres://localhost/open_agents"),
        ])
        .unwrap();

        assert_eq!(config.state_store().label(), "postgres");
    }

    #[test]
    fn from_reader_accepts_vercel_sandbox_selection_without_credentials() {
        let config = load(&[
            ("SLACK_BOT_TOKEN", "xoxb-test"),
            ("SLACK_SIGNING_SECRET", "secret"),
            ("OPEN_AGENTS_SANDBOX", "vercel"),
            ("VERCEL_SANDBOX_NAME", "oa-session"),
            ("VERCEL_SANDBOX_BASE_SNAPSHOT_ID", "snap_123"),
        ])
        .unwrap();

        assert_eq!(
            config.sandbox(),
            &SandboxMode::Vercel {
                base_snapshot_id: Some("snap_123".to_string()),
                sandbox_name: Some("oa-session".to_string()),
            }
        );
    }

    #[test]
    fn operator_summary_redacts_secret_values() {
        let config = load(&[
            ("SLACK_BOT_TOKEN", "xoxb-real-secret"),
            ("SLACK_SIGNING_SECRET", "signing-secret"),
            ("SLACK_APP_TOKEN", "xapp-secret"),
            ("OPEN_AGENTS_SLACK_INGRESS", "socket-mode"),
        ])
        .unwrap();

        let summary = config.operator_summary();
        assert!(summary.contains("slack_bot_token=present"));
        assert!(summary.contains("socket_mode_token=present"));
        assert!(summary.contains("runtime=fixture"));
        assert!(summary.contains("sandbox=just-bash"));
        assert!(summary.contains("model=openai/gpt-4.1-mini"));
        assert!(summary.contains("github_token=missing"));
        assert!(!summary.contains("xoxb-real-secret"));
        assert!(!summary.contains("signing-secret"));
        assert!(!summary.contains("xapp-secret"));
    }
}
