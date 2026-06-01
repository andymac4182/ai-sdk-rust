//! Sandbox boundary for Open Agents remote-agent execution.

#![forbid(unsafe_code)]

pub mod git;
pub mod git_finish;
pub mod just_bash;
pub mod vercel;

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// The maximum captured stdout or stderr length returned by sandbox commands.
pub const DEFAULT_MAX_OUTPUT_LENGTH: usize = 50_000;

/// Default timeout for sandbox commands, in milliseconds.
pub const DEFAULT_EXEC_TIMEOUT_MS: u64 = 30_000;

/// Bucket that owns the first sandbox connector implementation.
pub const OWNER_BUCKET: u8 = 3;

/// Optional base snapshot id for Vercel-backed sandboxes.
pub const VERCEL_SANDBOX_BASE_SNAPSHOT_ID_ENV: &str = "VERCEL_SANDBOX_BASE_SNAPSHOT_ID";
/// Optional API base URL override for Vercel Sandbox tests and private gateways.
pub const VERCEL_SANDBOX_API_BASE_URL_ENV: &str = "VERCEL_SANDBOX_API_BASE_URL";
/// Optional stable named sandbox to resume instead of creating a new sandbox.
pub const VERCEL_SANDBOX_NAME_ENV: &str = "VERCEL_SANDBOX_NAME";
/// Optional Vercel Sandbox runtime. Defaults to the upstream SDK default.
pub const VERCEL_SANDBOX_RUNTIME_ENV: &str = "VERCEL_SANDBOX_RUNTIME";
/// Optional Vercel Sandbox vCPU count.
pub const VERCEL_SANDBOX_VCPUS_ENV: &str = "VERCEL_SANDBOX_VCPUS";
/// Optional Vercel Sandbox timeout, in milliseconds.
pub const VERCEL_SANDBOX_TIMEOUT_MS_ENV: &str = "VERCEL_SANDBOX_TIMEOUT_MS";
/// Optional flag to enable named-sandbox persistence at creation time.
pub const VERCEL_SANDBOX_PERSISTENT_ENV: &str = "VERCEL_SANDBOX_PERSISTENT";
/// Service-specific Vercel access-token credential. Preferred on Vercel to avoid
/// colliding with the CLI's own `VERCEL_TOKEN` handling during builds.
pub const OPEN_AGENTS_VERCEL_TOKEN_ENV: &str = "OPEN_AGENTS_VERCEL_TOKEN";
/// Vercel access-token credential used when OIDC is unavailable.
pub const VERCEL_TOKEN_ENV: &str = "VERCEL_TOKEN";
/// Vercel OIDC token credential, available automatically on Vercel.
pub const VERCEL_OIDC_TOKEN_ENV: &str = "VERCEL_OIDC_TOKEN";
/// Vercel team identifier used by the Sandbox v2 API.
pub const VERCEL_TEAM_ID_ENV: &str = "VERCEL_TEAM_ID";
/// Vercel project identifier used by the Sandbox v2 API.
pub const VERCEL_PROJECT_ID_ENV: &str = "VERCEL_PROJECT_ID";

pub use git::{
    CommitOutcome, DiffFileStat, DiffSummary, FileChange, FileChangeStatus, GitCredentials,
    GitError, GitOutput, GitRedactor, GitRemoteActionMode, GitSandbox, GitStatus,
    PullRequestCommandOutcome, PushOptions, PushOutcome, is_safe_branch_name,
};
pub use git_finish::{
    GitFinishOptions, GitFinishReport, GitFinishStatus, PullRequestOptions, PullRequestOutcome,
    run_git_finish,
};
pub use just_bash::{JUST_BASH_DEFAULT_WORKING_DIRECTORY, JustBashSandbox, JustBashSandboxOptions};
pub use vercel::{
    VercelCommandData, VercelSandbox, VercelSandboxClient, VercelSandboxConfig,
    VercelSandboxCreateRequest, VercelSandboxCredentials, VercelSandboxMetadata,
    VercelSandboxRoute, VercelSandboxSession, VercelSandboxStatus, VercelSandboxUpstreamSource,
};

const DETACHED_QUICK_FAILURE_WINDOW_MS: u64 = 2_000;
const SHELL_BINARY: &str = "/bin/bash";

/// Result alias used by sandbox operations.
pub type SandboxResult<T> = std::result::Result<T, SandboxError>;

/// Sandbox context passed into agent calls.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxContext {
    /// Provider-specific resumable state payload.
    pub state: serde_json::Value,
    /// Working directory exposed to the agent.
    pub working_directory: String,
    /// Current git branch, when the sandbox is repo-backed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_branch: Option<String>,
    /// Provider/runtime details included in the system prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_details: Option<String>,
}

impl SandboxContext {
    /// Creates a sandbox context with provider state and working directory.
    pub fn new(state: serde_json::Value, working_directory: impl Into<String>) -> Self {
        Self {
            state,
            working_directory: working_directory.into(),
            current_branch: None,
            environment_details: None,
        }
    }

    /// Records the current branch.
    pub fn with_current_branch(mut self, current_branch: impl Into<String>) -> Self {
        self.current_branch = Some(current_branch.into());
        self
    }

    /// Records environment details for prompt construction.
    pub fn with_environment_details(mut self, environment_details: impl Into<String>) -> Self {
        self.environment_details = Some(environment_details.into());
        self
    }
}

/// Identifies a sandbox backend.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxType {
    /// In-process Just Bash virtual filesystem backend.
    JustBash,
    /// Local filesystem and process sandbox for tests and development.
    Local,
    /// Vercel Sandbox cloud backend.
    Vercel,
}

impl fmt::Display for SandboxType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::JustBash => formatter.write_str("just-bash"),
            Self::Local => formatter.write_str("local"),
            Self::Vercel => formatter.write_str("vercel"),
        }
    }
}

