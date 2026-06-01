//! Node-API bindings for the Rust `just-bash` backend.
//!
//! The handwritten adapter code does not use `unsafe`, but napi-rs generates
//! Node-API FFI glue from the `#[napi]` macros. This crate scopes the
//! `unsafe_code` lint exception to that generated boundary.

use std::collections::{BTreeMap, HashMap};

use just_bash::{
    Bash, BashOptions, JustBashError, JustBashExecOptions, JustBashExecResult, resolve_path,
};
use napi::{Error, Result, Status};
use napi_derive::napi;

/// Constructor options compatible with upstream Just Bash's core test surface.
#[derive(Default)]
#[napi(object)]
pub struct RustBashOptions {
    /// Initial virtual files keyed by path.
    pub files: Option<HashMap<String, String>>,
    /// Base environment available to each execution.
    pub env: Option<HashMap<String, String>>,
    /// Base working directory.
    pub cwd: Option<String>,
    /// Optional portable command allow-list.
    pub commands: Option<Vec<String>>,
}

/// Per-execution options compatible with upstream Just Bash's core test surface.
#[napi(object)]
pub struct RustBashExecOptions {
    /// Environment variables applied for this execution only.
    pub env: Option<HashMap<String, String>>,
    /// Replace the base environment before applying `env`.
    #[napi(js_name = "replaceEnv")]
    pub replace_env: Option<bool>,
    /// Working directory for this execution only.
    pub cwd: Option<String>,
    /// Standard input for the first command or pipeline.
    pub stdin: Option<String>,
    /// Literal argv entries appended to the first command.
    pub args: Option<Vec<String>>,
    /// Per-execution timeout in milliseconds.
    #[napi(js_name = "timeoutMs")]
    pub timeout_ms: Option<u32>,
}

/// Execution metadata returned by the Rust backend.
#[napi(object)]
pub struct RustBashExecMetadata {
    /// Backend identifier.
    pub backend: String,
    /// True when execution delegated to an external sandbox.
    #[napi(js_name = "externalSandbox")]
    pub external_sandbox: bool,
    /// Effective working directory at execution start.
    pub cwd: String,
    /// Effective timeout in milliseconds.
    #[napi(js_name = "timeoutMs")]
    pub timeout_ms: u32,
    /// Number of simple commands attempted.
    #[napi(js_name = "commandCount")]
    pub command_count: u32,
    /// True when stdout or stderr was truncated.
    pub truncated: bool,
}

/// Result returned from `RustBash.exec`.
#[napi(object)]
pub struct RustBashExecResult {
    /// Captured stdout.
    pub stdout: String,
    /// Captured stderr.
    pub stderr: String,
    /// Process-like exit code.
    #[napi(js_name = "exitCode")]
    pub exit_code: i32,
    /// Final environment for the isolated execution.
    pub env: HashMap<String, String>,
    /// Execution metadata from the Rust backend.
    pub metadata: RustBashExecMetadata,
}

/// Napi-rs class exposing the Rust Just Bash backend to Node tests.
#[napi]
pub struct RustBash {
    inner: Bash,
    cwd: String,
}

#[napi]
impl RustBash {
    /// Creates a Rust-backed Just Bash session.
    #[napi(constructor)]
    pub fn new(options: Option<RustBashOptions>) -> Self {
        let options = options.unwrap_or_default();
        let cwd = options.cwd.unwrap_or_else(|| "/home/user".to_string());
        let inner = Bash::with_options(BashOptions {
            commands: options.commands,
            files: into_btree(options.files),
            env: into_btree(options.env),
            cwd: Some(cwd.clone()),
        });

        Self { inner, cwd }
    }

    /// Executes a script and returns the Rust backend's shell-shaped result.
    #[napi]
    pub fn exec(&self, script: String, options: Option<RustBashExecOptions>) -> RustBashExecResult {
        let result = self
            .inner
            .exec_with_options(script, into_exec_options(options));
        into_napi_result(result)
    }

    /// Reads a UTF-8 virtual file.
    #[napi(js_name = "readFile")]
    pub fn read_file(&self, path: String) -> Result<String> {
        self.inner
            .read_file(&resolve_path(&self.cwd, &path))
            .map_err(into_napi_error)
    }

    /// Writes a UTF-8 virtual file.
    #[napi(js_name = "writeFile")]
    pub fn write_file(&self, path: String, content: String) -> Result<()> {
        self.inner
            .write_file(&resolve_path(&self.cwd, &path), &content)
            .map_err(into_napi_error)
    }

    /// Returns true when a virtual path exists.
    #[napi(js_name = "fileExists")]
    pub fn file_exists(&self, path: String) -> bool {
        self.inner.file_exists(&resolve_path(&self.cwd, &path))
    }

    /// Returns the constructor working directory.
    #[napi(js_name = "getCwd")]
    pub fn get_cwd(&self) -> String {
        self.cwd.clone()
    }

    /// Returns the current base environment observed through an empty exec.
    #[napi(js_name = "getEnv")]
    pub fn get_env(&self) -> HashMap<String, String> {
        self.inner.exec("").env.into_iter().collect()
    }

    /// Returns sorted registered command names.
    #[napi(js_name = "registeredCommandNames")]
    pub fn registered_command_names(&self) -> Vec<String> {
        self.inner.registered_command_names()
    }
}

fn into_btree(values: Option<HashMap<String, String>>) -> BTreeMap<String, String> {
    values
        .unwrap_or_default()
        .into_iter()
        .collect::<BTreeMap<_, _>>()
}

fn into_exec_options(options: Option<RustBashExecOptions>) -> JustBashExecOptions {
    let Some(options) = options else {
        return JustBashExecOptions::new();
    };

    let mut exec_options = JustBashExecOptions::new();
    if let Some(env) = options.env {
        exec_options = exec_options.with_envs(env.into_iter().collect());
    }
    if let Some(replace_env) = options.replace_env {
        exec_options = exec_options.with_replace_env(replace_env);
    }
    if let Some(cwd) = options.cwd {
        exec_options = exec_options.with_cwd(cwd);
    }
    if let Some(stdin) = options.stdin {
        exec_options = exec_options.with_stdin(stdin);
    }
    if let Some(args) = options.args {
        exec_options = exec_options.with_args(args);
    }
    if let Some(timeout_ms) = options.timeout_ms {
        exec_options = exec_options.with_timeout_ms(u64::from(timeout_ms));
    }
    exec_options
}

fn into_napi_result(result: JustBashExecResult) -> RustBashExecResult {
    RustBashExecResult {
        stdout: result.stdout,
        stderr: result.stderr,
        exit_code: result.exit_code,
        env: result.env.into_iter().collect(),
        metadata: RustBashExecMetadata {
            backend: result.metadata.backend,
            external_sandbox: result.metadata.external_sandbox,
            cwd: result.metadata.cwd,
            timeout_ms: u32::try_from(result.metadata.timeout_ms).unwrap_or(u32::MAX),
            command_count: u32::try_from(result.metadata.command_count).unwrap_or(u32::MAX),
            truncated: result.metadata.truncated,
        },
    }
}

fn into_napi_error(error: JustBashError) -> Error {
    Error::new(Status::GenericFailure, error.to_string())
}
