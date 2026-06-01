//! Vercel Sandbox v2 API client and Open Agents sandbox backend.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;

use flate2::Compression;
use flate2::write::GzEncoder;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tar::{Builder as TarBuilder, Header};

use crate::{
    DEFAULT_MAX_OUTPUT_LENGTH, OPEN_AGENTS_VERCEL_TOKEN_ENV, Sandbox, SandboxConnectConfig,
    SandboxDetachedCommand, SandboxDetachedOptions, SandboxDirEntry, SandboxDirEntryKind,
    SandboxError, SandboxExecOptions, SandboxExecResult, SandboxMkdirOptions, SandboxResult,
    SandboxSource, SandboxState, SandboxStats, SandboxTimeoutExtension, SandboxType,
    SnapshotResult, VERCEL_OIDC_TOKEN_ENV, VERCEL_PROJECT_ID_ENV, VERCEL_SANDBOX_API_BASE_URL_ENV,
    VERCEL_SANDBOX_NAME_ENV, VERCEL_SANDBOX_PERSISTENT_ENV, VERCEL_SANDBOX_RUNTIME_ENV,
    VERCEL_SANDBOX_TIMEOUT_MS_ENV, VERCEL_SANDBOX_VCPUS_ENV, VERCEL_TEAM_ID_ENV, VERCEL_TOKEN_ENV,
};

const DEFAULT_VERCEL_API_BASE_URL: &str = "https://vercel.com/api";
const DEFAULT_VERCEL_WORKING_DIRECTORY: &str = "/vercel/sandbox";
const DEFAULT_VERCEL_USER_AGENT: &str = "ai-sdk-rust/open-agents-sandbox";
const STAT_FORMAT: &str = "%s|%f|%Y";

/// Vercel access credentials used by the Sandbox v2 API.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VercelSandboxCredentials {
    /// Bearer token from `VERCEL_TOKEN` or `VERCEL_OIDC_TOKEN`.
    pub token: String,
    /// Vercel team id.
    pub team_id: String,
    /// Vercel project id.
    pub project_id: String,
}

impl VercelSandboxCredentials {
    /// Creates credentials from explicit values.
    pub fn new(
        token: impl Into<String>,
        team_id: impl Into<String>,
        project_id: impl Into<String>,
    ) -> Self {
        Self {
            token: token.into(),
            team_id: team_id.into(),
            project_id: project_id.into(),
        }
    }
}

/// Vercel Sandbox client configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VercelSandboxConfig {
    /// API base URL. Defaults to `https://vercel.com/api`.
    pub base_url: String,
    /// Vercel credentials.
    pub credentials: VercelSandboxCredentials,
    /// Optional stable sandbox name.
    pub sandbox_name: Option<String>,
    /// Optional runtime, for example `node24` or `python3.13`.
    pub runtime: Option<String>,
    /// Optional vCPU allocation.
    pub vcpus: Option<u32>,
    /// Optional sandbox timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// Optional persistence flag for named sandboxes.
    pub persistent: Option<bool>,
}

impl VercelSandboxConfig {
    /// Creates config from explicit credentials and default Vercel API settings.
    pub fn new(credentials: VercelSandboxCredentials) -> Self {
        Self {
            base_url: DEFAULT_VERCEL_API_BASE_URL.to_string(),
            credentials,
            sandbox_name: None,
            runtime: None,
            vcpus: None,
            timeout_ms: None,
            persistent: None,
        }
    }

    /// Sets the Vercel API base URL.
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    /// Sets a stable named sandbox.
    pub fn with_sandbox_name(mut self, sandbox_name: impl Into<String>) -> Self {
        self.sandbox_name = Some(sandbox_name.into());
        self
    }

    /// Sets the runtime.
    pub fn with_runtime(mut self, runtime: impl Into<String>) -> Self {
        self.runtime = Some(runtime.into());
        self
    }

    /// Sets the vCPU count.
    pub fn with_vcpus(mut self, vcpus: u32) -> Self {
        self.vcpus = Some(vcpus);
        self
    }

    /// Sets the timeout in milliseconds.
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// Sets whether created named sandboxes should be persistent.
    pub fn with_persistent(mut self, persistent: bool) -> Self {
        self.persistent = Some(persistent);
        self
    }

    /// Loads configuration from process environment variables.
    pub fn from_env() -> SandboxResult<Self> {
        Self::from_reader(|name| std::env::var(name).ok())
    }

    /// Loads configuration using a caller-provided variable reader.
    pub fn from_reader(
        mut read_var: impl FnMut(&'static str) -> Option<String>,
    ) -> SandboxResult<Self> {
        let token = present(read_var(OPEN_AGENTS_VERCEL_TOKEN_ENV))
            .or_else(|| present(read_var(VERCEL_TOKEN_ENV)))
            .or_else(|| present(read_var(VERCEL_OIDC_TOKEN_ENV)))
            .ok_or(SandboxError::MissingConfig {
                name: OPEN_AGENTS_VERCEL_TOKEN_ENV,
            })?;
        let team_id = present(read_var(VERCEL_TEAM_ID_ENV)).ok_or(SandboxError::MissingConfig {
            name: VERCEL_TEAM_ID_ENV,
        })?;
        let project_id =
            present(read_var(VERCEL_PROJECT_ID_ENV)).ok_or(SandboxError::MissingConfig {
                name: VERCEL_PROJECT_ID_ENV,
            })?;
        let base_url = present(read_var(VERCEL_SANDBOX_API_BASE_URL_ENV))
            .unwrap_or_else(|| DEFAULT_VERCEL_API_BASE_URL.to_string());
        let sandbox_name = present(read_var(VERCEL_SANDBOX_NAME_ENV));
        let runtime = present(read_var(VERCEL_SANDBOX_RUNTIME_ENV));
        let vcpus = parse_optional_u32(
            read_var(VERCEL_SANDBOX_VCPUS_ENV).as_deref(),
            VERCEL_SANDBOX_VCPUS_ENV,
        )?;
        let timeout_ms = parse_optional_u64(
            read_var(VERCEL_SANDBOX_TIMEOUT_MS_ENV).as_deref(),
            VERCEL_SANDBOX_TIMEOUT_MS_ENV,
        )?;
        let persistent = parse_optional_bool(
            read_var(VERCEL_SANDBOX_PERSISTENT_ENV).as_deref(),
            VERCEL_SANDBOX_PERSISTENT_ENV,
        )?;

        Ok(Self {
            base_url,
            credentials: VercelSandboxCredentials::new(token, team_id, project_id),
            sandbox_name,
            runtime,
            vcpus,
            timeout_ms,
            persistent,
        })
    }
}

/// Stable Vercel Sandbox status values from the v2 API.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum VercelSandboxStatus {
    /// Session or sandbox is pending.
    Pending,
    /// Session or sandbox is running.
    Running,
    /// Session or sandbox is stopping.
    Stopping,
    /// Session or sandbox is stopped.
    Stopped,
    /// Session or sandbox failed.
    Failed,
    /// Session was aborted.
    Aborted,
    /// Session is snapshotting.
    Snapshotting,
    /// Unknown status returned by the API.
    #[default]
    #[serde(other)]
    Unknown,
}

impl VercelSandboxStatus {
    fn is_stopped(&self) -> bool {
        matches!(
            self,
            Self::Stopped | Self::Failed | Self::Aborted | Self::Unknown
        )
    }
}

/// Vercel Sandbox route metadata.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VercelSandboxRoute {
    /// Full route URL, when returned by the API.
    pub url: String,
    /// Vercel subdomain without the `.vercel.run` suffix.
    pub subdomain: String,
    /// Exposed port.
    pub port: u16,
}

/// Vercel named-sandbox metadata.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VercelSandboxMetadata {
    /// Stable named sandbox.
    pub name: String,
    /// Whether automatic persistence is enabled.
    pub persistent: bool,
    /// Current session id.
    pub current_session_id: String,
    /// Current snapshot id, when present.
    pub current_snapshot_id: Option<String>,
    /// Current status.
    pub status: VercelSandboxStatus,
    /// Runtime id.
    pub runtime: Option<String>,
    /// vCPU allocation.
    pub vcpus: Option<u32>,
    /// Memory allocation in MB.
    pub memory: Option<u64>,
    /// Timeout in milliseconds.
    pub timeout: Option<u64>,
    /// Creation timestamp in milliseconds.
    pub created_at: u64,
    /// Update timestamp in milliseconds.
    pub updated_at: u64,
    /// Default working directory.
    pub cwd: Option<String>,
    /// Tags.
    pub tags: Option<BTreeMap<String, String>>,
}

/// Vercel Sandbox session metadata.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VercelSandboxSession {
    /// Session id.
    pub id: String,
    /// VM status.
    pub status: VercelSandboxStatus,
    /// Memory allocation in MB.
    pub memory: u64,
    /// vCPU allocation.
    pub vcpus: u32,
    /// Region.
    pub region: String,
    /// Runtime id.
    pub runtime: String,
    /// Timeout in milliseconds.
    pub timeout: u64,
    /// Request timestamp in milliseconds.
    pub requested_at: u64,
    /// Start timestamp in milliseconds.
    pub started_at: Option<u64>,
    /// Stop request timestamp in milliseconds.
    pub requested_stop_at: Option<u64>,
    /// Stop timestamp in milliseconds.
    pub stopped_at: Option<u64>,
    /// Source snapshot id.
    pub source_snapshot_id: Option<String>,
    /// Creation timestamp in milliseconds.
    pub created_at: u64,
    /// Current working directory.
    pub cwd: String,
    /// Update timestamp in milliseconds.
    pub updated_at: u64,
    /// Interactive shell port.
    pub interactive_port: Option<u16>,
    /// Active CPU duration, when reported.
    pub active_cpu_duration_ms: Option<u64>,
}