/// Serializable sandbox state used to reconnect durable runs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SandboxState {
    /// In-process Just Bash virtual filesystem state.
    #[serde(rename = "just-bash")]
    JustBash {
        /// Process-local virtual workspace id used to reconnect in-memory state.
        #[serde(rename = "workspaceId")]
        workspace_id: String,
        /// Current virtual working directory exposed to agents.
        #[serde(rename = "workingDirectory")]
        working_directory: String,
        /// Current git branch, when known.
        #[serde(
            default,
            rename = "currentBranch",
            skip_serializing_if = "Option::is_none"
        )]
        current_branch: Option<String>,
        /// Runtime expiration timestamp, in milliseconds since Unix epoch.
        #[serde(default, rename = "expiresAt", skip_serializing_if = "Option::is_none")]
        expires_at: Option<u64>,
    },
    /// Local sandbox state.
    #[serde(rename = "local")]
    Local {
        /// Absolute workspace root.
        root: String,
        /// Current working directory exposed to agents.
        #[serde(rename = "workingDirectory")]
        working_directory: String,
        /// Current git branch, when known.
        #[serde(
            default,
            rename = "currentBranch",
            skip_serializing_if = "Option::is_none"
        )]
        current_branch: Option<String>,
        /// Runtime expiration timestamp, in milliseconds since Unix epoch.
        #[serde(default, rename = "expiresAt", skip_serializing_if = "Option::is_none")]
        expires_at: Option<u64>,
    },
    /// Vercel cloud sandbox state.
    #[serde(rename = "vercel")]
    Vercel {
        /// Source repository to clone or resume.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<SandboxSource>,
        /// Durable persistent sandbox name.
        #[serde(
            default,
            rename = "sandboxName",
            skip_serializing_if = "Option::is_none"
        )]
        sandbox_name: Option<String>,
        /// Legacy runtime sandbox id.
        #[serde(default, rename = "sandboxId", skip_serializing_if = "Option::is_none")]
        sandbox_id: Option<String>,
        /// Snapshot id used for restore.
        #[serde(
            default,
            rename = "snapshotId",
            skip_serializing_if = "Option::is_none"
        )]
        snapshot_id: Option<String>,
        /// Runtime expiration timestamp, in milliseconds since Unix epoch.
        #[serde(default, rename = "expiresAt", skip_serializing_if = "Option::is_none")]
        expires_at: Option<u64>,
    },
}

impl SandboxState {
    /// Returns the backend discriminator for this state.
    pub const fn sandbox_type(&self) -> SandboxType {
        match self {
            Self::JustBash { .. } => SandboxType::JustBash,
            Self::Local { .. } => SandboxType::Local,
            Self::Vercel { .. } => SandboxType::Vercel,
        }
    }
}

/// Git source configuration for a sandbox workspace.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxSource {
    /// Repository URL.
    pub repo: String,
    /// Existing branch to checkout.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// New branch to create after clone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_branch: Option<String>,
}

impl SandboxSource {
    /// Creates a source from a repository URL.
    pub fn new(repo: impl Into<String>) -> Self {
        Self {
            repo: repo.into(),
            branch: None,
            new_branch: None,
        }
    }

    /// Sets the branch to checkout.
    pub fn with_branch(mut self, branch: impl Into<String>) -> Self {
        self.branch = Some(branch.into());
        self
    }

    /// Sets the new branch to create.
    pub fn with_new_branch(mut self, new_branch: impl Into<String>) -> Self {
        self.new_branch = Some(new_branch.into());
        self
    }
}

/// Options used when connecting to a sandbox state.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct SandboxConnectOptions {
    /// Environment variables exposed to commands.
    pub env: BTreeMap<String, String>,
    /// GitHub token used only while preparing trusted git operations.
    pub github_token: Option<String>,
    /// Git user used for commits in initialized workspaces.
    pub git_user: Option<SandboxGitUser>,
    /// Timeout to apply to the connected runtime.
    pub timeout_ms: Option<u64>,
    /// Ports to expose for preview URLs.
    pub ports: Vec<u16>,
    /// Skip git initialization/bootstrap for base-snapshot preparation.
    pub skip_git_workspace_bootstrap: bool,
}

impl fmt::Debug for SandboxConnectOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = formatter.debug_struct("SandboxConnectOptions");
        debug.field("env_keys", &self.env.keys().collect::<Vec<_>>());
        debug.field(
            "github_token",
            &self.github_token.as_ref().map(|_| "<redacted>"),
        );
        debug.field("git_user", &self.git_user);
        debug.field("timeout_ms", &self.timeout_ms);
        debug.field("ports", &self.ports);
        debug.field(
            "skip_git_workspace_bootstrap",
            &self.skip_git_workspace_bootstrap,
        );
        debug.finish()
    }
}

impl SandboxConnectOptions {
    /// Creates empty connect options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an environment variable for commands.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Sets the short-lived GitHub setup token.
    pub fn with_github_token(mut self, token: impl Into<String>) -> Self {
        self.github_token = Some(token.into());
        self
    }

    /// Sets the git user.
    pub fn with_git_user(mut self, git_user: SandboxGitUser) -> Self {
        self.git_user = Some(git_user);
        self
    }

    /// Sets the sandbox timeout in milliseconds.
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// Sets preview ports for the sandbox.
    pub fn with_ports(mut self, ports: impl IntoIterator<Item = u16>) -> Self {
        self.ports = ports.into_iter().collect();
        self
    }

    /// Skips git initialization/bootstrap for base snapshot preparation.
    pub fn with_skip_git_workspace_bootstrap(mut self, skip: bool) -> Self {
        self.skip_git_workspace_bootstrap = skip;
        self
    }
}

/// Git commit user configuration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxGitUser {
    /// Commit author name.
    pub name: String,
    /// Commit author email.
    pub email: String,
}

impl SandboxGitUser {
    /// Creates a git user.
    pub fn new(name: impl Into<String>, email: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            email: email.into(),
        }
    }
}

/// Full connect configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SandboxConnectConfig {
    /// Serializable state to connect.
    pub state: SandboxState,
    /// Runtime options that are not persisted in state.
    pub options: SandboxConnectOptions,
}

impl SandboxConnectConfig {
    /// Creates a connect configuration from state.
    pub fn new(state: SandboxState) -> Self {
        Self {
            state,
            options: SandboxConnectOptions::new(),
        }
    }

    /// Sets connect options.
    pub fn with_options(mut self, options: SandboxConnectOptions) -> Self {
        self.options = options;
        self
    }
}

