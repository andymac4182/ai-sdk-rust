//! Node-API bindings for the Rust `just-bash` backend.
//!
//! The handwritten adapter code does not use `unsafe`, but napi-rs generates
//! Node-API FFI glue from the `#[napi]` macros. This crate scopes the
//! `unsafe_code` lint exception to that generated boundary.

use std::collections::{BTreeMap, HashMap};

use just_bash::{
    Bash, BashOptions, JustBashError, JustBashExecOptions, JustBashExecResult,
    plan_just_bash_cli_args, resolve_path,
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

/// Planned upstream-style CLI invocation returned by `planCliInvocation`.
#[napi(object)]
pub struct RustJustBashCliPlan {
    /// Top-level action: help, version, or execute.
    pub action: String,
    /// Script source: inline, script-file, stdin, or none.
    #[napi(js_name = "scriptSource")]
    pub script_source: String,
    /// Inline script from `-c`, when present.
    pub script: Option<String>,
    /// Script file path from the first positional argument, when present.
    #[napi(js_name = "scriptFile")]
    pub script_file: Option<String>,
    /// Resolved host root path.
    pub root: String,
    /// Requested virtual cwd.
    pub cwd: String,
    /// Effective virtual cwd passed to Bash.
    #[napi(js_name = "effectiveCwd")]
    pub effective_cwd: String,
    /// True when `--cwd` was provided.
    #[napi(js_name = "cwdOverridden")]
    pub cwd_overridden: bool,
    /// True when `-e` or `--errexit` was provided.
    pub errexit: bool,
    /// True when writes are allowed by the upstream OverlayFS.
    #[napi(js_name = "allowWrite")]
    pub allow_write: bool,
    /// True when optional Python commands are enabled.
    pub python: bool,
    /// True when optional JavaScript commands are enabled.
    pub javascript: bool,
    /// True when JSON CLI output is requested.
    pub json: bool,
    /// True when help output was requested.
    pub help: bool,
    /// True when version output was requested.
    pub version: bool,
    /// Help or version output for non-exec actions.
    pub output: Option<String>,
    /// Process-like exit code for non-exec actions.
    #[napi(js_name = "exitCode")]
    pub exit_code: i32,
    /// Script file resolved to the virtual mount point, when applicable.
    #[napi(js_name = "virtualScriptFilePath")]
    pub virtual_script_file_path: Option<String>,
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
            ..BashOptions::default()
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

/// Plans an upstream-style `just-bash` CLI invocation without host filesystem access.
#[napi(js_name = "planCliInvocation")]
pub fn plan_cli_invocation(
    args: Vec<String>,
    #[napi(ts_arg_type = "string | undefined")] process_cwd: Option<String>,
    #[napi(ts_arg_type = "boolean | undefined")] stdin_is_tty: Option<bool>,
) -> Result<RustJustBashCliPlan> {
    let process_cwd = process_cwd.unwrap_or_else(|| {
        std::env::current_dir()
            .ok()
            .and_then(|path| path.to_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| "/".to_string())
    });
    let plan = plan_just_bash_cli_args(args, &process_cwd, stdin_is_tty.unwrap_or(true))
        .map_err(|error| Error::new(Status::InvalidArg, error.to_string()))?;
    Ok(RustJustBashCliPlan {
        action: plan.action.as_str().to_string(),
        script_source: plan.script_source.as_str().to_string(),
        script: plan.options.script.clone(),
        script_file: plan.options.script_file.clone(),
        root: plan.options.root.clone(),
        cwd: plan.options.cwd.clone(),
        effective_cwd: plan.effective_cwd.clone(),
        cwd_overridden: plan.options.cwd_overridden,
        errexit: plan.options.errexit,
        allow_write: plan.options.allow_write,
        python: plan.options.python,
        javascript: plan.options.javascript,
        json: plan.options.json,
        help: plan.options.help,
        version: plan.options.version,
        output: plan.output.clone(),
        exit_code: plan.exit_code,
        virtual_script_file_path: plan.virtual_script_file_path(),
    })
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