/// Vercel command metadata.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VercelCommandData {
    /// Command id.
    pub id: String,
    /// Executable name.
    pub name: String,
    /// Command arguments.
    pub args: Vec<String>,
    /// Working directory.
    pub cwd: String,
    /// Session id.
    pub session_id: String,
    /// Exit code, when complete.
    pub exit_code: Option<i32>,
    /// Start timestamp in milliseconds.
    pub started_at: u64,
}

/// Source object accepted by Vercel's create-sandbox API.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum VercelSandboxUpstreamSource {
    /// Git repository source.
    #[serde(rename = "git")]
    Git {
        /// Repository URL.
        url: String,
        /// Optional shallow clone depth.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        depth: Option<u32>,
        /// Optional branch, tag, or commit.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        revision: Option<String>,
        /// Optional basic-auth username.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        username: Option<String>,
        /// Optional basic-auth password.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        password: Option<String>,
    },
    /// Tarball URL source.
    #[serde(rename = "tarball")]
    Tarball {
        /// Tarball URL.
        url: String,
    },
    /// Snapshot source.
    #[serde(rename = "snapshot")]
    Snapshot {
        /// Snapshot id.
        #[serde(rename = "snapshotId")]
        snapshot_id: String,
    },
}

/// Resource allocation for create/update requests.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VercelSandboxResources {
    /// Number of vCPUs.
    pub vcpus: u32,
}