/// Connects to a sandbox from serialized state.
pub fn connect_sandbox(config: SandboxConnectConfig) -> SandboxResult<Box<dyn Sandbox>> {
    match config.state {
        SandboxState::JustBash {
            workspace_id,
            working_directory,
            current_branch,
            expires_at,
        } => {
            let options = JustBashSandboxOptions {
                env: config.options.env,
                current_branch,
                timeout_ms: config.options.timeout_ms,
                expires_at_ms: expires_at,
            };
            Ok(Box::new(JustBashSandbox::with_options(
                workspace_id,
                working_directory,
                options,
            )?))
        }
        SandboxState::Local {
            root,
            current_branch,
            expires_at,
            ..
        } => {
            let options = LocalSandboxOptions {
                env: config.options.env,
                current_branch,
                timeout_ms: config.options.timeout_ms,
                expires_at_ms: expires_at,
            };
            Ok(Box::new(LocalSandbox::with_options(root, options)?))
        }
        SandboxState::Vercel { .. } => Ok(Box::new(VercelSandbox::connect(config)?)),
    }
}

/// File type returned by sandbox directory listing.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxDirEntryKind {
    /// Directory entry.
    Directory,
    /// Regular file entry.
    File,
    /// Symbolic link entry.
    Symlink,
    /// Another filesystem entry type.
    Other,
}

/// Directory entry returned by [`Sandbox::read_dir`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxDirEntry {
    /// Entry basename.
    pub name: String,
    /// Entry kind.
    pub kind: SandboxDirEntryKind,
}

impl SandboxDirEntry {
    /// Returns true when this entry is a directory.
    pub const fn is_directory(&self) -> bool {
        matches!(self.kind, SandboxDirEntryKind::Directory)
    }

    /// Returns true when this entry is a regular file.
    pub const fn is_file(&self) -> bool {
        matches!(self.kind, SandboxDirEntryKind::File)
    }

    /// Returns true when this entry is a symbolic link.
    pub const fn is_symlink(&self) -> bool {
        matches!(self.kind, SandboxDirEntryKind::Symlink)
    }
}

/// File stats returned by [`Sandbox::stat`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxStats {
    /// True when the path is a directory.
    pub is_directory: bool,
    /// True when the path is a regular file.
    pub is_file: bool,
    /// File size in bytes.
    pub size: u64,
    /// Modification time in milliseconds since Unix epoch.
    pub mtime_ms: u64,
}

/// Options passed to [`Sandbox::mkdir`].
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxMkdirOptions {
    /// Create missing parent directories.
    pub recursive: bool,
}

impl SandboxMkdirOptions {
    /// Creates non-recursive mkdir options.
    pub const fn new() -> Self {
        Self { recursive: false }
    }

    /// Creates recursive mkdir options.
    pub const fn recursive() -> Self {
        Self { recursive: true }
    }
}

/// Shell command execution options.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxExecOptions {
    /// Shell command to execute.
    pub command: String,
    /// Working directory for the command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Timeout in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
}

impl SandboxExecOptions {
    /// Creates command execution options.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            cwd: None,
            timeout_ms: None,
        }
    }

    /// Sets the command working directory.
    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Sets the command timeout.
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }
}

/// Shell command execution result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxExecResult {
    /// True when the command exited with status code zero.
    pub success: bool,
    /// Command exit code, or `None` when timed out before a code was available.
    pub exit_code: Option<i32>,
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
    /// True when stdout or stderr was truncated.
    pub truncated: bool,
}

/// Operation families the sandbox crate exposes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxOperation {
    /// Read a UTF-8 file.
    ReadFile,
    /// Read raw file bytes.
    ReadFileBuffer,
    /// Write a UTF-8 file.
    WriteFile,
    /// Return file metadata.
    Stat,
    /// Check path accessibility.
    Access,
    /// Create directories.
    Mkdir,
    /// Read directory entries.
    Readdir,
    /// Execute a command and wait for output.
    Exec,
    /// Execute a detached command.
    ExecDetached,
    /// Resolve a public URL for a port.
    Domain,
    /// Stop the sandbox.
    Stop,
    /// Extend the sandbox timeout.
    ExtendTimeout,
    /// Snapshot the sandbox filesystem.
    Snapshot,
}

/// Detached command execution options.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxDetachedOptions {
    /// Shell command to execute in detached mode.
    pub command: String,
    /// Working directory for the command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

impl SandboxDetachedOptions {
    /// Creates detached command options.
    pub fn new(command: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            cwd: None,
        }
    }

    /// Sets the detached command working directory.
    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }
}

/// Detached command handle returned after startup.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxDetachedCommand {
    /// Backend command id.
    pub command_id: String,
}

/// Snapshot result returned by sandbox backends that support snapshots.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotResult {
    /// Backend snapshot id.
    pub snapshot_id: String,
}

/// Timeout extension result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxTimeoutExtension {
    /// New expiration timestamp in milliseconds since Unix epoch.
    pub expires_at: u64,
}

/// Object-safe sandbox boundary.
pub trait Sandbox: fmt::Debug + Send + Sync {
    /// Returns the sandbox backend type.
    fn sandbox_type(&self) -> SandboxType;

    /// Returns the working directory exposed to agents.
    fn working_directory(&self) -> &str;

    /// Returns the environment keys and values available to commands.
    fn env(&self) -> &BTreeMap<String, String>;

    /// Returns the current git branch, when known.
    fn current_branch(&self) -> Option<&str>;

    /// Returns environment details suitable for agent instructions.
    fn environment_details(&self) -> Option<&str>;

    /// Returns the base host for preview URLs, when available.
    fn host(&self) -> Option<&str>;

    /// Returns the expiration timestamp in milliseconds since Unix epoch.
    fn expires_at_ms(&self) -> Option<u64>;

    /// Returns the configured timeout in milliseconds.
    fn timeout_ms(&self) -> Option<u64>;

    /// Reads a UTF-8 file.
    fn read_file(&self, path: &str) -> SandboxResult<String>;

    /// Reads a file as bytes.
    fn read_file_buffer(&self, path: &str) -> SandboxResult<Vec<u8>>;

    /// Writes a UTF-8 file, creating parent directories when needed.
    fn write_file(&self, path: &str, content: &str) -> SandboxResult<()>;

