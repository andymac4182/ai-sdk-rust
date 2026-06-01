use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;

use open_agents_slack::SlackIngressMode;

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
            Self::Local { .. } => "local",
            Self::Vercel { .. } => "vercel",
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
    model_api_key: Option<String>,
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
            .field(
                "model_api_key",
                &self.model_api_key.as_ref().map(|_| "<redacted>"),
            )
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

        Ok(Self {
            bind_addr,
            state_store,
            slack_ingress,
            slack_bot_token,
            slack_signing_secret,
            slack_app_token,
            slack_api_url,
            sandbox,
            model_api_key,
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
            sandbox: SandboxMode::Local {
                root: PathBuf::from("."),
            },
            model_api_key: None,
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

    /// Optional model credential. Callers must not log this value.
    pub fn model_api_key(&self) -> Option<&str> {
        self.model_api_key.as_deref()
    }

    /// Redacted summary safe to print in operator logs.
    pub fn operator_summary(&self) -> String {
        format!(
            "bind_addr={} state={} slack_ingress={} sandbox={} slack_bot_token={} signing_secret={} socket_mode_token={} slack_api_url={} model_credential={}",
            self.bind_addr,
            self.state_store.label(),
            slack_ingress_label(self.slack_ingress),
            self.sandbox.label(),
            present_label(Some(&self.slack_bot_token)),
            present_label(Some(&self.slack_signing_secret)),
            present_label(self.slack_app_token.as_ref()),
            present_label(self.slack_api_url.as_ref()),
            present_label(self.model_api_key.as_ref()),
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
    match raw.unwrap_or("local") {
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
            expected: "local or vercel",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn load(
        pairs: &[(&'static str, &'static str)],
    ) -> Result<OpenAgentsServiceConfig, ConfigError> {
        let vars: HashMap<&'static str, String> = pairs
            .iter()
            .map(|(key, value)| (*key, (*value).to_string()))
            .collect();
        OpenAgentsServiceConfig::from_reader(|name| vars.get(name).cloned())
    }

    #[test]
    fn from_reader_defaults_to_memory_webhook_and_local_sandbox() {
        let config = load(&[
            ("SLACK_BOT_TOKEN", "xoxb-test"),
            ("SLACK_SIGNING_SECRET", "secret"),
        ])
        .unwrap();

        assert_eq!(config.bind_addr().to_string(), "127.0.0.1:8080");
        assert_eq!(config.state_store().label(), "memory");
        assert_eq!(config.slack_ingress(), SlackIngressMode::Webhook);
        assert_eq!(config.slack_api_url(), None);
        assert_eq!(config.sandbox().label(), "local");
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
        assert!(!summary.contains("xoxb-real-secret"));
        assert!(!summary.contains("signing-secret"));
        assert!(!summary.contains("xapp-secret"));
    }
}