/// Request body for Vercel `POST /v2/sandboxes`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VercelSandboxCreateRequest {
    /// Vercel project id.
    pub project_id: String,
    /// Optional stable sandbox name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Source material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<VercelSandboxUpstreamSource>,
    /// Ports to expose.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<u16>,
    /// Timeout in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    /// Resource allocation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<VercelSandboxResources>,
    /// Runtime id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    /// Default environment variables inherited by commands.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Key/value tags.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tags: BTreeMap<String, String>,
    /// Enable automatic persistence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistent: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct VercelSandboxAndSessionResponse {
    sandbox: VercelSandboxMetadata,
    session: VercelSandboxSession,
    #[serde(default)]
    routes: Vec<VercelSandboxRoute>,
    #[serde(default)]
    resumed: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct VercelSessionAndRoutesResponse {
    session: VercelSandboxSession,
    #[serde(default)]
    routes: Vec<VercelSandboxRoute>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct VercelSessionResponse {
    session: VercelSandboxSession,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct VercelStopSessionResponse {
    session: VercelSandboxSession,
    #[serde(default)]
    sandbox: Option<VercelSandboxMetadata>,
    #[serde(default)]
    snapshot: Option<VercelSnapshotMetadata>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct VercelSnapshotResponse {
    snapshot: VercelSnapshotMetadata,
    session: VercelSandboxSession,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct VercelSnapshotMetadata {
    id: String,
    source_session_id: String,
    status: String,
    size_bytes: u64,
    created_at: u64,
    updated_at: u64,
    expires_at: Option<u64>,
    parent_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct VercelCommandResponse {
    command: VercelCommandData,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct VercelSandboxesResponse {
    #[serde(default)]
    sandboxes: Vec<VercelSandboxMetadata>,
    pagination: VercelPagination,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct VercelPagination {
    count: usize,
    next: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VercelRunCommandRequest<'a> {
    command: &'a str,
    args: &'a [String],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cwd: Option<&'a str>,
    env: &'a BTreeMap<String, String>,
    sudo: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    wait: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct VercelLogLine {
    stream: String,
    data: Value,
}

#[derive(Clone, Debug)]
struct VercelSandboxHttpResponse {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

/// Synchronous Vercel Sandbox v2 API client.
#[derive(Clone, Debug)]
pub struct VercelSandboxClient {
    config: VercelSandboxConfig,
}

impl VercelSandboxClient {
    /// Creates a client from explicit config.
    pub fn new(config: VercelSandboxConfig) -> Self {
        Self { config }
    }

    /// Returns the client config.
    pub fn config(&self) -> &VercelSandboxConfig {
        &self.config
    }

    /// Creates a sandbox and initial session.
    pub fn create_sandbox(
        &self,
        request: &VercelSandboxCreateRequest,
    ) -> SandboxResult<(
        VercelSandboxMetadata,
        VercelSandboxSession,
        Vec<VercelSandboxRoute>,
    )> {
        let response: VercelSandboxAndSessionResponse =
            self.post_json("create sandbox", "/v2/sandboxes", &[], request)?;
        Ok((response.sandbox, response.session, response.routes))
    }

    /// Gets a named sandbox, optionally resuming its current session.
    pub fn get_sandbox(
        &self,
        name: &str,
        resume: Option<bool>,
    ) -> SandboxResult<(
        VercelSandboxMetadata,
        VercelSandboxSession,
        Vec<VercelSandboxRoute>,
        bool,
    )> {
        let mut query = vec![(
            "projectId".to_string(),
            self.config.credentials.project_id.clone(),
        )];
        if let Some(resume) = resume {
            query.push(("resume".to_string(), resume.to_string()));
        }
        let path = format!("/v2/sandboxes/{}", encode_component(name));
        let response: VercelSandboxAndSessionResponse =
            self.get_json("get sandbox", &path, &query)?;
        Ok((
            response.sandbox,
            response.session,
            response.routes,
            response.resumed.unwrap_or(false),
        ))
    }

    /// Lists named sandboxes in the configured project.
    pub fn list_sandboxes(
        &self,
        limit: Option<u32>,
        cursor: Option<&str>,
    ) -> SandboxResult<Vec<VercelSandboxMetadata>> {
        let mut query = vec![(
            "project".to_string(),
            self.config.credentials.project_id.clone(),
        )];
        if let Some(limit) = limit {
            query.push(("limit".to_string(), limit.to_string()));
        }
        if let Some(cursor) = cursor {
            query.push(("cursor".to_string(), cursor.to_string()));
        }
        let response: VercelSandboxesResponse =
            self.get_json("list sandboxes", "/v2/sandboxes", &query)?;
        Ok(response.sandboxes)
    }

    /// Gets a session and its routes.
    pub fn get_session(
        &self,
        session_id: &str,
    ) -> SandboxResult<(VercelSandboxSession, Vec<VercelSandboxRoute>)> {
        let path = format!("/v2/sandboxes/sessions/{}", encode_component(session_id));
        let response: VercelSessionAndRoutesResponse = self.get_json("get session", &path, &[])?;
        Ok((response.session, response.routes))
    }

    /// Runs a command and waits for completion.
    pub fn run_command_wait(
        &self,
        session_id: &str,
        command: &str,
        args: &[String],
        cwd: Option<&str>,
        env: &BTreeMap<String, String>,
        sudo: bool,
    ) -> SandboxResult<VercelCommandData> {
        let body = VercelRunCommandRequest {
            command,
            args,
            cwd,
            env,
            sudo,
            wait: true,
        };
        let path = format!(
            "/v2/sandboxes/sessions/{}/cmd",
            encode_component(session_id)
        );
        let response = self.request_json_body("run command", "POST", &path, &[], Some(&body))?;
        ensure_success("run command", &response)?;
        ensure_content_type("run command", &response, "application/x-ndjson")?;
        let chunks = parse_ndjson::<VercelCommandResponse>("run command", &response.body)?;
        let finished = chunks
            .last()
            .ok_or_else(|| SandboxError::Api {
                operation: "run command".to_string(),
                status: Some(response.status),
                message: "stream ended before command data was received".to_string(),
            })?
            .command
            .clone();
        if finished.exit_code.is_none() {
            return Err(SandboxError::Api {
                operation: "run command".to_string(),
                status: Some(response.status),
                message: "stream ended before command finished".to_string(),
            });
        }
        Ok(finished)
    }

    /// Starts a detached command.
    pub fn run_command_detached(
        &self,
        session_id: &str,
        command: &str,
        args: &[String],
        cwd: Option<&str>,
        env: &BTreeMap<String, String>,
        sudo: bool,
    ) -> SandboxResult<VercelCommandData> {
        let body = VercelRunCommandRequest {
            command,
            args,
            cwd,
            env,
            sudo,
            wait: false,
        };
        let path = format!(
            "/v2/sandboxes/sessions/{}/cmd",
            encode_component(session_id)
        );
        let response: VercelCommandResponse =
            self.post_json("start detached command", &path, &[], &body)?;
        Ok(response.command)
    }

    /// Reads stdout and stderr logs for a command.
    pub fn command_logs(
        &self,
        session_id: &str,
        command_id: &str,
    ) -> SandboxResult<(String, String, bool)> {
        let path = format!(
            "/v2/sandboxes/sessions/{}/cmd/{}/logs",
            encode_component(session_id),
            encode_component(command_id)
        );
        let response = self.request_json_body::<()>("command logs", "GET", &path, &[], None)?;
        ensure_success("command logs", &response)?;
        ensure_content_type("command logs", &response, "application/x-ndjson")?;
        let logs = parse_ndjson::<VercelLogLine>("command logs", &response.body)?;
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut truncated = false;
        for log in logs {
            match log.stream.as_str() {
                "stdout" => {
                    let Some(data) = log.data.as_str() else {
                        continue;
                    };
                    append_capped(&mut stdout, data, &mut truncated);
                }
                "stderr" => {
                    let Some(data) = log.data.as_str() else {
                        continue;
                    };
                    append_capped(&mut stderr, data, &mut truncated);
                }
                "error" => {
                    return Err(SandboxError::Api {
                        operation: "command logs".to_string(),
                        status: Some(response.status),
                        message: log.data.to_string(),
                    });
                }
                _ => {}
            }
        }
        Ok((stdout, stderr, truncated))
    }

    /// Creates a directory using the Vercel filesystem API.
    pub fn mkdir(&self, session_id: &str, path: &str, cwd: Option<&str>) -> SandboxResult<()> {
        let body = serde_json::json!({ "path": path, "cwd": cwd });
        let endpoint = format!(
            "/v2/sandboxes/sessions/{}/fs/mkdir",
            encode_component(session_id)
        );
        let _: Value = self.post_json("mkdir", &endpoint, &[], &body)?;
        Ok(())
    }

    /// Reads a file from the sandbox filesystem.
    pub fn read_file(
        &self,
        session_id: &str,
        path: &str,
        cwd: Option<&str>,
    ) -> SandboxResult<Option<Vec<u8>>> {
        let body = serde_json::json!({ "path": path, "cwd": cwd });
        let endpoint = format!(
            "/v2/sandboxes/sessions/{}/fs/read",
            encode_component(session_id)
        );
        let response = self.request_json_body("read file", "POST", &endpoint, &[], Some(&body))?;
        if response.status == 404 {
            return Ok(None);
        }
        ensure_success("read file", &response)?;
        ensure_content_type("read file", &response, "application/octet-stream")?;
        Ok(Some(response.body))
    }

    /// Writes files using the same gzip tar upload API as `@vercel/sandbox`.
    pub fn write_files(
        &self,
        session_id: &str,
        cwd: &str,
        files: &[VercelWriteFile<'_>],
    ) -> SandboxResult<()> {
        let archive = encode_write_files_archive(cwd, files)?;
        let endpoint = format!(
            "/v2/sandboxes/sessions/{}/fs/write",
            encode_component(session_id)
        );
        let response = self.request_bytes(
            "write files",
            "POST",
            &endpoint,
            &[],
            &[("content-type", "application/gzip"), ("x-cwd", "/")],
            archive,
        )?;
        ensure_success("write files", &response)?;
        Ok(())
    }

    /// Stops the current session.
    pub fn stop_session(
        &self,
        session_id: &str,
    ) -> SandboxResult<(
        VercelSandboxSession,
        Option<VercelSandboxMetadata>,
        Option<String>,
    )> {
        let endpoint = format!(
            "/v2/sandboxes/sessions/{}/stop",
            encode_component(session_id)
        );
        let response =
            self.request_json_body::<()>("stop session", "POST", &endpoint, &[], None)?;
        let response: VercelStopSessionResponse = parse_json_response("stop session", response)?;
        Ok((
            response.session,
            response.sandbox,
            response.snapshot.map(|snapshot| snapshot.id),
        ))
    }

    /// Extends the current session timeout.
    pub fn extend_timeout(
        &self,
        session_id: &str,
        duration_ms: u64,
    ) -> SandboxResult<VercelSandboxSession> {
        let endpoint = format!(
            "/v2/sandboxes/sessions/{}/extend-timeout",
            encode_component(session_id)
        );
        let body = serde_json::json!({ "duration": duration_ms });
        let response: VercelSessionResponse =
            self.post_json("extend timeout", &endpoint, &[], &body)?;
        Ok(response.session)
    }

    /// Creates a filesystem snapshot and stops the current session.
    pub fn create_snapshot(
        &self,
        session_id: &str,
    ) -> SandboxResult<(String, VercelSandboxSession)> {
        let endpoint = format!(
            "/v2/sandboxes/sessions/{}/snapshot",
            encode_component(session_id)
        );
        let response =
            self.request_json_body::<()>("create snapshot", "POST", &endpoint, &[], None)?;
        let response: VercelSnapshotResponse = parse_json_response("create snapshot", response)?;
        Ok((response.snapshot.id, response.session))
    }

    fn get_json<T: DeserializeOwned>(
        &self,
        operation: &str,
        path: &str,
        query: &[(String, String)],
    ) -> SandboxResult<T> {
        let response = self.request_json_body::<()>(operation, "GET", path, query, None)?;
        parse_json_response(operation, response)
    }

    fn post_json<T: DeserializeOwned, B: Serialize>(
        &self,
        operation: &str,
        path: &str,
        query: &[(String, String)],
        body: &B,
    ) -> SandboxResult<T> {
        let response = self.request_json_body(operation, "POST", path, query, Some(body))?;
        parse_json_response(operation, response)
    }

    fn request_json_body<B: Serialize>(
        &self,
        operation: &str,
        method: &str,
        path: &str,
        query: &[(String, String)],
        body: Option<&B>,
    ) -> SandboxResult<VercelSandboxHttpResponse> {
        let body = match body {
            Some(body) => Some(serde_json::to_vec(body).map_err(|error| SandboxError::Api {
                operation: operation.to_string(),
                status: None,
                message: error.to_string(),
            })?),
            None => None,
        };
        self.request_bytes(
            operation,
            method,
            path,
            query,
            &[("content-type", "application/json")],
            body.unwrap_or_default(),
        )
    }

    fn request_bytes(
        &self,
        operation: &str,
        method: &str,
        path: &str,
        query: &[(String, String)],
        headers: &[(&str, &str)],
        body: Vec<u8>,
    ) -> SandboxResult<VercelSandboxHttpResponse> {
        let url = self.url(path, query);
        let response = match method {
            "GET" => {
                let mut builder = ureq::get(&url)
                    .header(
                        "authorization",
                        format!("Bearer {}", self.config.credentials.token),
                    )
                    .header("user-agent", DEFAULT_VERCEL_USER_AGENT);
                for (name, value) in headers {
                    builder = builder.header(*name, *value);
                }
                builder.config().http_status_as_error(false).build().call()
            }
            "POST" => {
                let mut builder = ureq::post(&url)
                    .header(
                        "authorization",
                        format!("Bearer {}", self.config.credentials.token),
                    )
                    .header("user-agent", DEFAULT_VERCEL_USER_AGENT);
                for (name, value) in headers {
                    builder = builder.header(*name, *value);
                }
                let request = builder.config().http_status_as_error(false).build();
                if body.is_empty() {
                    request.send_empty()
                } else {
                    request.send(body)
                }
            }
            "PATCH" => {
                let mut builder = ureq::patch(&url)
                    .header(
                        "authorization",
                        format!("Bearer {}", self.config.credentials.token),
                    )
                    .header("user-agent", DEFAULT_VERCEL_USER_AGENT);
                for (name, value) in headers {
                    builder = builder.header(*name, *value);
                }
                let request = builder.config().http_status_as_error(false).build();
                if body.is_empty() {
                    request.send_empty()
                } else {
                    request.send(body)
                }
            }
            "DELETE" => {
                let mut builder = ureq::delete(&url)
                    .header(
                        "authorization",
                        format!("Bearer {}", self.config.credentials.token),
                    )
                    .header("user-agent", DEFAULT_VERCEL_USER_AGENT);
                for (name, value) in headers {
                    builder = builder.header(*name, *value);
                }
                builder.config().http_status_as_error(false).build().call()
            }
            _ => {
                return Err(SandboxError::Api {
                    operation: operation.to_string(),
                    status: None,
                    message: format!("unsupported method {method}"),
                });
            }
        };
        read_http_response(operation, response)
    }

    fn url(&self, path: &str, query: &[(String, String)]) -> String {
        let mut url = format!("{}{}", self.config.base_url.trim_end_matches('/'), path);
        append_query(&mut url, "teamId", &self.config.credentials.team_id);
        for (key, value) in query {
            append_query(&mut url, key, value);
        }
        url
    }
}

/// File upload entry for Vercel write-files requests.
#[derive(Clone, Debug)]
pub struct VercelWriteFile<'a> {
    /// Sandbox path.
    pub path: &'a str,
    /// File content.
    pub content: &'a [u8],
    /// Optional file mode.
    pub mode: Option<u32>,
}

/// Open Agents `Sandbox` implementation backed by Vercel Sandbox.
pub struct VercelSandbox {
    client: VercelSandboxClient,
    inner: Mutex<VercelSandboxInner>,
    env: BTreeMap<String, String>,
    current_branch: Option<String>,
    working_directory: String,
    environment_details: String,
}

#[derive(Clone, Debug)]
struct VercelSandboxInner {
    sandbox: VercelSandboxMetadata,
    session: VercelSandboxSession,
    routes: Vec<VercelSandboxRoute>,
    timeout_ms: Option<u64>,
    last_snapshot_id: Option<String>,
}

impl fmt::Debug for VercelSandbox {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let inner = self.inner.lock().ok();
        formatter
            .debug_struct("VercelSandbox")
            .field(
                "sandbox_name",
                &inner.as_ref().map(|inner| &inner.sandbox.name),
            )
            .field("session_id", &inner.as_ref().map(|inner| &inner.session.id))
            .field("working_directory", &self.working_directory)
            .field("env_keys", &self.env.keys().collect::<Vec<_>>())
            .field("current_branch", &self.current_branch)
            .finish_non_exhaustive()
    }
}

impl VercelSandbox {
    /// Connects to a Vercel sandbox using process environment configuration.
    pub fn connect(config: SandboxConnectConfig) -> SandboxResult<Self> {
        Self::connect_with_config(config, VercelSandboxConfig::from_env()?)
    }

    /// Connects to a Vercel sandbox using explicit configuration.
    pub fn connect_with_config(
        config: SandboxConnectConfig,
        mut vercel_config: VercelSandboxConfig,
    ) -> SandboxResult<Self> {
        if let Some(timeout_ms) = config.options.timeout_ms {
            vercel_config.timeout_ms = Some(timeout_ms);
        }
        let client = VercelSandboxClient::new(vercel_config.clone());
        let SandboxState::Vercel {
            source,
            sandbox_name,
            sandbox_id,
            snapshot_id,
            expires_at: _,
        } = config.state
        else {
            return Err(SandboxError::UnsupportedOperation {
                operation: "connect".to_string(),
                sandbox_type: SandboxType::Local,
            });
        };

        let current_branch = source
            .as_ref()
            .and_then(|source| source.new_branch.clone().or_else(|| source.branch.clone()));
        let create_sandbox = |name: Option<String>| {
            let create = VercelSandboxCreateRequest {
                project_id: vercel_config.credentials.project_id.clone(),
                name,
                source: snapshot_id
                    .clone()
                    .map(|snapshot_id| VercelSandboxUpstreamSource::Snapshot { snapshot_id })
                    .or_else(|| source.as_ref().map(source_to_vercel_source)),
                ports: config.options.ports.clone(),
                timeout: vercel_config.timeout_ms,
                resources: vercel_config
                    .vcpus
                    .map(|vcpus| VercelSandboxResources { vcpus }),
                runtime: vercel_config.runtime.clone(),
                env: config.options.env.clone(),
                tags: BTreeMap::new(),
                persistent: vercel_config.persistent,
            };
            client.create_sandbox(&create)
        };
        let selected_name = sandbox_name.or(vercel_config.sandbox_name.clone());
        let (sandbox, session, routes) = match selected_name {
            Some(name) => match client.get_sandbox(&name, Some(true)) {
                Ok((sandbox, session, routes, _)) => (sandbox, session, routes),
                Err(SandboxError::Api {
                    status: Some(404), ..
                }) if sandbox_id.is_none() => create_sandbox(Some(name))?,
                Err(error) => return Err(error),
            },
            None => create_sandbox(vercel_config.sandbox_name.clone())?,
        };
        let working_directory = if session.cwd.is_empty() {
            sandbox
                .cwd
                .clone()
                .unwrap_or_else(|| DEFAULT_VERCEL_WORKING_DIRECTORY.to_string())
        } else {
            session.cwd.clone()
        };
        let timeout_ms = Some(session.timeout).or(vercel_config.timeout_ms);
        let environment_details = format!(
            "- Vercel Sandbox commands run in an isolated Amazon Linux microVM as the vercel-sandbox user\n- Use {} as the workspace directory unless an absolute path is required\n- Exposed ports resolve to https://<subdomain>.vercel.run through sandbox.domain(port)",
            working_directory
        );
        let sandbox = Self {
            client,
            inner: Mutex::new(VercelSandboxInner {
                last_snapshot_id: sandbox
                    .current_snapshot_id
                    .clone()
                    .or(session.source_snapshot_id.clone()),
                sandbox,
                session,
                routes,
                timeout_ms,
            }),
            env: config.options.env,
            current_branch,
            working_directory,
            environment_details,
        };

        if let Some(source) = source {
            if let Some(new_branch) = source.new_branch {
                let command = format!("git checkout -B {}", shell_quote(&new_branch));
                let _ = sandbox.exec(SandboxExecOptions::new(command));
            }
        }

        Ok(sandbox)
    }

    fn session_id(&self) -> SandboxResult<String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| SandboxError::internal("vercel sandbox state lock poisoned"))?;
        if inner.session.status.is_stopped() {
            let (sandbox, session, routes, _) =
                self.client.get_sandbox(&inner.sandbox.name, Some(true))?;
            inner.sandbox = sandbox;
            inner.session = session;
            inner.routes = routes;
        }
        Ok(inner.session.id.clone())
    }

    fn command_output(
        &self,
        command: &str,
        args: &[String],
        cwd: Option<&str>,
    ) -> SandboxResult<(VercelCommandData, String, String, bool)> {
        let session_id = self.session_id()?;
        let command_data =
            self.client
                .run_command_wait(&session_id, command, args, cwd, &self.env, false)?;
        let (stdout, stderr, truncated) =
            self.client.command_logs(&session_id, &command_data.id)?;
        Ok((command_data, stdout, stderr, truncated))
    }

    fn update_session(&self, session: VercelSandboxSession) -> SandboxResult<()> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| SandboxError::internal("vercel sandbox state lock poisoned"))?;
        inner.timeout_ms = Some(session.timeout);
        inner.session = session;
        Ok(())
    }
}

impl Sandbox for VercelSandbox {
    fn sandbox_type(&self) -> SandboxType {
        SandboxType::Vercel
    }

    fn working_directory(&self) -> &str {
        &self.working_directory
    }

    fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }

    fn current_branch(&self) -> Option<&str> {
        self.current_branch.as_deref()
    }

    fn environment_details(&self) -> Option<&str> {
        Some(&self.environment_details)
    }

    fn host(&self) -> Option<&str> {
        Some("vercel.run")
    }

    fn expires_at_ms(&self) -> Option<u64> {
        let inner = self.inner.lock().ok()?;
        session_expires_at(&inner.session)
    }

    fn timeout_ms(&self) -> Option<u64> {
        self.inner.lock().ok().and_then(|inner| inner.timeout_ms)
    }

    fn read_file(&self, path: &str) -> SandboxResult<String> {
        let buffer = self.read_file_buffer(path)?;
        String::from_utf8(buffer).map_err(|error| SandboxError::InvalidUtf8 {
            path: path.to_string(),
            message: error.to_string(),
        })
    }

    fn read_file_buffer(&self, path: &str) -> SandboxResult<Vec<u8>> {
        let session_id = self.session_id()?;
        self.client
            .read_file(&session_id, path, Some(&self.working_directory))?
            .ok_or_else(|| SandboxError::Io {
                operation: "read".to_string(),
                path: path.to_string(),
                message: "file not found".to_string(),
            })
    }

    fn write_file(&self, path: &str, content: &str) -> SandboxResult<()> {
        let session_id = self.session_id()?;
        self.client.write_files(
            &session_id,
            &self.working_directory,
            &[VercelWriteFile {
                path,
                content: content.as_bytes(),
                mode: None,
            }],
        )
    }

    fn stat(&self, path: &str) -> SandboxResult<SandboxStats> {
        let args = vec!["-c".to_string(), STAT_FORMAT.to_string(), path.to_string()];
        let (command, stdout, stderr, _) =
            self.command_output("stat", &args, Some(&self.working_directory))?;
        if command.exit_code != Some(0) {
            return Err(SandboxError::Io {
                operation: "stat".to_string(),
                path: path.to_string(),
                message: stderr,
            });
        }
        parse_stat(path, &stdout)
    }

    fn access(&self, path: &str) -> SandboxResult<()> {
        let args = vec!["-e".to_string(), path.to_string()];
        let (command, _stdout, stderr, _) =
            self.command_output("test", &args, Some(&self.working_directory))?;
        if command.exit_code == Some(0) {
            Ok(())
        } else {
            Err(SandboxError::Io {
                operation: "access".to_string(),
                path: path.to_string(),
                message: if stderr.is_empty() {
                    "path not found".to_string()
                } else {
                    stderr
                },
            })
        }
    }

    fn mkdir(&self, path: &str, options: SandboxMkdirOptions) -> SandboxResult<()> {
        let session_id = self.session_id()?;
        if options.recursive {
            let args = vec!["-p".to_string(), path.to_string()];
            let (command, _stdout, stderr, _) =
                self.command_output("mkdir", &args, Some(&self.working_directory))?;
            if command.exit_code != Some(0) {
                return Err(SandboxError::Io {
                    operation: "mkdir".to_string(),
                    path: path.to_string(),
                    message: stderr,
                });
            }
            return Ok(());
        }
        self.client
            .mkdir(&session_id, path, Some(&self.working_directory))
    }

    fn read_dir(&self, path: &str) -> SandboxResult<Vec<SandboxDirEntry>> {
        let args = vec![
            path.to_string(),
            "-maxdepth".to_string(),
            "1".to_string(),
            "-mindepth".to_string(),
            "1".to_string(),
            "-printf".to_string(),
            "%f|%y\n".to_string(),
        ];
        let (command, stdout, stderr, _) =
            self.command_output("find", &args, Some(&self.working_directory))?;
        if command.exit_code != Some(0) {
            return Err(SandboxError::Io {
                operation: "read_dir".to_string(),
                path: path.to_string(),
                message: stderr,
            });
        }
        let mut entries = stdout
            .lines()
            .filter_map(|line| {
                let (name, kind) = line.split_once('|')?;
                Some(SandboxDirEntry {
                    name: name.to_string(),
                    kind: match kind {
                        "d" => SandboxDirEntryKind::Directory,
                        "f" => SandboxDirEntryKind::File,
                        "l" => SandboxDirEntryKind::Symlink,
                        _ => SandboxDirEntryKind::Other,
                    },
                })
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    fn exec(&self, options: SandboxExecOptions) -> SandboxResult<SandboxExecResult> {
        let args = vec!["-lc".to_string(), options.command.clone()];
        let session_id = self.session_id()?;
        let command = self.client.run_command_wait(
            &session_id,
            "/bin/bash",
            &args,
            options.cwd.as_deref().or(Some(&self.working_directory)),
            &self.env,
            false,
        )?;
        let (stdout, stderr, truncated) = self.client.command_logs(&session_id, &command.id)?;
        Ok(SandboxExecResult {
            success: command.exit_code == Some(0),
            exit_code: command.exit_code,
            stdout,
            stderr,
            truncated,
        })
    }

    fn exec_detached(
        &self,
        options: SandboxDetachedOptions,
    ) -> SandboxResult<SandboxDetachedCommand> {
        let args = vec!["-lc".to_string(), options.command];
        let session_id = self.session_id()?;
        let command = self.client.run_command_detached(
            &session_id,
            "/bin/bash",
            &args,
            options.cwd.as_deref().or(Some(&self.working_directory)),
            &self.env,
            false,
        )?;
        Ok(SandboxDetachedCommand {
            command_id: command.id,
        })
    }

    fn set_github_auth_token(&self, _token: Option<&str>) -> SandboxResult<()> {
        Ok(())
    }

    fn domain(&self, port: u16) -> Option<String> {
        let inner = self.inner.lock().ok()?;
        inner
            .routes
            .iter()
            .find(|route| route.port == port)
            .map(|route| {
                if route.url.is_empty() {
                    format!("https://{}.vercel.run", route.subdomain)
                } else {
                    route.url.clone()
                }
            })
    }

    fn stop(&self) -> SandboxResult<()> {
        let session_id = self.session_id()?;
        let (session, sandbox, snapshot_id) = self.client.stop_session(&session_id)?;
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| SandboxError::internal("vercel sandbox state lock poisoned"))?;
        if let Some(sandbox) = sandbox {
            inner.sandbox = sandbox;
        }
        if let Some(snapshot_id) = snapshot_id {
            inner.last_snapshot_id = Some(snapshot_id);
        }
        inner.timeout_ms = Some(session.timeout);
        inner.session = session;
        Ok(())
    }

    fn extend_timeout(&self, additional_ms: u64) -> SandboxResult<SandboxTimeoutExtension> {
        let session_id = self.session_id()?;
        let session = self.client.extend_timeout(&session_id, additional_ms)?;
        let expires_at = session_expires_at(&session).unwrap_or_default();
        self.update_session(session)?;
        Ok(SandboxTimeoutExtension { expires_at })
    }

    fn snapshot(&self) -> SandboxResult<SnapshotResult> {
        let session_id = self.session_id()?;
        let (snapshot_id, session) = self.client.create_snapshot(&session_id)?;
        self.update_session(session)?;
        if let Ok(mut inner) = self.inner.lock() {
            inner.last_snapshot_id = Some(snapshot_id.clone());
        }
        Ok(SnapshotResult { snapshot_id })
    }

    fn state(&self) -> SandboxState {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        SandboxState::Vercel {
            source: None,
            sandbox_name: Some(inner.sandbox.name.clone()),
            sandbox_id: Some(inner.session.id.clone()),
            snapshot_id: inner
                .sandbox
                .current_snapshot_id
                .clone()
                .or_else(|| inner.last_snapshot_id.clone())
                .or_else(|| inner.session.source_snapshot_id.clone()),
            expires_at: session_expires_at(&inner.session),
        }
    }
}

fn source_to_vercel_source(source: &SandboxSource) -> VercelSandboxUpstreamSource {
    VercelSandboxUpstreamSource::Git {
        url: source.repo.clone(),
        depth: None,
        revision: source.branch.clone(),
        username: None,
        password: None,
    }
}

fn parse_json_response<T: DeserializeOwned>(
    operation: &str,
    response: VercelSandboxHttpResponse,
) -> SandboxResult<T> {
    ensure_success(operation, &response)?;
    serde_json::from_slice(&response.body).map_err(|error| SandboxError::Api {
        operation: operation.to_string(),
        status: Some(response.status),
        message: error.to_string(),
    })
}

fn read_http_response(
    operation: &str,
    response: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> SandboxResult<VercelSandboxHttpResponse> {
    let mut response = response.map_err(|error| SandboxError::Api {
        operation: operation.to_string(),
        status: None,
        message: error.to_string(),
    })?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_ascii_lowercase(), value.to_string()))
        })
        .collect();
    let body = response
        .body_mut()
        .read_to_vec()
        .map_err(|error| SandboxError::Api {
            operation: operation.to_string(),
            status: Some(status),
            message: error.to_string(),
        })?;
    Ok(VercelSandboxHttpResponse {
        status,
        headers,
        body,
    })
}

fn ensure_success(operation: &str, response: &VercelSandboxHttpResponse) -> SandboxResult<()> {
    if (200..300).contains(&response.status) {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&response.body);
    let message = serde_json::from_slice::<Value>(&response.body)
        .ok()
        .and_then(|json| {
            json.pointer("/error/message")
                .or_else(|| json.pointer("/message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| text.to_string());
    Err(SandboxError::Api {
        operation: operation.to_string(),
        status: Some(response.status),
        message,
    })
}

fn ensure_content_type(
    operation: &str,
    response: &VercelSandboxHttpResponse,
    expected: &str,
) -> SandboxResult<()> {
    let content_type = response
        .headers
        .get("content-type")
        .map(String::as_str)
        .unwrap_or("");
    if content_type.contains(expected) {
        return Ok(());
    }
    Err(SandboxError::Api {
        operation: operation.to_string(),
        status: Some(response.status),
        message: format!(
            "expected content-type containing {expected}, got {}",
            if content_type.is_empty() {
                "(none)"
            } else {
                content_type
            }
        ),
    })
}

fn parse_ndjson<T: DeserializeOwned>(operation: &str, body: &[u8]) -> SandboxResult<Vec<T>> {
    let text = String::from_utf8_lossy(body);
    let mut parsed = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        parsed.push(
            serde_json::from_str(line).map_err(|error| SandboxError::Api {
                operation: operation.to_string(),
                status: None,
                message: format!("invalid ndjson line {line:?}: {error}"),
            })?,
        );
    }
    Ok(parsed)
}

fn append_query(url: &mut String, key: &str, value: &str) {
    if url.contains('?') {
        url.push('&');
    } else {
        url.push('?');
    }
    url.push_str(&encode_component(key));
    url.push('=');
    url.push_str(&encode_component(value));
}

fn encode_component(input: &str) -> String {
    let mut output = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(char::from(byte));
            }
            _ => {
                output.push('%');
                output.push_str(&format!("{byte:02X}"));
            }
        }
    }
    output
}

fn encode_write_files_archive(cwd: &str, files: &[VercelWriteFile<'_>]) -> SandboxResult<Vec<u8>> {
    let encoder = GzEncoder::new(Vec::new(), Compression::default());
    let mut archive = TarBuilder::new(encoder);
    for file in files {
        let name = normalize_upload_path(file.path, cwd, "/")?;
        let mut header = Header::new_gnu();
        header.set_size(u64::try_from(file.content.len()).unwrap_or(u64::MAX));
        header.set_mode(file.mode.unwrap_or(0o644));
        header.set_cksum();
        archive
            .append_data(&mut header, name, file.content)
            .map_err(|error| SandboxError::Api {
                operation: "write files".to_string(),
                status: None,
                message: error.to_string(),
            })?;
    }
    let encoder = archive.into_inner().map_err(|error| SandboxError::Api {
        operation: "write files".to_string(),
        status: None,
        message: error.to_string(),
    })?;
    encoder.finish().map_err(|error| SandboxError::Api {
        operation: "write files".to_string(),
        status: None,
        message: error.to_string(),
    })
}

fn normalize_upload_path(path: &str, cwd: &str, extract_dir: &str) -> SandboxResult<String> {
    if !cwd.starts_with('/') {
        return Err(SandboxError::Api {
            operation: "write files".to_string(),
            status: None,
            message: "cwd dir must be absolute".to_string(),
        });
    }
    if !extract_dir.starts_with('/') {
        return Err(SandboxError::Api {
            operation: "write files".to_string(),
            status: None,
            message: "extractDir must be absolute".to_string(),
        });
    }
    let joined = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{}/{}", cwd.trim_end_matches('/'), path)
    };
    let normalized = normalize_posix_path(&joined);
    let extract_dir = normalize_posix_path(extract_dir);
    Ok(normalized
        .strip_prefix(extract_dir.trim_end_matches('/'))
        .unwrap_or(&normalized)
        .trim_start_matches('/')
        .to_string())
}

fn normalize_posix_path(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    let mut normalized = if absolute {
        "/".to_string()
    } else {
        String::new()
    };
    normalized.push_str(&parts.join("/"));
    if normalized.is_empty() {
        ".".to_string()
    } else {
        normalized
    }
}

fn parse_stat(_path: &str, stdout: &str) -> SandboxResult<SandboxStats> {
    let parts = stdout.trim().split('|').collect::<Vec<_>>();
    if parts.len() < 3 {
        return Err(SandboxError::Api {
            operation: "stat".to_string(),
            status: None,
            message: format!("invalid stat output {stdout:?}"),
        });
    }
    let size = parts[0].parse::<u64>().map_err(|error| SandboxError::Api {
        operation: "stat".to_string(),
        status: None,
        message: error.to_string(),
    })?;
    let mode = u32::from_str_radix(parts[1], 16).map_err(|error| SandboxError::Api {
        operation: "stat".to_string(),
        status: None,
        message: error.to_string(),
    })?;
    let mtime_ms = parts[2]
        .parse::<u64>()
        .map(|seconds| seconds.saturating_mul(1_000))
        .map_err(|error| SandboxError::Api {
            operation: "stat".to_string(),
            status: None,
            message: error.to_string(),
        })?;
    Ok(SandboxStats {
        is_directory: mode & 0o170000 == 0o040000,
        is_file: mode & 0o170000 == 0o100000,
        size,
        mtime_ms,
    })
}

fn append_capped(target: &mut String, data: &str, truncated: &mut bool) {
    let remaining = DEFAULT_MAX_OUTPUT_LENGTH.saturating_sub(target.len());
    if remaining == 0 {
        *truncated = true;
        return;
    }
    if data.len() <= remaining {
        target.push_str(data);
        return;
    }
    let mut end = remaining;
    while !data.is_char_boundary(end) {
        end -= 1;
    }
    target.push_str(&data[..end]);
    *truncated = true;
}

fn session_expires_at(session: &VercelSandboxSession) -> Option<u64> {
    let start = session.started_at.unwrap_or(session.requested_at);
    if start == 0 || session.timeout == 0 {
        return None;
    }
    Some(start.saturating_add(session.timeout))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
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

fn parse_optional_u32(value: Option<&str>, name: &'static str) -> SandboxResult<Option<u32>> {
    match value.and_then(|value| present(Some(value.to_string()))) {
        Some(value) => value
            .parse::<u32>()
            .map(Some)
            .map_err(|error| SandboxError::Api {
                operation: "parse config".to_string(),
                status: None,
                message: format!("{name}={value:?} is invalid: {error}"),
            }),
        None => Ok(None),
    }
}

fn parse_optional_u64(value: Option<&str>, name: &'static str) -> SandboxResult<Option<u64>> {
    match value.and_then(|value| present(Some(value.to_string()))) {
        Some(value) => value
            .parse::<u64>()
            .map(Some)
            .map_err(|error| SandboxError::Api {
                operation: "parse config".to_string(),
                status: None,
                message: format!("{name}={value:?} is invalid: {error}"),
            }),
        None => Ok(None),
    }
}

fn parse_optional_bool(value: Option<&str>, name: &'static str) -> SandboxResult<Option<bool>> {
    match value.and_then(|value| present(Some(value.to_string()))) {
        Some(value) => match value.as_str() {
            "1" | "true" | "yes" => Ok(Some(true)),
            "0" | "false" | "no" => Ok(Some(false)),
            _ => Err(SandboxError::Api {
                operation: "parse config".to_string(),
                status: None,
                message: format!("{name}={value:?} is invalid: expected true or false"),
            }),
        },
        None => Ok(None),
    }
}

#[cfg(test)]
fn gzip_tar_contains_path(body: &[u8], expected_path: &str) -> std::io::Result<bool> {
    let decoder = flate2::read::GzDecoder::new(body);
    let mut archive = tar::Archive::new(decoder);
    for entry in archive.entries()? {
        let entry = entry?;
        if entry.path()?.to_string_lossy() == expected_path {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::thread;

    #[derive(Clone, Debug)]
    struct RecordedRequest {
        method: String,
        path: String,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    }

    struct MockResponse {
        status: u16,
        content_type: &'static str,
        body: Vec<u8>,
    }

    struct MockVercelServer {
        base_url: String,
        requests: Arc<Mutex<Vec<RecordedRequest>>>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl MockVercelServer {
        fn new(responses: Vec<MockResponse>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock server");
            let base_url = format!("http://{}", listener.local_addr().expect("local addr"));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let captured = Arc::clone(&requests);
            let handle = thread::spawn(move || {
                for response in responses {
                    let (stream, _) = listener.accept().expect("accept request");
                    let request = read_request(stream, response);
                    captured.lock().expect("requests lock").push(request);
                }
            });
            Self {
                base_url,
                requests,
                handle: Some(handle),
            }
        }

        fn client(&self) -> VercelSandboxClient {
            VercelSandboxClient::new(
                VercelSandboxConfig::new(VercelSandboxCredentials::new(
                    "token", "team_123", "proj_123",
                ))
                .with_base_url(&self.base_url),
            )
        }

        fn requests(&self) -> Vec<RecordedRequest> {
            self.requests.lock().expect("requests lock").clone()
        }
    }

    impl Drop for MockVercelServer {
        fn drop(&mut self) {
            if let Some(handle) = self.handle.take() {
                handle.join().expect("mock server thread joins");
            }
        }
    }

    fn read_request(mut stream: TcpStream, response: MockResponse) -> RecordedRequest {
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut request_line = String::new();
        reader.read_line(&mut request_line).expect("request line");
        let mut headers = BTreeMap::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("header line");
            if line == "\r\n" {
                break;
            }
            if let Some((name, value)) = line.split_once(':') {
                headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
            }
        }
        let content_length = headers
            .get("content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = vec![0_u8; content_length];
        reader.read_exact(&mut body).expect("request body");

        let response_text = format!(
            "HTTP/1.1 {} OK\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            response.status,
            response.content_type,
            response.body.len()
        );
        stream
            .write_all(response_text.as_bytes())
            .expect("write response head");
        stream
            .write_all(&response.body)
            .expect("write response body");

        let (method, path) = request_line
            .split_once(' ')
            .and_then(|(method, rest)| rest.split_once(' ').map(|(path, _)| (method, path)))
            .expect("request line shape");
        RecordedRequest {
            method: method.to_string(),
            path: path.to_string(),
            headers,
            body,
        }
    }

    fn json_response(value: Value) -> MockResponse {
        MockResponse {
            status: 200,
            content_type: "application/json",
            body: serde_json::to_vec(&value).expect("json response"),
        }
    }

    fn ndjson_response(values: &[Value]) -> MockResponse {
        let mut body = Vec::new();
        for value in values {
            serde_json::to_writer(&mut body, value).expect("ndjson line");
            body.push(b'\n');
        }
        MockResponse {
            status: 200,
            content_type: "application/x-ndjson",
            body,
        }
    }

    fn sandbox_response(name: &str, session_id: &str) -> Value {
        serde_json::json!({
            "sandbox": {
                "name": name,
                "persistent": true,
                "currentSessionId": session_id,
                "status": "running",
                "runtime": "node24",
                "timeout": 300000,
                "createdAt": 1,
                "updatedAt": 2,
                "cwd": "/vercel/sandbox"
            },
            "session": session_value(session_id),
            "routes": [{
                "url": "https://agent-3000.vercel.run",
                "subdomain": "agent-3000",
                "port": 3000
            }]
        })
    }

    fn session_value(session_id: &str) -> Value {
        serde_json::json!({
            "id": session_id,
            "memory": 2048,
            "vcpus": 1,
            "region": "iad1",
            "runtime": "node24",
            "timeout": 300000,
            "status": "running",
            "requestedAt": 1000,
            "startedAt": 2000,
            "createdAt": 1000,
            "cwd": "/vercel/sandbox",
            "updatedAt": 2000
        })
    }

    #[test]
    fn vercel_client_create_sandbox_sends_upstream_shape() {
        let server =
            MockVercelServer::new(vec![json_response(sandbox_response("oa-test", "sess_123"))]);
        let client = server.client();

        let (sandbox, session, routes) = client
            .create_sandbox(&VercelSandboxCreateRequest {
                project_id: "proj_123".to_string(),
                name: Some("oa-test".to_string()),
                source: Some(VercelSandboxUpstreamSource::Snapshot {
                    snapshot_id: "snap_123".to_string(),
                }),
                ports: vec![3000],
                timeout: Some(60000),
                resources: Some(VercelSandboxResources { vcpus: 2 }),
                runtime: Some("node24".to_string()),
                env: BTreeMap::from([("RUST_LOG".to_string(), "info".to_string())]),
                tags: BTreeMap::new(),
                persistent: Some(true),
            })
            .expect("create sandbox");

        assert_eq!(sandbox.name, "oa-test");
        assert_eq!(session.id, "sess_123");
        assert_eq!(routes[0].port, 3000);

        let requests = server.requests();
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/v2/sandboxes?teamId=team_123");
        assert_eq!(
            requests[0].headers.get("authorization").map(String::as_str),
            Some("Bearer token")
        );
        let body: Value = serde_json::from_slice(&requests[0].body).expect("request json");
        assert_eq!(body["projectId"], "proj_123");
        assert_eq!(
            body["source"],
            serde_json::json!({"type": "snapshot", "snapshotId": "snap_123"})
        );
        assert_eq!(body["ports"], serde_json::json!([3000]));
        assert_eq!(body["resources"], serde_json::json!({"vcpus": 2}));
        assert_eq!(body["env"], serde_json::json!({"RUST_LOG": "info"}));
    }

    #[test]
    fn vercel_client_get_sandbox_passes_project_and_resume_query() {
        let server =
            MockVercelServer::new(vec![json_response(sandbox_response("oa name", "sess_123"))]);
        let client = server.client();

        client
            .get_sandbox("oa name", Some(true))
            .expect("get sandbox");

        let requests = server.requests();
        assert_eq!(requests[0].method, "GET");
        assert_eq!(
            requests[0].path,
            "/v2/sandboxes/oa%20name?teamId=team_123&projectId=proj_123&resume=true"
        );
    }

    #[test]
    fn vercel_connect_creates_named_sandbox_when_missing() {
        let server = MockVercelServer::new(vec![
            MockResponse {
                status: 404,
                content_type: "application/json",
                body: serde_json::to_vec(&serde_json::json!({
                    "error": { "message": "not found" }
                }))
                .expect("json error"),
            },
            json_response(sandbox_response("oa-new", "sess_new")),
        ]);
        let config = VercelSandboxConfig::new(VercelSandboxCredentials::new(
            "token", "team_123", "proj_123",
        ))
        .with_base_url(&server.base_url);

        let sandbox = VercelSandbox::connect_with_config(
            SandboxConnectConfig::new(SandboxState::Vercel {
                source: None,
                sandbox_name: Some("oa-new".to_string()),
                sandbox_id: None,
                snapshot_id: None,
                expires_at: None,
            }),
            config,
        )
        .expect("connect named sandbox");

        assert_eq!(sandbox.state().sandbox_type(), SandboxType::Vercel);
        let requests = server.requests();
        assert_eq!(requests[0].method, "GET");
        assert_eq!(
            requests[0].path,
            "/v2/sandboxes/oa-new?teamId=team_123&projectId=proj_123&resume=true"
        );
        assert_eq!(requests[1].method, "POST");
        assert_eq!(requests[1].path, "/v2/sandboxes?teamId=team_123");
        let body: Value = serde_json::from_slice(&requests[1].body).expect("request json");
        assert_eq!(body["name"], "oa-new");
        assert_eq!(body["projectId"], "proj_123");
    }

    #[test]
    fn vercel_client_list_sandboxes_passes_project_and_cursor_query() {
        let server = MockVercelServer::new(vec![json_response(serde_json::json!({
            "sandboxes": [{
                "name": "oa-a",
                "persistent": true,
                "currentSessionId": "sess_a",
                "status": "running",
                "createdAt": 1,
                "updatedAt": 2
            }],
            "pagination": { "count": 1, "next": null }
        }))]);
        let client = server.client();

        let sandboxes = client
            .list_sandboxes(Some(10), Some("cursor_1"))
            .expect("list sandboxes");

        assert_eq!(sandboxes[0].name, "oa-a");
        let requests = server.requests();
        assert_eq!(
            requests[0].path,
            "/v2/sandboxes?teamId=team_123&project=proj_123&limit=10&cursor=cursor_1"
        );
    }

    #[test]
    fn vercel_client_run_command_wait_reads_finished_chunk_and_logs() {
        let command_start = serde_json::json!({
            "command": {
                "id": "cmd_123",
                "name": "bash",
                "args": ["-lc", "pwd"],
                "cwd": "/vercel/sandbox",
                "sessionId": "sess_123",
                "exitCode": null,
                "startedAt": 1
            }
        });
        let command_finished = serde_json::json!({
            "command": {
                "id": "cmd_123",
                "name": "bash",
                "args": ["-lc", "pwd"],
                "cwd": "/vercel/sandbox",
                "sessionId": "sess_123",
                "exitCode": 0,
                "startedAt": 1
            }
        });
        let server = MockVercelServer::new(vec![
            ndjson_response(&[command_start, command_finished]),
            ndjson_response(&[
                serde_json::json!({"stream": "stdout", "data": "/vercel/sandbox\n"}),
                serde_json::json!({"stream": "stderr", "data": ""}),
            ]),
        ]);
        let client = server.client();

        let command = client
            .run_command_wait(
                "sess_123",
                "/bin/bash",
                &["-lc".to_string(), "pwd".to_string()],
                Some("/vercel/sandbox"),
                &BTreeMap::new(),
                false,
            )
            .expect("run command");
        let (stdout, stderr, truncated) = client
            .command_logs("sess_123", &command.id)
            .expect("command logs");

        assert_eq!(command.exit_code, Some(0));
        assert_eq!(stdout, "/vercel/sandbox\n");
        assert_eq!(stderr, "");
        assert!(!truncated);
        let requests = server.requests();
        assert_eq!(
            requests[0].path,
            "/v2/sandboxes/sessions/sess_123/cmd?teamId=team_123"
        );
        let body: Value = serde_json::from_slice(&requests[0].body).expect("request json");
        assert_eq!(body["command"], "/bin/bash");
        assert_eq!(body["args"], serde_json::json!(["-lc", "pwd"]));
        assert_eq!(body["wait"], true);
        assert_eq!(
            requests[1].path,
            "/v2/sandboxes/sessions/sess_123/cmd/cmd_123/logs?teamId=team_123"
        );
    }

    #[test]
    fn vercel_client_read_file_maps_404_to_none_and_content_type_errors() {
        let server = MockVercelServer::new(vec![
            MockResponse {
                status: 404,
                content_type: "application/json",
                body: br#"{"error":{"message":"missing"}}"#.to_vec(),
            },
            MockResponse {
                status: 200,
                content_type: "application/json",
                body: br#"{"not":"a file"}"#.to_vec(),
            },
        ]);
        let client = server.client();

        assert_eq!(
            client
                .read_file("sess_123", "missing.txt", Some("/vercel/sandbox"))
                .expect("404 read"),
            None
        );
        let error = client
            .read_file("sess_123", "bad.txt", Some("/vercel/sandbox"))
            .expect_err("content-type error");
        assert!(error.to_string().contains("content-type"));
    }

    #[test]
    fn vercel_client_api_errors_include_status_and_message() {
        let server = MockVercelServer::new(vec![MockResponse {
            status: 422,
            content_type: "application/json",
            body: br#"{"error":{"message":"sandbox is snapshotting"}}"#.to_vec(),
        }]);
        let client = server.client();

        let error = client
            .get_sandbox("oa-test", Some(true))
            .expect_err("api error");

        assert!(matches!(
            error,
            SandboxError::Api {
                status: Some(422),
                ..
            }
        ));
        assert!(error.to_string().contains("sandbox is snapshotting"));
    }

    #[test]
    fn vercel_client_write_files_uses_gzip_tar_upload_shape() {
        let server = MockVercelServer::new(vec![json_response(serde_json::json!({}))]);
        let client = server.client();

        client
            .write_files(
                "sess_123",
                "/vercel/sandbox",
                &[VercelWriteFile {
                    path: "src/main.rs",
                    content: b"fn main() {}\n",
                    mode: Some(0o644),
                }],
            )
            .expect("write files");

        let requests = server.requests();
        assert_eq!(requests[0].method, "POST");
        assert_eq!(
            requests[0].path,
            "/v2/sandboxes/sessions/sess_123/fs/write?teamId=team_123"
        );
        assert_eq!(
            requests[0].headers.get("content-type").map(String::as_str),
            Some("application/gzip")
        );
        assert_eq!(
            requests[0].headers.get("x-cwd").map(String::as_str),
            Some("/")
        );
        assert!(
            gzip_tar_contains_path(&requests[0].body, "vercel/sandbox/src/main.rs")
                .expect("tar can be read")
        );
    }

    #[test]
    fn vercel_client_extend_timeout_and_snapshot_parse_session_updates() {
        let server = MockVercelServer::new(vec![
            json_response(serde_json::json!({
                "session": {
                    "id": "sess_123",
                    "memory": 2048,
                    "vcpus": 1,
                    "region": "iad1",
                    "runtime": "node24",
                    "timeout": 360000,
                    "status": "running",
                    "requestedAt": 1000,
                    "startedAt": 2000,
                    "createdAt": 1000,
                    "cwd": "/vercel/sandbox",
                    "updatedAt": 4000
                }
            })),
            json_response(serde_json::json!({
                "snapshot": {
                    "id": "snap_123",
                    "sourceSessionId": "sess_123",
                    "status": "created",
                    "sizeBytes": 1,
                    "createdAt": 4000,
                    "updatedAt": 4000
                },
                "session": {
                    "id": "sess_123",
                    "memory": 2048,
                    "vcpus": 1,
                    "region": "iad1",
                    "runtime": "node24",
                    "timeout": 360000,
                    "status": "stopped",
                    "requestedAt": 1000,
                    "startedAt": 2000,
                    "createdAt": 1000,
                    "cwd": "/vercel/sandbox",
                    "updatedAt": 5000
                }
            })),
        ]);
        let client = server.client();

        let extended = client
            .extend_timeout("sess_123", 60000)
            .expect("extend timeout");
        let (snapshot_id, snapshotted) =
            client.create_snapshot("sess_123").expect("create snapshot");

        assert_eq!(extended.timeout, 360000);
        assert_eq!(snapshot_id, "snap_123");
        assert_eq!(snapshotted.status, VercelSandboxStatus::Stopped);
        let requests = server.requests();
        assert_eq!(
            requests[0].path,
            "/v2/sandboxes/sessions/sess_123/extend-timeout?teamId=team_123"
        );
        assert_eq!(
            requests[1].path,
            "/v2/sandboxes/sessions/sess_123/snapshot?teamId=team_123"
        );
    }

    #[test]
    fn vercel_sandbox_backend_connects_execs_reads_writes_lists_and_stops() {
        let command_finished = |id: &str, command: &str, exit_code: i32| {
            ndjson_response(&[
                serde_json::json!({
                    "command": {
                        "id": id,
                        "name": command,
                        "args": [],
                        "cwd": "/vercel/sandbox",
                        "sessionId": "sess_123",
                        "exitCode": null,
                        "startedAt": 1
                    }
                }),
                serde_json::json!({
                    "command": {
                        "id": id,
                        "name": command,
                        "args": [],
                        "cwd": "/vercel/sandbox",
                        "sessionId": "sess_123",
                        "exitCode": exit_code,
                        "startedAt": 1
                    }
                }),
            ])
        };
        let server = MockVercelServer::new(vec![
            json_response(sandbox_response("oa-test", "sess_123")),
            command_finished("cmd_pwd", "/bin/bash", 0),
            ndjson_response(&[
                serde_json::json!({"stream": "stdout", "data": "/vercel/sandbox\n"}),
            ]),
            MockResponse {
                status: 200,
                content_type: "application/octet-stream",
                body: b"hello".to_vec(),
            },
            json_response(serde_json::json!({})),
            command_finished("cmd_find", "find", 0),
            ndjson_response(&[
                serde_json::json!({"stream": "stdout", "data": "main.rs|f\nsrc|d\n"}),
            ]),
            command_finished("cmd_stat", "stat", 0),
            ndjson_response(&[serde_json::json!({"stream": "stdout", "data": "13|81a4|2\n"})]),
            json_response(serde_json::json!({
                "session": {
                    "id": "sess_123",
                    "memory": 2048,
                    "vcpus": 1,
                    "region": "iad1",
                    "runtime": "node24",
                    "timeout": 300000,
                    "status": "stopped",
                    "requestedAt": 1000,
                    "startedAt": 2000,
                    "createdAt": 1000,
                    "cwd": "/vercel/sandbox",
                    "updatedAt": 3000
                },
                "sandbox": {
                    "name": "oa-test",
                    "persistent": true,
                    "currentSessionId": "sess_123",
                    "currentSnapshotId": "snap_123",
                    "status": "stopped",
                    "createdAt": 1,
                    "updatedAt": 3
                },
                "snapshot": {
                    "id": "snap_123",
                    "sourceSessionId": "sess_123",
                    "status": "created",
                    "sizeBytes": 1,
                    "createdAt": 3,
                    "updatedAt": 3
                }
            })),
        ]);
        let config = VercelSandboxConfig::new(VercelSandboxCredentials::new(
            "token", "team_123", "proj_123",
        ))
        .with_base_url(&server.base_url)
        .with_sandbox_name("oa-test");
        let sandbox = VercelSandbox::connect_with_config(
            SandboxConnectConfig::new(SandboxState::Vercel {
                source: None,
                sandbox_name: Some("oa-test".to_string()),
                sandbox_id: None,
                snapshot_id: None,
                expires_at: None,
            }),
            config,
        )
        .expect("connect vercel sandbox");

        let result = sandbox
            .exec(SandboxExecOptions::new("pwd"))
            .expect("exec pwd");
        assert_eq!(result.stdout, "/vercel/sandbox\n");
        assert_eq!(sandbox.read_file("hello.txt").expect("read file"), "hello");
        sandbox
            .write_file("src/main.rs", "fn main() {}\n")
            .expect("write file");
        assert_eq!(
            sandbox.read_dir(".").expect("read dir"),
            vec![
                SandboxDirEntry {
                    name: "main.rs".to_string(),
                    kind: SandboxDirEntryKind::File,
                },
                SandboxDirEntry {
                    name: "src".to_string(),
                    kind: SandboxDirEntryKind::Directory,
                },
            ]
        );
        let stats = sandbox.stat("src/main.rs").expect("stat file");
        assert!(stats.is_file);
        assert!(!stats.is_directory);
        assert_eq!(stats.size, 13);
        assert_eq!(
            sandbox.domain(3000),
            Some("https://agent-3000.vercel.run".to_string())
        );
        sandbox.stop().expect("stop");
        assert_eq!(
            sandbox.state(),
            SandboxState::Vercel {
                source: None,
                sandbox_name: Some("oa-test".to_string()),
                sandbox_id: Some("sess_123".to_string()),
                snapshot_id: Some("snap_123".to_string()),
                expires_at: Some(302000),
            }
        );
    }

    #[test]
    fn vercel_config_from_reader_reports_missing_credentials_and_parses_options() {
        let error = VercelSandboxConfig::from_reader(|_| None).expect_err("missing token");
        assert!(matches!(
            error,
            SandboxError::MissingConfig {
                name: OPEN_AGENTS_VERCEL_TOKEN_ENV
            }
        ));

        let config = VercelSandboxConfig::from_reader(|name| match name {
            OPEN_AGENTS_VERCEL_TOKEN_ENV => Some("token".to_string()),
            VERCEL_TEAM_ID_ENV => Some("team".to_string()),
            VERCEL_PROJECT_ID_ENV => Some("project".to_string()),
            VERCEL_SANDBOX_NAME_ENV => Some("oa".to_string()),
            VERCEL_SANDBOX_RUNTIME_ENV => Some("node24".to_string()),
            VERCEL_SANDBOX_VCPUS_ENV => Some("2".to_string()),
            VERCEL_SANDBOX_TIMEOUT_MS_ENV => Some("60000".to_string()),
            VERCEL_SANDBOX_PERSISTENT_ENV => Some("true".to_string()),
            _ => None,
        })
        .expect("config");

        assert_eq!(config.credentials.team_id, "team");
        assert_eq!(config.sandbox_name.as_deref(), Some("oa"));
        assert_eq!(config.runtime.as_deref(), Some("node24"));
        assert_eq!(config.vcpus, Some(2));
        assert_eq!(config.timeout_ms, Some(60000));
        assert_eq!(config.persistent, Some(true));
    }

    #[test]
    fn vercel_config_from_reader_accepts_legacy_vercel_token_env() {
        let config = VercelSandboxConfig::from_reader(|name| match name {
            VERCEL_TOKEN_ENV => Some("legacy-token".to_string()),
            VERCEL_TEAM_ID_ENV => Some("team".to_string()),
            VERCEL_PROJECT_ID_ENV => Some("project".to_string()),
            _ => None,
        })
        .expect("config");

        assert_eq!(config.credentials.token, "legacy-token");
    }

    #[test]
    #[ignore = "requires live Vercel Sandbox credentials and creates a real sandbox"]
    fn live_vercel_sandbox_create_exec_read_write_list_stop_smoke() {
        let Ok(config) = VercelSandboxConfig::from_env() else {
            eprintln!("missing Vercel sandbox credentials; skipping live smoke");
            return;
        };
        let sandbox_name = format!(
            "ai-sdk-rust-live-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_millis()
        );
        let config = config
            .with_sandbox_name(&sandbox_name)
            .with_persistent(false);
        let sandbox = VercelSandbox::connect_with_config(
            SandboxConnectConfig::new(SandboxState::Vercel {
                source: None,
                sandbox_name: None,
                sandbox_id: None,
                snapshot_id: None,
                expires_at: None,
            }),
            config,
        )
        .expect("connect live sandbox");

        let result = sandbox
            .exec(SandboxExecOptions::new("printf '%s' \"$PWD\""))
            .expect("exec live pwd");
        assert!(result.success);
        assert_eq!(result.stdout, DEFAULT_VERCEL_WORKING_DIRECTORY);
        let toolchain = sandbox
            .exec(SandboxExecOptions::new("command -v git && git --version"))
            .expect("check live git toolchain");
        assert!(
            toolchain.success,
            "expected live sandbox to include git: {toolchain:#?}"
        );
        sandbox
            .write_file("open-agents-live.txt", "ok\n")
            .expect("write live file");
        assert_eq!(
            sandbox
                .read_file("open-agents-live.txt")
                .expect("read live file"),
            "ok\n"
        );
        let listed = sandbox.read_dir(".").expect("list live workspace");
        assert!(
            listed
                .iter()
                .any(|entry| entry.name == "open-agents-live.txt")
        );
        sandbox.stop().expect("stop live sandbox");
    }
}