    /// Stats a path.
    fn stat(&self, path: &str) -> SandboxResult<SandboxStats>;

    /// Verifies that a path exists.
    fn access(&self, path: &str) -> SandboxResult<()>;

    /// Creates a directory.
    fn mkdir(&self, path: &str, options: SandboxMkdirOptions) -> SandboxResult<()>;

    /// Reads a directory.
    fn read_dir(&self, path: &str) -> SandboxResult<Vec<SandboxDirEntry>>;

    /// Executes a shell command and waits for completion.
    fn exec(&self, options: SandboxExecOptions) -> SandboxResult<SandboxExecResult>;

    /// Executes a shell command in detached mode.
    fn exec_detached(
        &self,
        options: SandboxDetachedOptions,
    ) -> SandboxResult<SandboxDetachedCommand>;

    /// Temporarily configures GitHub setup credentials.
    fn set_github_auth_token(&self, token: Option<&str>) -> SandboxResult<()>;

    /// Returns a public URL for a port, when supported.
    fn domain(&self, port: u16) -> Option<String>;

    /// Stops the sandbox.
    fn stop(&self) -> SandboxResult<()>;

    /// Extends the sandbox timeout.
    fn extend_timeout(&self, additional_ms: u64) -> SandboxResult<SandboxTimeoutExtension>;

    /// Creates a filesystem snapshot.
    fn snapshot(&self) -> SandboxResult<SnapshotResult>;

    /// Returns serializable state for durable reconnect.
    fn state(&self) -> SandboxState;
}

/// Options for [`LocalSandbox`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LocalSandboxOptions {
    /// Environment variables exposed to commands.
    pub env: BTreeMap<String, String>,
    /// Current git branch, when known.
    pub current_branch: Option<String>,
    /// Configured timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// Current expiration timestamp in milliseconds since Unix epoch.
    pub expires_at_ms: Option<u64>,
}

impl LocalSandboxOptions {
    /// Creates empty local sandbox options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an environment variable for commands.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Sets the known current branch.
    pub fn with_current_branch(mut self, current_branch: impl Into<String>) -> Self {
        self.current_branch = Some(current_branch.into());
        self
    }

    /// Sets the local timeout in milliseconds.
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self.expires_at_ms = Some(now_ms().saturating_add(timeout_ms));
        self
    }
}

/// Local filesystem and process sandbox.
pub struct LocalSandbox {
    root: PathBuf,
    working_directory: String,
    env: BTreeMap<String, String>,
    current_branch: Option<String>,
    timeout_ms: Option<u64>,
    expires_at_ms: Mutex<Option<u64>>,
    stopped: Mutex<bool>,
}

impl fmt::Debug for LocalSandbox {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalSandbox")
            .field("root", &self.root)
            .field("working_directory", &self.working_directory)
            .field("env_keys", &self.env.keys().collect::<Vec<_>>())
            .field("current_branch", &self.current_branch)
            .field("timeout_ms", &self.timeout_ms)
            .field("expires_at_ms", &self.expires_at_ms())
            .finish_non_exhaustive()
    }
}

impl LocalSandbox {
    /// Creates a local sandbox rooted at an existing workspace directory.
    pub fn new(root: impl AsRef<Path>) -> SandboxResult<Self> {
        Self::with_options(root, LocalSandboxOptions::new())
    }

    /// Creates a local sandbox rooted at an existing workspace directory.
    pub fn with_options(
        root: impl AsRef<Path>,
        mut options: LocalSandboxOptions,
    ) -> SandboxResult<Self> {
        let root = root.as_ref();
        fs::create_dir_all(root).map_err(|error| SandboxError::io("mkdir", root, error))?;
        let root = fs::canonicalize(root)
            .map_err(|error| SandboxError::io("canonicalize", root, error))?;
        if options.expires_at_ms.is_none() {
            options.expires_at_ms = options
                .timeout_ms
                .map(|timeout_ms| now_ms().saturating_add(timeout_ms));
        }
        let working_directory = root.to_string_lossy().into_owned();

        Ok(Self {
            root,
            working_directory,
            env: options.env,
            current_branch: options.current_branch,
            timeout_ms: options.timeout_ms,
            expires_at_ms: Mutex::new(options.expires_at_ms),
            stopped: Mutex::new(false),
        })
    }

    fn ensure_running(&self) -> SandboxResult<()> {
        let stopped = self
            .stopped
            .lock()
            .map_err(|_| SandboxError::internal("local sandbox stop state lock poisoned"))?;
        if *stopped {
            return Err(SandboxError::Stopped {
                sandbox_type: SandboxType::Local,
            });
        }
        Ok(())
    }

    fn resolve_existing_path(&self, path: &str, operation: &str) -> SandboxResult<PathBuf> {
        let resolved = self.resolve_path(path, operation)?;
        let canonical = fs::canonicalize(&resolved)
            .map_err(|error| SandboxError::io(operation, &resolved, error))?;
        if !canonical.starts_with(&self.root) {
            return Err(SandboxError::PathOutsideWorkspace {
                path: path.to_string(),
                workspace: self.working_directory.clone(),
            });
        }
        Ok(canonical)
    }

    fn resolve_path(&self, path: &str, operation: &str) -> SandboxResult<PathBuf> {
        let raw_path = Path::new(path);
        let candidate = if raw_path.is_absolute() {
            raw_path.to_path_buf()
        } else {
            self.root.join(raw_path)
        };
        let normalized = normalize_path(&candidate);
        if !normalized.starts_with(&self.root) {
            return Err(SandboxError::PathOutsideWorkspace {
                path: path.to_string(),
                workspace: self.working_directory.clone(),
            });
        }

        let nearest = nearest_existing_ancestor(&normalized)
            .map_err(|error| SandboxError::io(operation, &normalized, error))?;
        let canonical_nearest = fs::canonicalize(nearest)
            .map_err(|error| SandboxError::io(operation, nearest, error))?;
        if !canonical_nearest.starts_with(&self.root) {
            return Err(SandboxError::PathOutsideWorkspace {
                path: path.to_string(),
                workspace: self.working_directory.clone(),
            });
        }

        Ok(normalized)
    }

    fn resolve_cwd(&self, cwd: Option<&str>) -> SandboxResult<PathBuf> {
        let cwd = cwd.unwrap_or(self.working_directory());
        let resolved = self.resolve_existing_path(cwd, "cwd")?;
        let stats =
            fs::metadata(&resolved).map_err(|error| SandboxError::io("cwd", &resolved, error))?;
        if !stats.is_dir() {
            return Err(SandboxError::NotDirectory {
                path: cwd.to_string(),
            });
        }
        Ok(resolved)
    }
}

impl Sandbox for LocalSandbox {
    fn sandbox_type(&self) -> SandboxType {
        SandboxType::Local
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
        Some(
            "- Local sandbox commands run on this machine inside the configured workspace directory\n- Use workspace-relative paths for file operations\n- Preview URLs use 127.0.0.1 with the requested port",
        )
    }

    fn host(&self) -> Option<&str> {
        Some("127.0.0.1")
    }

    fn expires_at_ms(&self) -> Option<u64> {
        self.expires_at_ms
            .lock()
            .ok()
            .and_then(|expires_at_ms| *expires_at_ms)
    }

    fn timeout_ms(&self) -> Option<u64> {
        self.timeout_ms
    }

    fn read_file(&self, path: &str) -> SandboxResult<String> {
        let buffer = self.read_file_buffer(path)?;
        String::from_utf8(buffer).map_err(|error| SandboxError::InvalidUtf8 {
            path: path.to_string(),
            message: error.to_string(),
        })
    }

    fn read_file_buffer(&self, path: &str) -> SandboxResult<Vec<u8>> {
        self.ensure_running()?;
        let resolved = self.resolve_existing_path(path, "read")?;
        fs::read(&resolved).map_err(|error| SandboxError::io("read", resolved, error))
    }

    fn write_file(&self, path: &str, content: &str) -> SandboxResult<()> {
        self.ensure_running()?;
        let resolved = self.resolve_path(path, "write")?;
        if let Some(parent) = resolved.parent() {
            fs::create_dir_all(parent).map_err(|error| SandboxError::io("mkdir", parent, error))?;
        }
        fs::write(&resolved, content).map_err(|error| SandboxError::io("write", resolved, error))
    }

    fn stat(&self, path: &str) -> SandboxResult<SandboxStats> {
        self.ensure_running()?;
        let resolved = self.resolve_existing_path(path, "stat")?;
        let metadata =
            fs::metadata(&resolved).map_err(|error| SandboxError::io("stat", &resolved, error))?;
        let mtime_ms = metadata
            .modified()
            .ok()
            .and_then(system_time_ms)
            .unwrap_or_default();

        Ok(SandboxStats {
            is_directory: metadata.is_dir(),
            is_file: metadata.is_file(),
            size: metadata.len(),
            mtime_ms,
        })
    }

    fn access(&self, path: &str) -> SandboxResult<()> {
        self.ensure_running()?;
        self.resolve_existing_path(path, "access").map(|_| ())
    }

    fn mkdir(&self, path: &str, options: SandboxMkdirOptions) -> SandboxResult<()> {
        self.ensure_running()?;
        let resolved = self.resolve_path(path, "mkdir")?;
        let result = if options.recursive {
            fs::create_dir_all(&resolved)
        } else {
            fs::create_dir(&resolved)
        };
        result.map_err(|error| SandboxError::io("mkdir", resolved, error))
    }

    fn read_dir(&self, path: &str) -> SandboxResult<Vec<SandboxDirEntry>> {
        self.ensure_running()?;
        let resolved = self.resolve_existing_path(path, "read_dir")?;
        let mut entries = Vec::new();
        for entry in fs::read_dir(&resolved)
            .map_err(|error| SandboxError::io("read_dir", &resolved, error))?
        {
            let entry = entry.map_err(|error| SandboxError::io("read_dir", &resolved, error))?;
            let file_type = entry
                .file_type()
                .map_err(|error| SandboxError::io("read_dir", entry.path(), error))?;
            let kind = if file_type.is_dir() {
                SandboxDirEntryKind::Directory
            } else if file_type.is_file() {
                SandboxDirEntryKind::File
            } else if file_type.is_symlink() {
                SandboxDirEntryKind::Symlink
            } else {
                SandboxDirEntryKind::Other
            };
            entries.push(SandboxDirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                kind,
            });
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    fn exec(&self, options: SandboxExecOptions) -> SandboxResult<SandboxExecResult> {
        self.ensure_running()?;
        let cwd = self.resolve_cwd(options.cwd.as_deref())?;
        run_shell_command(&options.command, &cwd, &self.env, options.timeout_ms)
    }

    fn exec_detached(
        &self,
        options: SandboxDetachedOptions,
    ) -> SandboxResult<SandboxDetachedCommand> {
        self.ensure_running()?;
        let cwd = self.resolve_cwd(options.cwd.as_deref())?;
        run_detached_shell_command(&options.command, &cwd, &self.env)
    }

    fn set_github_auth_token(&self, _token: Option<&str>) -> SandboxResult<()> {
        self.ensure_running()
    }

    fn domain(&self, port: u16) -> Option<String> {
        Some(format!("http://127.0.0.1:{port}"))
    }

    fn stop(&self) -> SandboxResult<()> {
        let mut stopped = self
            .stopped
            .lock()
            .map_err(|_| SandboxError::internal("local sandbox stop state lock poisoned"))?;
        *stopped = true;
        if let Ok(mut expires_at_ms) = self.expires_at_ms.lock() {
            *expires_at_ms = None;
        }
        Ok(())
    }

    fn extend_timeout(&self, additional_ms: u64) -> SandboxResult<SandboxTimeoutExtension> {
        self.ensure_running()?;
        let mut expires_at_ms = self
            .expires_at_ms
            .lock()
            .map_err(|_| SandboxError::internal("local sandbox expiration lock poisoned"))?;
        let next_expires_at = expires_at_ms
            .unwrap_or_else(now_ms)
            .saturating_add(additional_ms);
        *expires_at_ms = Some(next_expires_at);
        Ok(SandboxTimeoutExtension {
            expires_at: next_expires_at,
        })
    }

    fn snapshot(&self) -> SandboxResult<SnapshotResult> {
        Err(SandboxError::UnsupportedOperation {
            operation: "snapshot".to_string(),
            sandbox_type: SandboxType::Local,
        })
    }

    fn state(&self) -> SandboxState {
        SandboxState::Local {
            root: self.working_directory.clone(),
            working_directory: self.working_directory.clone(),
            current_branch: self.current_branch.clone(),
            expires_at: self.expires_at_ms(),
        }
    }
}

/// Error returned by sandbox operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SandboxError {
    /// A filesystem path escaped the workspace root.
    PathOutsideWorkspace {
        /// Rejected path.
        path: String,
        /// Workspace root.
        workspace: String,
    },
    /// A path was expected to be a directory.
    NotDirectory {
        /// Rejected path.
        path: String,
    },
    /// A filesystem operation failed.
    Io {
        /// Operation name.
        operation: String,
        /// Path involved in the failure.
        path: String,
        /// Error message.
        message: String,
    },
    /// A file was not valid UTF-8.
    InvalidUtf8 {
        /// File path.
        path: String,
        /// Error message.
        message: String,
    },
    /// A command could not be started.
    CommandSpawn {
        /// Command string.
        command: String,
        /// Error message.
        message: String,
    },
    /// A detached command exited during the quick-failure window.
    DetachedCommandFailed {
        /// Command string.
        command: String,
        /// Process exit code.
        exit_code: Option<i32>,
        /// Standard error snippet.
        stderr: String,
    },
    /// The backend does not support this operation.
    UnsupportedOperation {
        /// Operation name.
        operation: String,
        /// Backend type.
        sandbox_type: SandboxType,
    },
    /// The sandbox has already stopped.
    Stopped {
        /// Backend type.
        sandbox_type: SandboxType,
    },
    /// Required configuration was absent.
    MissingConfig {
        /// Missing environment variable or field.
        name: &'static str,
    },
    /// The Vercel Sandbox API returned an error or malformed response.
    Api {
        /// Operation name.
        operation: String,
        /// HTTP status, when a response was available.
        status: Option<u16>,
        /// Error details.
        message: String,
    },
    /// An internal invariant failed.
    Internal {
        /// Error message.
        message: String,
    },
}

impl SandboxError {
    fn io(operation: impl Into<String>, path: impl AsRef<Path>, error: io::Error) -> Self {
        Self::Io {
            operation: operation.into(),
            path: path.as_ref().to_string_lossy().into_owned(),
            message: error.to_string(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self::Internal {
            message: message.into(),
        }
    }
}

impl fmt::Display for SandboxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PathOutsideWorkspace { path, workspace } => {
                write!(
                    formatter,
                    "path '{path}' is outside workspace '{workspace}'"
                )
            }
            Self::NotDirectory { path } => write!(formatter, "path '{path}' is not a directory"),
            Self::Io {
                operation,
                path,
                message,
            } => write!(formatter, "{operation} failed for '{path}': {message}"),
            Self::InvalidUtf8 { path, message } => {
                write!(formatter, "file '{path}' is not valid UTF-8: {message}")
            }
            Self::CommandSpawn { command, message } => {
                write!(formatter, "failed to start command '{command}': {message}")
            }
            Self::DetachedCommandFailed {
                command,
                exit_code,
                stderr,
            } => write!(
                formatter,
                "detached command '{command}' exited early with code {exit_code:?}: {stderr}"
            ),
            Self::UnsupportedOperation {
                operation,
                sandbox_type,
            } => write!(
                formatter,
                "sandbox backend '{sandbox_type}' does not support operation '{operation}'"
            ),
            Self::Stopped { sandbox_type } => {
                write!(formatter, "sandbox backend '{sandbox_type}' has stopped")
            }
            Self::MissingConfig { name } => {
                write!(formatter, "missing required sandbox configuration {name}")
            }
            Self::Api {
                operation,
                status,
                message,
            } => match status {
                Some(status) => write!(
                    formatter,
                    "Vercel Sandbox API {operation} failed with status {status}: {message}"
                ),
                None => write!(
                    formatter,
                    "Vercel Sandbox API {operation} failed: {message}"
                ),
            },
            Self::Internal { message } => formatter.write_str(message),
        }
    }
}

impl Error for SandboxError {}

fn run_shell_command(
    command: &str,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    timeout_ms: Option<u64>,
) -> SandboxResult<SandboxExecResult> {
    let mut child = Command::new(SHELL_BINARY)
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .envs(env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| SandboxError::CommandSpawn {
            command: command.to_string(),
            message: error.to_string(),
        })?;
    let stdout = child.stdout.take().ok_or_else(|| SandboxError::Internal {
        message: "command stdout pipe was not captured".to_string(),
    })?;
    let stderr = child.stderr.take().ok_or_else(|| SandboxError::Internal {
        message: "command stderr pipe was not captured".to_string(),
    })?;
    let stdout_reader = thread::spawn(move || read_capped_output(stdout));
    let stderr_reader = thread::spawn(move || read_capped_output(stderr));

    let timeout = Duration::from_millis(timeout_ms.unwrap_or(DEFAULT_EXEC_TIMEOUT_MS));
    let start = Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| SandboxError::CommandSpawn {
                command: command.to_string(),
                message: error.to_string(),
            })?
        {
            let stdout = join_capped_output(stdout_reader)?;
            let stderr = join_capped_output(stderr_reader)?;
            return Ok(SandboxExecResult {
                success: status.success(),
                exit_code: status.code(),
                stdout: String::from_utf8_lossy(&stdout.bytes).into_owned(),
                stderr: String::from_utf8_lossy(&stderr.bytes).into_owned(),
                truncated: stdout.truncated || stderr.truncated,
            });
        }

        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = join_capped_output(stdout_reader);
            let _ = join_capped_output(stderr_reader);
            return Ok(SandboxExecResult {
                success: false,
                exit_code: None,
                stdout: String::new(),
                stderr: format!("Command timed out after {}ms", timeout.as_millis()),
                truncated: false,
            });
        }

        thread::sleep(Duration::from_millis(10));
    }
}

struct CappedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn read_capped_output(mut reader: impl Read) -> io::Result<CappedOutput> {
    let mut bytes = Vec::new();
    let mut truncated = false;
    let mut chunk = [0_u8; 8192];

    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }

        let remaining = DEFAULT_MAX_OUTPUT_LENGTH.saturating_sub(bytes.len());
        if remaining > 0 {
            let retained = read.min(remaining);
            bytes.extend_from_slice(&chunk[..retained]);
            truncated |= retained < read;
        } else {
            truncated = true;
        }
    }

    Ok(CappedOutput { bytes, truncated })
}

fn join_capped_output(
    reader: thread::JoinHandle<io::Result<CappedOutput>>,
) -> SandboxResult<CappedOutput> {
    reader
        .join()
        .map_err(|_| SandboxError::internal("command output reader panicked"))?
        .map_err(|error| SandboxError::Internal {
            message: format!("failed to read command output: {error}"),
        })
}

fn run_detached_shell_command(
    command: &str,
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> SandboxResult<SandboxDetachedCommand> {
    let log_id = detached_log_id();
    let stdout_path = std::env::temp_dir().join(format!("{log_id}.stdout"));
    let stderr_path = std::env::temp_dir().join(format!("{log_id}.stderr"));
    let stdout = File::create(&stdout_path)
        .map_err(|error| SandboxError::io("create", &stdout_path, error))?;
    let stderr = File::create(&stderr_path)
        .map_err(|error| SandboxError::io("create", &stderr_path, error))?;

    let mut child = Command::new(SHELL_BINARY)
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .envs(env)
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| SandboxError::CommandSpawn {
            command: command.to_string(),
            message: error.to_string(),
        })?;

    let command_id = format!("local:{}", child.id());
    let start = Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| SandboxError::CommandSpawn {
                command: command.to_string(),
                message: error.to_string(),
            })?
        {
            if !status.success() {
                let stderr = fs::read_to_string(&stderr_path).unwrap_or_default();
                let stderr = truncate_text(stderr);
                let _ = fs::remove_file(&stdout_path);
                let _ = fs::remove_file(&stderr_path);
                return Err(SandboxError::DetachedCommandFailed {
                    command: command.to_string(),
                    exit_code: status.code(),
                    stderr,
                });
            }
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            return Ok(SandboxDetachedCommand { command_id });
        }

        if start.elapsed() >= Duration::from_millis(DETACHED_QUICK_FAILURE_WINDOW_MS) {
            let _ = fs::remove_file(&stdout_path);
            let _ = fs::remove_file(&stderr_path);
            return Ok(SandboxDetachedCommand { command_id });
        }

        thread::sleep(Duration::from_millis(10));
    }
}

fn truncate_text(output: String) -> String {
    if output.chars().count() <= DEFAULT_MAX_OUTPUT_LENGTH {
        return output;
    }

    output.chars().take(DEFAULT_MAX_OUTPUT_LENGTH).collect()
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn nearest_existing_ancestor(path: &Path) -> io::Result<&Path> {
    let mut current = path;
    loop {
        if current.exists() {
            return Ok(current);
        }
        current = current
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no existing ancestor"))?;
    }
}

fn now_ms() -> u64 {
    system_time_ms(SystemTime::now()).unwrap_or_default()
}

fn system_time_ms(time: SystemTime) -> Option<u64> {
    let duration = time.duration_since(UNIX_EPOCH).ok()?;
    u64::try_from(duration.as_millis()).ok()
}

fn detached_log_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("open-agents-sandbox-{}-{nanos}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_WORKSPACE_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempWorkspace {
        path: PathBuf,
    }

    impl TempWorkspace {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "open-agents-sandbox-test-{}-{}",
                std::process::id(),
                TEMP_WORKSPACE_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("temp workspace is created");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn local_sandbox_reads_writes_stats_and_lists_temp_workspace_files() {
        let temp = TempWorkspace::new();
        let sandbox = LocalSandbox::new(temp.path()).expect("local sandbox");

        sandbox
            .write_file("src/main.rs", "fn main() {}\n")
            .expect("write file");

        assert_eq!(
            sandbox.read_file("src/main.rs").expect("read file"),
            "fn main() {}\n"
        );
        let stats = sandbox.stat("src/main.rs").expect("stat file");
        assert!(stats.is_file);
        assert!(!stats.is_directory);
        assert_eq!(stats.size, 13);
        sandbox.access("src/main.rs").expect("access file");

        let entries = sandbox.read_dir("src").expect("read dir");
        assert_eq!(
            entries,
            vec![SandboxDirEntry {
                name: "main.rs".to_string(),
                kind: SandboxDirEntryKind::File,
            }]
        );
    }

    #[test]
    fn local_sandbox_creates_recursive_directories_in_temp_workspace() {
        let temp = TempWorkspace::new();
        let sandbox = LocalSandbox::new(temp.path()).expect("local sandbox");

        sandbox
            .mkdir("nested/deep", SandboxMkdirOptions::recursive())
            .expect("mkdir");
        let stats = sandbox.stat("nested/deep").expect("stat directory");

        assert!(stats.is_directory);
        assert!(!stats.is_file);
    }

    #[test]
    fn local_sandbox_rejects_path_escape_attempts() {
        let temp = TempWorkspace::new();
        let sandbox = LocalSandbox::new(temp.path()).expect("local sandbox");

        assert!(matches!(
            sandbox.read_file("../outside.txt"),
            Err(SandboxError::PathOutsideWorkspace { .. })
        ));
        assert!(matches!(
            sandbox.write_file("/tmp/open-agents-sandbox-outside.txt", "nope"),
            Err(SandboxError::PathOutsideWorkspace { .. })
        ));
    }

    #[test]
    fn local_sandbox_executes_commands_in_temp_workspace() {
        let temp = TempWorkspace::new();
        let sandbox = LocalSandbox::with_options(
            temp.path(),
            LocalSandboxOptions::new().with_env("OPEN_AGENTS_TEST", "present"),
        )
        .expect("local sandbox");
        sandbox
            .mkdir("scripts", SandboxMkdirOptions::recursive())
            .expect("mkdir");

        let result = sandbox
            .exec(
                SandboxExecOptions::new("printf \"%s:%s\" \"$PWD\" \"$OPEN_AGENTS_TEST\"")
                    .with_cwd("scripts")
                    .with_timeout_ms(1_000),
            )
            .expect("exec");

        assert!(result.success);
        assert_eq!(result.exit_code, Some(0));
        assert!(result.stdout.ends_with("scripts:present"));
        assert_eq!(result.stderr, "");
        assert!(!result.truncated);
    }

    #[test]
    fn local_sandbox_truncates_command_output() {
        let temp = TempWorkspace::new();
        let sandbox = LocalSandbox::new(temp.path()).expect("local sandbox");

        let result = sandbox
            .exec(SandboxExecOptions::new("perl -e 'print \"x\" x 120000'"))
            .expect("exec");

        assert!(result.success);
        assert!(result.truncated);
        assert_eq!(result.stdout.len(), DEFAULT_MAX_OUTPUT_LENGTH);
    }

    #[test]
    fn local_sandbox_reports_command_timeout() {
        let temp = TempWorkspace::new();
        let sandbox = LocalSandbox::new(temp.path()).expect("local sandbox");

        let result = sandbox
            .exec(SandboxExecOptions::new("sleep 2").with_timeout_ms(50))
            .expect("exec");

        assert!(!result.success);
        assert_eq!(result.exit_code, None);
        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "Command timed out after 50ms");
        assert!(!result.truncated);
    }

    #[test]
    fn local_sandbox_starts_detached_processes() {
        let temp = TempWorkspace::new();
        let sandbox = LocalSandbox::new(temp.path()).expect("local sandbox");

        let detached = sandbox
            .exec_detached(SandboxDetachedOptions::new(
                "printf ready > ready.txt; sleep 3",
            ))
            .expect("detached exec");

        assert!(detached.command_id.starts_with("local:"));
        assert_eq!(
            sandbox.read_file("ready.txt").expect("detached wrote file"),
            "ready"
        );
    }

    #[test]
    fn local_sandbox_rejects_detached_quick_failures() {
        let temp = TempWorkspace::new();
        let sandbox = LocalSandbox::new(temp.path()).expect("local sandbox");

        let error = sandbox
            .exec_detached(SandboxDetachedOptions::new("printf failure >&2; exit 23"))
            .expect_err("quick failure");

        assert!(matches!(
            error,
            SandboxError::DetachedCommandFailed {
                exit_code: Some(23),
                ..
            }
        ));
    }

    #[test]
    fn sandbox_state_serializes_and_reconnects_local_workspace() {
        let temp = TempWorkspace::new();
        let sandbox = LocalSandbox::with_options(
            temp.path(),
            LocalSandboxOptions::new()
                .with_current_branch("codex/test")
                .with_timeout_ms(1_000),
        )
        .expect("local sandbox");
        sandbox.write_file("note.txt", "saved").expect("write");

        let state = sandbox.state();
        let serialized = serde_json::to_value(&state).expect("serialize");
        assert_eq!(serialized["type"], "local");
        assert_eq!(serialized["workingDirectory"], sandbox.working_directory());
        assert_eq!(serialized["currentBranch"], "codex/test");
        assert!(serialized["expiresAt"].is_u64());

        let reconnected = connect_sandbox(SandboxConnectConfig::new(
            serde_json::from_value(serialized).expect("deserialize"),
        ))
        .expect("connect local");

        assert_eq!(reconnected.current_branch(), Some("codex/test"));
        assert_eq!(reconnected.read_file("note.txt").expect("read"), "saved");
    }

    #[test]
    fn sandbox_vercel_state_serializes_upstream_factory_shape() {
        let state = SandboxState::Vercel {
            source: Some(
                SandboxSource::new("https://github.com/vercel-labs/open-agents")
                    .with_branch("main")
                    .with_new_branch("codex/demo"),
            ),
            sandbox_name: Some("session_123".to_string()),
            sandbox_id: Some("legacy_123".to_string()),
            snapshot_id: Some("snap_123".to_string()),
            expires_at: Some(1_800_000),
        };

        let serialized = serde_json::to_value(state).expect("serialize");

        assert_eq!(serialized["type"], "vercel");
        assert_eq!(serialized["sandboxName"], "session_123");
        assert_eq!(serialized["sandboxId"], "legacy_123");
        assert_eq!(serialized["snapshotId"], "snap_123");
        assert_eq!(serialized["expiresAt"], 1_800_000);
        assert_eq!(
            serialized["source"],
            serde_json::json!({
                "repo": "https://github.com/vercel-labs/open-agents",
                "branch": "main",
                "newBranch": "codex/demo"
            })
        );
    }

    #[test]
    fn sandbox_context_round_trips_with_optional_fields() {
        let context = SandboxContext::new(serde_json::json!({"type": "vercel"}), "/workspace")
            .with_current_branch("main")
            .with_environment_details("Vercel Sandbox");

        let encoded = serde_json::to_string(&context).expect("serialize context");
        let decoded: SandboxContext = serde_json::from_str(&encoded).expect("deserialize context");

        assert_eq!(decoded.current_branch.as_deref(), Some("main"));
        assert_eq!(
            decoded.environment_details.as_deref(),
            Some("Vercel Sandbox")
        );
    }

    #[test]
    fn operation_serializes_as_snake_case() {
        let encoded =
            serde_json::to_string(&SandboxOperation::ExecDetached).expect("serialize operation");

        assert_eq!(encoded, "\"exec_detached\"");
    }

    #[test]
    fn sandbox_trait_object_operates_against_local_backend() {
        let temp = TempWorkspace::new();
        let sandbox: Box<dyn Sandbox> =
            Box::new(LocalSandbox::new(temp.path()).expect("local sandbox"));

        sandbox
            .write_file("object.txt", "trait")
            .expect("write through trait");
        let result = sandbox
            .exec(SandboxExecOptions::new("cat object.txt"))
            .expect("exec through trait");

        assert_eq!(sandbox.sandbox_type(), SandboxType::Local);
        assert_eq!(result.stdout, "trait");
    }

    #[test]
    fn connect_options_debug_redacts_credentials_and_env_values() {
        let options = SandboxConnectOptions::new()
            .with_env("SECRET_ENV", "hidden")
            .with_github_token("ghp_secret");

        let debug = format!("{options:?}");

        assert!(debug.contains("SECRET_ENV"));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("hidden"));
        assert!(!debug.contains("ghp_secret"));
    }
}
