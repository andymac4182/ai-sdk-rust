//! Just Bash-style in-process shell session for Open Agents.
//!
//! This module owns the JB-02 core contract only: session state, exec
//! options/results, cancellation/time limits, default metadata, and an inline
//! executor tool bridge. It intentionally does not replace any Open Agents
//! runtime path; JB-07 can choose where to wire this backend.

use std::cmp::Ordering as CmpOrdering;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};

use crate::commands::CommandRegistry;
use crate::encoding::{BufferEncoding, OutputPayload, bytes_to_string};
use crate::error::{JustBashError, JustBashErrorKind, JustBashResult};
use crate::fs::{CpOptions, DirentEntry, FileStat, MkdirOptions, RmOptions, VirtualFileSystem};
use crate::path::resolve_path;
use crate::security::{
    HttpMethod, NetworkPolicy, NetworkRequest, NetworkResponse, StaticNetworkTransport,
    execute_network_request,
};

/// Stable metadata label for the Rust in-process backend.
pub const JUST_BASH_BACKEND: &str = "rust-just-bash";

/// Exit status used for cancelled or timed-out commands.
pub const JUST_BASH_TIMEOUT_EXIT_CODE: i32 = 124;

/// Default timeout for a Just Bash execution, in milliseconds.
pub const JUST_BASH_DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// Default maximum captured stdout or stderr length.
pub const JUST_BASH_DEFAULT_MAX_OUTPUT_LENGTH: usize = 50_000;

/// Cancellation handle used by [`JustBashExecOptions`].
#[derive(Clone, Default)]
pub struct JustBashCancelToken {
    cancelled: Arc<AtomicBool>,
}

impl JustBashCancelToken {
    /// Creates a new non-cancelled token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Returns true when cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

impl fmt::Debug for JustBashCancelToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JustBashCancelToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// Per-execution options. Every exec starts from the session defaults, then
/// applies this structure without mutating the session shell state.
#[derive(Clone, Debug, Default)]
pub struct JustBashExecOptions {
    /// Environment variables applied for this exec only.
    pub env: BTreeMap<String, String>,
    /// Start from an empty environment before applying [`Self::env`].
    pub replace_env: bool,
    /// Working directory for this exec only.
    pub cwd: Option<String>,
    /// Standard input passed to the first command or pipeline.
    pub stdin: Option<String>,
    /// Literal argv entries appended to the first command.
    pub args: Vec<String>,
    /// Optional per-exec timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// Optional cooperative cancellation token.
    pub cancel_token: Option<JustBashCancelToken>,
}

impl JustBashExecOptions {
    /// Creates empty exec options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one per-exec environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Adds many per-exec environment variables.
    pub fn with_envs(mut self, env: BTreeMap<String, String>) -> Self {
        self.env.extend(env);
        self
    }

    /// Sets `replace_env`.
    pub fn with_replace_env(mut self, replace_env: bool) -> Self {
        self.replace_env = replace_env;
        self
    }

    /// Sets the working directory for this exec.
    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Sets standard input for this exec.
    pub fn with_stdin(mut self, stdin: impl Into<String>) -> Self {
        self.stdin = Some(stdin.into());
        self
    }

    /// Sets literal arguments appended to the first command.
    pub fn with_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Sets a per-exec timeout.
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// Sets the cancellation token.
    pub fn with_cancel_token(mut self, cancel_token: JustBashCancelToken) -> Self {
        self.cancel_token = Some(cancel_token);
        self
    }
}

/// Metadata returned with every [`JustBashExecResult`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JustBashExecMetadata {
    /// Backend identifier.
    pub backend: String,
    /// True when execution delegated to a provider outside this process.
    pub external_sandbox: bool,
    /// Effective working directory at exec start.
    pub cwd: String,
    /// Effective timeout in milliseconds.
    pub timeout_ms: u64,
    /// Number of simple commands attempted.
    pub command_count: usize,
    /// True when stdout or stderr was truncated.
    pub truncated: bool,
}

impl JustBashExecMetadata {
    fn new(cwd: String, timeout_ms: u64) -> Self {
        Self {
            backend: JUST_BASH_BACKEND.to_string(),
            external_sandbox: false,
            cwd,
            timeout_ms,
            command_count: 0,
            truncated: false,
        }
    }
}

/// Result from [`JustBashSession::exec`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JustBashExecResult {
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
    /// Process-like exit code.
    pub exit_code: i32,
    /// Final environment for the isolated exec.
    pub env: BTreeMap<String, String>,
    /// Default execution metadata.
    pub metadata: JustBashExecMetadata,
}

impl JustBashExecResult {
    /// Returns true when the exit code is zero.
    pub const fn success(&self) -> bool {
        self.exit_code == 0
    }
}

/// Result returned by a Rust custom command.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JustBashCustomCommandResult {
    /// Captured standard output.
    pub stdout: String,
    /// Captured standard error.
    pub stderr: String,
    /// Process-like exit code.
    pub exit_code: i32,
}

impl JustBashCustomCommandResult {
    /// Creates a custom-command result.
    pub fn new(stdout: impl Into<String>, stderr: impl Into<String>, exit_code: i32) -> Self {
        Self {
            stdout: stdout.into(),
            stderr: stderr.into(),
            exit_code,
        }
    }

    /// Creates a successful stdout-only result.
    pub fn stdout(stdout: impl Into<String>) -> Self {
        Self::new(stdout, "", 0)
    }

    /// Creates a failing stderr-only result.
    pub fn stderr(exit_code: i32, stderr: impl Into<String>) -> Self {
        Self::new("", stderr, exit_code)
    }
}

impl From<JustBashExecResult> for JustBashCustomCommandResult {
    fn from(result: JustBashExecResult) -> Self {
        Self {
            stdout: result.stdout,
            stderr: result.stderr,
            exit_code: result.exit_code,
        }
    }
}

/// Context passed to a Rust custom command handler.
#[derive(Clone, Debug)]
pub struct JustBashCustomCommandContext {
    /// Arguments after the command name.
    pub args: Vec<String>,
    /// Effective working directory for the current exec.
    pub cwd: String,
    /// Effective environment for the current exec.
    pub env: BTreeMap<String, String>,
    /// Standard input received from a pipeline or per-exec stdin.
    pub stdin: String,
    session: JustBashSession,
}

impl JustBashCustomCommandContext {
    /// Reads a UTF-8 virtual file relative to the current command cwd.
    pub fn read_file(&self, path: &str) -> JustBashResult<String> {
        self.session.read_file(&resolve_path(&self.cwd, path))
    }

    /// Writes a UTF-8 virtual file relative to the current command cwd.
    pub fn write_file(&self, path: &str, content: &str) -> JustBashResult<()> {
        self.session
            .write_file(&resolve_path(&self.cwd, path), content)
    }

    /// Executes a subcommand in the same virtual session.
    pub fn exec(&self, script: impl AsRef<str>) -> JustBashExecResult {
        self.exec_with_options(script, JustBashExecOptions::new())
    }

    /// Executes a subcommand with explicit per-exec overrides.
    pub fn exec_with_options(
        &self,
        script: impl AsRef<str>,
        mut options: JustBashExecOptions,
    ) -> JustBashExecResult {
        let mut env = self.env.clone();
        env.extend(options.env);
        options.env = env;
        if options.cwd.is_none() {
            options.cwd = Some(self.cwd.clone());
        }
        self.session.exec(script, options)
    }
}

enum JustBashCustomCommandKind {
    Eager(Arc<dyn Fn(JustBashCustomCommandContext) -> JustBashCustomCommandResult + Send + Sync>),
    Lazy {
        loader: Arc<dyn Fn() -> JustBashCustomCommand + Send + Sync>,
        cached: Arc<Mutex<Option<JustBashCustomCommand>>>,
    },
}

impl fmt::Debug for JustBashCustomCommandKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Eager(_) => formatter.write_str("Eager(..)"),
            Self::Lazy { cached, .. } => formatter
                .debug_struct("Lazy")
                .field(
                    "loaded",
                    &cached.lock().is_ok_and(|cached| cached.is_some()),
                )
                .finish(),
        }
    }
}

/// Public Rust custom command registered on a [`JustBashSession`].
#[derive(Clone, Debug)]
pub struct JustBashCustomCommand {
    name: String,
    kind: Arc<JustBashCustomCommandKind>,
}

impl JustBashCustomCommand {
    /// Creates an eager custom command from a synchronous Rust handler.
    pub fn new(
        name: impl Into<String>,
        handler: impl Fn(JustBashCustomCommandContext) -> JustBashCustomCommandResult
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            kind: Arc::new(JustBashCustomCommandKind::Eager(Arc::new(handler))),
        }
    }

    /// Creates a lazy custom command that loads once on first execution.
    pub fn lazy(
        name: impl Into<String>,
        loader: impl Fn() -> JustBashCustomCommand + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            kind: Arc::new(JustBashCustomCommandKind::Lazy {
                loader: Arc::new(loader),
                cached: Arc::new(Mutex::new(None)),
            }),
        }
    }

    /// Returns the command name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns true for lazy custom commands.
    pub fn is_lazy(&self) -> bool {
        matches!(&*self.kind, JustBashCustomCommandKind::Lazy { .. })
    }

    fn execute(&self, context: JustBashCustomCommandContext) -> JustBashCustomCommandResult {
        match &*self.kind {
            JustBashCustomCommandKind::Eager(handler) => handler(context),
            JustBashCustomCommandKind::Lazy { loader, cached } => {
                let loaded = match cached.lock() {
                    Ok(mut cached) => {
                        if cached.is_none() {
                            *cached = Some(loader());
                        }
                        cached.clone()
                    }
                    Err(_) => {
                        return JustBashCustomCommandResult::stderr(
                            1,
                            format!("{}: lazy command cache poisoned\n", self.name),
                        );
                    }
                };
                loaded
                    .expect("lazy custom command should be loaded")
                    .execute(context)
            }
        }
    }
}

/// Inline executor tool.
pub struct JustBashExecutorTool {
    description: Option<String>,
    handler: Arc<dyn Fn(JsonValue) -> Result<JsonValue, String> + Send + Sync>,
}

impl fmt::Debug for JustBashExecutorTool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JustBashExecutorTool")
            .field("description", &self.description)
            .finish_non_exhaustive()
    }
}

impl Clone for JustBashExecutorTool {
    fn clone(&self) -> Self {
        Self {
            description: self.description.clone(),
            handler: Arc::clone(&self.handler),
        }
    }
}

impl JustBashExecutorTool {
    /// Creates an inline executor tool from a synchronous Rust handler.
    pub fn new(
        description: Option<impl Into<String>>,
        handler: impl Fn(JsonValue) -> Result<JsonValue, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            description: description.map(Into::into),
            handler: Arc::new(handler),
        }
    }
}

/// Inline executor command registry.
#[derive(Clone, Debug, Default)]
pub struct JustBashExecutor {
    tools: BTreeMap<String, JustBashExecutorTool>,
    expose_tools_as_commands: bool,
}

impl JustBashExecutor {
    /// Creates an executor that exposes tools as namespace commands.
    pub fn new() -> Self {
        Self {
            tools: BTreeMap::new(),
            expose_tools_as_commands: true,
        }
    }

    /// Registers a tool path such as `math.add`.
    pub fn with_tool(mut self, path: impl Into<String>, tool: JustBashExecutorTool) -> Self {
        self.tools.insert(path.into(), tool);
        self
    }

    /// Controls whether tools are visible as shell namespace commands.
    pub fn with_expose_tools_as_commands(mut self, expose: bool) -> Self {
        self.expose_tools_as_commands = expose;
        self
    }

    fn subcommands(&self, namespace: &str) -> Vec<ToolSubcommand> {
        if !self.expose_tools_as_commands {
            return Vec::new();
        }
        self.tools
            .iter()
            .filter_map(|(path, tool)| {
                let (tool_namespace, raw_name) = path.split_once('.')?;
                if tool_namespace != namespace {
                    return None;
                }
                let name = camel_to_kebab(raw_name);
                let aliases = if name == raw_name {
                    Vec::new()
                } else {
                    vec![raw_name.to_string()]
                };
                Some(ToolSubcommand {
                    name,
                    original_path: path.clone(),
                    description: tool.description.clone(),
                    aliases,
                })
            })
            .collect()
    }

    fn invoke(&self, path: &str, args: JsonValue) -> Result<JsonValue, String> {
        let tool = self
            .tools
            .get(path)
            .ok_or_else(|| format!("Unknown tool: {path}"))?;
        (tool.handler)(args)
    }
}

/// Session construction options.
#[derive(Clone, Debug)]
pub struct JustBashSessionOptions {
    /// Initial virtual files.
    pub files: BTreeMap<String, String>,
    /// Base environment available to every exec.
    pub env: BTreeMap<String, String>,
    /// Base working directory.
    pub cwd: Option<String>,
    /// Default timeout in milliseconds.
    pub default_timeout_ms: Option<u64>,
    /// Maximum captured stdout or stderr length.
    pub max_output_length: Option<usize>,
    /// Maximum simple commands per exec.
    pub max_command_count: Option<usize>,
    /// Optional executor command registry.
    pub executor: Option<JustBashExecutor>,
    /// Optional portable command allow-list.
    pub commands: Option<Vec<String>>,
    /// Public Rust custom commands available before built-ins.
    pub custom_commands: Vec<JustBashCustomCommand>,
    /// Whether to create the upstream default `/home/user` layout.
    pub create_default_layout: bool,
    /// Optional network policy. When present, `curl` is registered and routed
    /// through the fake responses below.
    pub network_policy: Option<NetworkPolicy>,
    /// Fake HTTP responses keyed by URL for deterministic `curl` execution.
    pub network_responses: BTreeMap<String, NetworkResponse>,
}

impl Default for JustBashSessionOptions {
    fn default() -> Self {
        Self {
            files: BTreeMap::new(),
            env: BTreeMap::new(),
            cwd: None,
            default_timeout_ms: None,
            max_output_length: None,
            max_command_count: None,
            executor: None,
            commands: None,
            custom_commands: Vec::new(),
            create_default_layout: true,
            network_policy: None,
            network_responses: BTreeMap::new(),
        }
    }
}

impl JustBashSessionOptions {
    /// Creates empty session options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an initial file.
    pub fn with_file(mut self, path: impl Into<String>, content: impl Into<String>) -> Self {
        self.files.insert(path.into(), content.into());
        self
    }

    /// Adds a base environment variable.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Sets the base working directory.
    pub fn with_cwd(mut self, cwd: impl Into<String>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Sets the default timeout.
    pub fn with_default_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.default_timeout_ms = Some(timeout_ms);
        self
    }

    /// Sets the maximum captured stdout or stderr length.
    pub fn with_max_output_length(mut self, max_output_length: usize) -> Self {
        self.max_output_length = Some(max_output_length);
        self
    }

    /// Sets the maximum command count.
    pub fn with_max_command_count(mut self, max_command_count: usize) -> Self {
        self.max_command_count = Some(max_command_count);
        self
    }

    /// Sets the executor contract for namespace tools.
    pub fn with_executor(mut self, executor: JustBashExecutor) -> Self {
        self.executor = Some(executor);
        self
    }

    /// Restricts the portable command registry.
    pub fn with_commands<I, S>(mut self, commands: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.commands = Some(commands.into_iter().map(Into::into).collect());
        self
    }

    /// Adds one Rust custom command.
    pub fn with_custom_command(mut self, command: JustBashCustomCommand) -> Self {
        self.custom_commands.push(command);
        self
    }

    /// Adds many Rust custom commands.
    pub fn with_custom_commands<I>(mut self, commands: I) -> Self
    where
        I: IntoIterator<Item = JustBashCustomCommand>,
    {
        self.custom_commands.extend(commands);
        self
    }

    /// Controls whether the upstream default `/home/user` layout is created.
    pub fn with_create_default_layout(mut self, create: bool) -> Self {
        self.create_default_layout = create;
        self
    }

    /// Sets the opt-in network policy used by deterministic `curl`.
    pub fn with_network_policy(mut self, policy: NetworkPolicy) -> Self {
        self.network_policy = Some(policy);
        self
    }

    /// Adds a fake network response used by deterministic `curl`.
    pub fn with_network_response(mut self, response: NetworkResponse) -> Self {
        self.network_responses
            .insert(response.url.clone(), response);
        self
    }
}

/// In-process shell session with a persistent virtual filesystem and fresh
/// shell state for every exec.
#[derive(Clone, Debug)]
pub struct JustBashSession {
    inner: Arc<JustBashSessionInner>,
}

#[derive(Debug)]
struct JustBashSessionInner {
    fs: Mutex<VirtualFileSystem>,
    base_env: BTreeMap<String, String>,
    base_cwd: String,
    default_timeout_ms: u64,
    max_output_length: usize,
    max_command_count: usize,
    executor: Option<JustBashExecutor>,
    commands: CommandRegistry,
    custom_commands: BTreeMap<String, JustBashCustomCommand>,
    network_policy: Option<NetworkPolicy>,
    network_responses: BTreeMap<String, NetworkResponse>,
}

impl JustBashSession {
    /// Creates a memory-backed session with the upstream-style default layout.
    pub fn new() -> Self {
        Self::with_options(JustBashSessionOptions::new())
    }

    /// Creates a memory-backed session.
    pub fn with_options(options: JustBashSessionOptions) -> Self {
        let cwd = options.cwd.unwrap_or_else(|| {
            if options.create_default_layout {
                "/home/user".to_string()
            } else {
                "/".to_string()
            }
        });
        let network_enabled = options.network_policy.is_some();
        let commands = options
            .commands
            .as_deref()
            .map(|commands| CommandRegistry::filtered_with_network(commands, network_enabled))
            .unwrap_or_else(|| {
                let registry = CommandRegistry::default_portable();
                if network_enabled {
                    registry.with_network_commands()
                } else {
                    registry
                }
            });
        let mut fs = VirtualFileSystem::new();
        fs.mkdir("/bin", MkdirOptions { recursive: true })
            .expect("default Just Bash /bin directory is valid");
        for name in commands.names() {
            fs.write_file(&format!("/bin/{name}"), "")
                .expect("command stub path is valid");
        }
        let layout_dirs: Vec<&str> = if options.create_default_layout {
            vec!["/tmp", "/home", "/home/user", &cwd]
        } else if cwd == "/" {
            Vec::new()
        } else {
            vec![&cwd]
        };
        for dir in layout_dirs {
            fs.mkdir(dir, MkdirOptions { recursive: true })
                .expect("default Just Bash directory is valid");
        }
        for (path, content) in &options.files {
            fs.write_file(path, content.as_str())
                .expect("initial Just Bash file path is valid");
        }
        let mut base_env = default_env(&cwd);
        base_env.extend(options.env);

        Self {
            inner: Arc::new(JustBashSessionInner {
                fs: Mutex::new(fs),
                base_env,
                base_cwd: cwd,
                default_timeout_ms: options
                    .default_timeout_ms
                    .unwrap_or(JUST_BASH_DEFAULT_TIMEOUT_MS),
                max_output_length: options
                    .max_output_length
                    .unwrap_or(JUST_BASH_DEFAULT_MAX_OUTPUT_LENGTH),
                max_command_count: options.max_command_count.unwrap_or(10_000),
                executor: options.executor,
                commands,
                custom_commands: options
                    .custom_commands
                    .into_iter()
                    .map(|command| (command.name().to_string(), command))
                    .collect(),
                network_policy: options.network_policy,
                network_responses: options.network_responses,
            }),
        }
    }

    /// Executes a script with isolated environment, functions, and cwd.
    pub fn exec(
        &self,
        script: impl AsRef<str>,
        options: JustBashExecOptions,
    ) -> JustBashExecResult {
        let timeout_ms = options
            .timeout_ms
            .unwrap_or(self.inner.default_timeout_ms)
            .max(1);
        let cwd = options
            .cwd
            .clone()
            .unwrap_or_else(|| self.inner.base_cwd.clone());
        let mut env = if options.replace_env {
            BTreeMap::new()
        } else {
            self.inner.base_env.clone()
        };
        env.extend(options.env.clone());
        env.insert("PWD".to_string(), cwd.clone());
        env.entry("OLDPWD".to_string())
            .or_insert_with(|| cwd.clone());

        let mut state = ExecState {
            session: self,
            env,
            cwd: cwd.clone(),
            previous_cwd: self.inner.base_cwd.clone(),
            functions: BTreeMap::new(),
            stdin: options.stdin.unwrap_or_default(),
            extra_args: options.args,
            extra_args_consumed: false,
            started_at: Instant::now(),
            timeout_ms,
            cancel_token: options.cancel_token,
            command_count: 0,
            max_command_count: self.inner.max_command_count,
        };
        let mut script = script.as_ref().to_string();
        extract_function_definitions(&mut script, &mut state.functions);
        let mut result = execute_control_script(&mut state, &script);
        let truncated = cap_output(&mut result, self.inner.max_output_length);

        JustBashExecResult {
            stdout: result.stdout,
            stderr: result.stderr,
            exit_code: result.exit_code,
            env: state.env,
            metadata: JustBashExecMetadata {
                command_count: state.command_count,
                truncated,
                ..JustBashExecMetadata::new(cwd, timeout_ms)
            },
        }
    }

    /// Reads a UTF-8 file from the session filesystem.
    pub fn read_file(&self, path: &str) -> JustBashResult<String> {
        let fs = self.inner.fs.lock().map_err(|_| lock_poisoned_error())?;
        fs.read_file(path)
    }

    /// Reads raw bytes from the session filesystem.
    pub fn read_file_buffer(&self, path: &str) -> JustBashResult<Vec<u8>> {
        let fs = self.inner.fs.lock().map_err(|_| lock_poisoned_error())?;
        fs.read_file_buffer(path)
    }

    /// Writes a UTF-8 file to the session filesystem.
    pub fn write_file(&self, path: &str, content: &str) -> JustBashResult<()> {
        let mut fs = self.inner.fs.lock().map_err(|_| lock_poisoned_error())?;
        fs.write_file(path, content)
    }

    /// Returns file or directory information from the session filesystem.
    pub fn stat(&self, path: &str) -> JustBashResult<FileStat> {
        let fs = self.inner.fs.lock().map_err(|_| lock_poisoned_error())?;
        fs.stat(path)
    }

    /// Creates a directory in the session filesystem.
    pub fn mkdir(&self, path: &str, options: MkdirOptions) -> JustBashResult<()> {
        let mut fs = self.inner.fs.lock().map_err(|_| lock_poisoned_error())?;
        fs.mkdir(path, options)
    }

    /// Reads a directory with virtual file types from the session filesystem.
    pub fn readdir_with_file_types(&self, path: &str) -> JustBashResult<Vec<DirentEntry>> {
        let fs = self.inner.fs.lock().map_err(|_| lock_poisoned_error())?;
        fs.readdir_with_file_types(path)
    }

    /// Returns true when a virtual path exists.
    pub fn file_exists(&self, path: &str) -> bool {
        self.inner.fs.lock().is_ok_and(|fs| fs.stat(path).is_ok())
    }

    /// Returns sorted registered portable command names.
    pub fn registered_command_names(&self) -> Vec<String> {
        let mut names = self.inner.commands.names();
        names.extend(self.inner.custom_commands.keys().cloned());
        names.sort();
        names.dedup();
        names
    }

    /// Returns the session's persistent starting working directory.
    pub fn get_cwd(&self) -> String {
        self.inner.base_cwd.clone()
    }

    /// Returns the session's persistent starting environment.
    pub fn get_env(&self) -> BTreeMap<String, String> {
        self.inner.base_env.clone()
    }
}

impl Default for JustBashSession {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
struct ToolSubcommand {
    name: String,
    original_path: String,
    description: Option<String>,
    aliases: Vec<String>,
}

struct ExecState<'a> {
    session: &'a JustBashSession,
    env: BTreeMap<String, String>,
    cwd: String,
    previous_cwd: String,
    functions: BTreeMap<String, String>,
    stdin: String,
    extra_args: Vec<String>,
    extra_args_consumed: bool,
    started_at: Instant,
    timeout_ms: u64,
    cancel_token: Option<JustBashCancelToken>,
    command_count: usize,
    max_command_count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct CommandResult {
    stdout: String,
    stderr: String,
    exit_code: i32,
    exit_requested: bool,
}

fn execute_control_script(state: &mut ExecState<'_>, script: &str) -> CommandResult {
    let mut combined = CommandResult::default();
    let mut last_exit = 0;
    for (op, command) in split_control(script) {
        let run = match op {
            ControlOp::Always => true,
            ControlOp::And => last_exit == 0,
            ControlOp::Or => last_exit != 0,
        };
        if !run {
            continue;
        }
        let result = execute_pipeline(state, &command);
        combined.stdout.push_str(&result.stdout);
        combined.stderr.push_str(&result.stderr);
        combined.exit_code = result.exit_code;
        last_exit = result.exit_code;
        if result.exit_requested {
            combined.exit_requested = true;
            break;
        }
    }
    combined
}

fn execute_pipeline(state: &mut ExecState<'_>, command: &str) -> CommandResult {
    let mut stdin = state.stdin.clone();
    let mut final_result = CommandResult::default();
    let mut stderr = String::new();
    for part in split_pipeline(command) {
        let result = execute_simple_command(state, part.trim(), stdin);
        stderr.push_str(&result.stderr);
        stdin = result.stdout.clone();
        final_result = result;
        if final_result.exit_requested {
            break;
        }
    }
    final_result.stderr = stderr;
    final_result
}

fn execute_simple_command(
    state: &mut ExecState<'_>,
    command: &str,
    stdin: String,
) -> CommandResult {
    if command.trim().is_empty() {
        return CommandResult::default();
    }
    if let Some(cancelled) = cancelled_result(state) {
        return cancelled;
    }
    state.command_count += 1;
    if state.command_count > state.max_command_count {
        return stderr_result(
            1,
            format!(
                "bash: maximum command count ({}) exceeded\n",
                state.max_command_count
            ),
        );
    }
    if let Some(result) = execute_assignment_substitution(state, command, &stdin) {
        return result;
    }

    let (command, redirect) = split_stdout_redirection(command);
    let (command, stdin_redirect) = split_stdin_redirection(command);
    let stdin = if let Some(path) = stdin_redirect {
        let path = resolve_path(&state.cwd, &path);
        match state.session.inner.fs.lock() {
            Ok(fs) => match fs.read_file(&path) {
                Ok(content) => content,
                Err(_) => return stderr_result(1, format!("bash: {path}: No such file\n")),
            },
            Err(_) => return stderr_result(1, "bash: filesystem lock poisoned\n"),
        }
    } else {
        stdin
    };
    let mut tokens = match tokenize(command, &state.env) {
        Ok(tokens) => tokens,
        Err(error) => return stderr_result(2, format!("bash: {error}\n")),
    };
    let mut stdout_to_stderr = false;
    let mut stderr_to_stdout = false;
    tokens.retain(|token| match token.as_str() {
        ">&2" | "1>&2" => {
            stdout_to_stderr = true;
            false
        }
        "2>&1" => {
            stderr_to_stdout = true;
            false
        }
        _ => true,
    });
    if !state.extra_args_consumed && !tokens.is_empty() {
        tokens.extend(state.extra_args.clone());
        state.extra_args_consumed = true;
    }
    if tokens.is_empty() {
        return CommandResult::default();
    }

    let mut result = execute_tokens(state, &tokens, stdin);
    if stdout_to_stderr {
        result.stderr.push_str(&result.stdout);
        result.stdout.clear();
    }
    if stderr_to_stdout {
        result.stdout.push_str(&result.stderr);
        result.stderr.clear();
    }
    if let Some((path, append)) = redirect
        && result.exit_code == 0
    {
        let write = state
            .session
            .inner
            .fs
            .lock()
            .map_err(|_| lock_poisoned_error())
            .and_then(|mut fs| {
                fs.write_redirection(
                    &state.cwd,
                    &path,
                    OutputPayload::Text(result.stdout.clone()),
                    append,
                )
            });
        match write {
            Ok(()) => result.stdout.clear(),
            Err(error) => {
                result.stderr.push_str(&format!("bash: {error}\n"));
                result.exit_code = 1;
            }
        }
    }
    result
}

fn execute_tokens(state: &mut ExecState<'_>, tokens: &[String], stdin: String) -> CommandResult {
    let command = command_basename(&tokens[0]);
    if let Some(body) = state.functions.get(command).cloned() {
        let old_stdin = std::mem::replace(&mut state.stdin, stdin);
        let result = execute_control_script(state, &body);
        state.stdin = old_stdin;
        return result;
    }
    if let Some(result) = execute_custom_command(state, command, &tokens[1..], &stdin) {
        return result;
    }
    if let Some(result) = execute_executor_command(state, command, &tokens[1..], &stdin) {
        return result;
    }
    if !state.session.inner.commands.contains(command) {
        return stderr_result(127, format!("bash: {}: command not found\n", tokens[0]));
    }

    match command {
        "true" => CommandResult::default(),
        "false" => CommandResult {
            exit_code: 1,
            ..CommandResult::default()
        },
        "exit" => CommandResult {
            exit_code: tokens
                .get(1)
                .and_then(|value| value.parse().ok())
                .unwrap_or(0),
            exit_requested: true,
            ..CommandResult::default()
        },
        "pwd" => stdout_result(format!("{}\n", state.cwd)),
        "echo" => command_echo(&tokens[1..]),
        "printf" => command_printf(&tokens[1..]),
        "export" => {
            for arg in &tokens[1..] {
                if let Some((key, value)) = arg.split_once('=') {
                    state.env.insert(key.to_string(), value.to_string());
                }
            }
            CommandResult::default()
        }
        "unset" => {
            for arg in &tokens[1..] {
                state.env.remove(arg);
            }
            CommandResult::default()
        }
        "env" => command_env(state, &tokens[1..], &stdin),
        "printenv" => command_printenv(state, &tokens[1..]),
        "cd" => command_cd(state, tokens.get(1).map(String::as_str)),
        "cat" => command_cat(state, &tokens[1..], &stdin),
        "grep" => command_grep(state, &tokens[1..], &stdin, GrepMode::Regex),
        "fgrep" => command_grep(state, &tokens[1..], &stdin, GrepMode::Fixed),
        "egrep" => command_grep(state, &tokens[1..], &stdin, GrepMode::Regex),
        "rg" => command_rg(state, &tokens[1..], &stdin),
        "sed" => command_sed(state, &tokens[1..], &stdin),
        "awk" => command_awk(state, &tokens[1..], &stdin),
        "head" => command_head(state, &tokens[1..], &stdin),
        "tail" => command_tail(state, &tokens[1..], &stdin),
        "wc" => command_wc(state, &tokens[1..], &stdin),
        "sort" => command_sort(state, &tokens[1..], &stdin),
        "uniq" => command_uniq(state, &tokens[1..], &stdin),
        "cut" => command_cut(state, &tokens[1..], &stdin),
        "tr" => command_tr(&tokens[1..], &stdin),
        "basename" => command_basename_utility(&tokens[1..]),
        "dirname" => command_dirname_utility(&tokens[1..]),
        "ls" => command_ls(state, &tokens[1..]),
        "mkdir" => command_mkdir(state, &tokens[1..]),
        "touch" => command_touch(state, &tokens[1..]),
        "rm" => command_rm(state, &tokens[1..]),
        "cp" => command_cp(state, &tokens[1..]),
        "mv" => command_mv(state, &tokens[1..]),
        "find" => command_find(state, &tokens[1..]),
        "curl" => command_curl(state, &tokens[1..]),
        "read" => command_read(state, &tokens[1..], &stdin),
        "jq" => command_jq(state, &tokens[1..], &stdin),
        "yq" => command_yq(state, &tokens[1..], &stdin),
        "xan" => command_xan(state, &tokens[1..], &stdin),
        "sqlite3" => command_sqlite3(state, &tokens[1..], &stdin),
        "html-to-markdown" => command_html_to_markdown(state, &tokens[1..], &stdin),
        "which" => command_which(state, &tokens[1..]),
        "whoami" => stdout_result("user\n"),
        "sleep" => command_sleep(state, &tokens[1..]),
        "timeout" => command_timeout(state, &tokens[1..], stdin),
        "bash" | "sh" => command_bash(command, state, &tokens[1..], stdin),
        _ => stderr_result(127, format!("bash: {}: command not found\n", tokens[0])),
    }
}

fn command_echo(args: &[String]) -> CommandResult {
    let mut newline = true;
    let mut escapes = false;
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if !arg.starts_with('-') || arg == "-" || arg == "--" {
            break;
        }
        let mut recognized = true;
        for flag in arg[1..].chars() {
            match flag {
                'n' => newline = false,
                'e' => escapes = true,
                'E' => escapes = false,
                _ => {
                    recognized = false;
                    break;
                }
            }
        }
        if !recognized {
            break;
        }
        index += 1;
    }
    let mut output = args[index..].join(" ");
    if escapes {
        output = process_backslash_escapes(&output, EscapeMode::Echo);
    }
    if newline {
        output.push('\n');
    }
    stdout_result(output)
}

fn command_printf(args: &[String]) -> CommandResult {
    if args.first().is_some_and(|arg| arg == "--help") {
        return stdout_result("printf FORMAT [ARGUMENT]...\n");
    }
    let Some(format) = args.first() else {
        return stderr_result(2, "printf: usage: printf FORMAT [ARGUMENT]...\n");
    };
    let mut output = String::new();
    let mut stderr = String::new();
    let mut exit_code = 0;
    let mut arg_index = 1;
    let placeholder_count = count_printf_conversions(format);
    if placeholder_count == 0 {
        output.push_str(&render_printf_format(
            format,
            &[],
            &mut arg_index,
            &mut stderr,
            &mut exit_code,
        ));
        return CommandResult {
            stdout: output,
            stderr,
            exit_code,
            ..CommandResult::default()
        };
    }
    while arg_index < args.len() {
        output.push_str(&render_printf_format(
            format,
            args,
            &mut arg_index,
            &mut stderr,
            &mut exit_code,
        ));
    }
    CommandResult {
        stdout: output,
        stderr,
        exit_code,
        ..CommandResult::default()
    }
}

fn render_printf_format(
    format: &str,
    args: &[String],
    arg_index: &mut usize,
    stderr: &mut String,
    exit_code: &mut i32,
) -> String {
    let mut output = String::new();
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '%' if matches!(chars.peek(), Some('%')) => {
                chars.next();
                output.push('%');
            }
            '%' => {
                let directive = parse_printf_directive(&mut chars);
                let value = args.get(*arg_index).map(String::as_str).unwrap_or("");
                if matches!(directive.specifier, 's' | 'd' | 'i' | 'f' | 'x' | 'X' | 'o') {
                    *arg_index += usize::from(*arg_index < args.len());
                }
                let rendered = match directive.specifier {
                    's' => apply_printf_width(value.to_string(), &directive, false),
                    'd' | 'i' => {
                        let number = parse_printf_i64(value, stderr, exit_code);
                        apply_printf_width(number.to_string(), &directive, true)
                    }
                    'f' => {
                        let number = parse_printf_f64(value, stderr, exit_code);
                        let precision = directive.precision.unwrap_or(6);
                        apply_printf_width(format!("{number:.precision$}"), &directive, true)
                    }
                    'x' => {
                        let number = parse_printf_i64(value, stderr, exit_code);
                        apply_printf_width(format!("{number:x}"), &directive, true)
                    }
                    'X' => {
                        let number = parse_printf_i64(value, stderr, exit_code);
                        apply_printf_width(format!("{number:X}"), &directive, true)
                    }
                    'o' => {
                        let number = parse_printf_i64(value, stderr, exit_code);
                        apply_printf_width(format!("{number:o}"), &directive, true)
                    }
                    other => {
                        let mut raw = String::from("%");
                        raw.push(other);
                        raw
                    }
                };
                output.push_str(&rendered);
            }
            '\\' => output.push_str(&render_escape(&mut chars, EscapeMode::Printf)),
            _ => output.push(ch),
        }
    }
    output
}

fn count_printf_conversions(format: &str) -> usize {
    let mut count = 0;
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            continue;
        }
        match chars.next() {
            Some('%') | None => {}
            Some(ch) => {
                if matches!(ch, '-' | '0' | '1'..='9' | '.') {
                    while matches!(chars.peek(), Some('-' | '0'..='9' | '.')) {
                        chars.next();
                    }
                    if matches!(chars.next(), Some('s' | 'd' | 'i' | 'f' | 'x' | 'X' | 'o')) {
                        count += 1;
                    }
                } else if matches!(ch, 's' | 'd' | 'i' | 'f' | 'x' | 'X' | 'o') {
                    count += 1;
                }
            }
        }
    }
    count
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EscapeMode {
    Echo,
    Printf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PrintfDirective {
    left_justify: bool,
    zero_pad: bool,
    width: Option<usize>,
    precision: Option<usize>,
    specifier: char,
}

fn process_backslash_escapes(input: &str, mode: EscapeMode) -> String {
    let mut output = String::new();
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            output.push_str(&render_escape(&mut chars, mode));
        } else {
            output.push(ch);
        }
    }
    output
}

fn render_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, mode: EscapeMode) -> String {
    let Some(ch) = chars.next() else {
        return "\\".to_string();
    };
    match ch {
        'n' => "\n".to_string(),
        't' => "\t".to_string(),
        'r' => "\r".to_string(),
        '\\' => "\\".to_string(),
        'a' => "\x07".to_string(),
        'b' => "\x08".to_string(),
        'f' => "\x0c".to_string(),
        'v' => "\x0b".to_string(),
        'e' | 'E' => "\u{001b}".to_string(),
        'x' => take_radix_escape(chars, 2, 16).unwrap_or_else(|| "\\x".to_string()),
        'u' if mode == EscapeMode::Printf => {
            take_radix_escape(chars, 4, 16).unwrap_or_else(|| "\\u".to_string())
        }
        'U' if mode == EscapeMode::Printf => {
            take_radix_escape(chars, 8, 16).unwrap_or_else(|| "\\U".to_string())
        }
        first @ '0'..='7' => {
            let remaining_digits = if mode == EscapeMode::Echo && first == '0' {
                3
            } else {
                2
            };
            let mut digits = String::from(first);
            for _ in 0..remaining_digits {
                if let Some(next @ '0'..='7') = chars.peek().copied() {
                    digits.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            u32::from_str_radix(&digits, 8)
                .ok()
                .and_then(char::from_u32)
                .map(|value| value.to_string())
                .unwrap_or_default()
        }
        other => {
            let mut escaped = String::from("\\");
            escaped.push(other);
            escaped
        }
    }
}

fn take_radix_escape(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    max_digits: usize,
    radix: u32,
) -> Option<String> {
    let mut digits = String::new();
    for _ in 0..max_digits {
        if let Some(next) = chars.peek().copied()
            && next.is_digit(radix)
        {
            digits.push(next);
            chars.next();
        } else {
            break;
        }
    }
    if digits.is_empty() {
        return None;
    }
    u32::from_str_radix(&digits, radix)
        .ok()
        .and_then(char::from_u32)
        .map(|value| value.to_string())
}

fn parse_printf_directive(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> PrintfDirective {
    let mut left_justify = false;
    let mut zero_pad = false;
    loop {
        match chars.peek().copied() {
            Some('-') => {
                left_justify = true;
                chars.next();
            }
            Some('0') => {
                zero_pad = true;
                chars.next();
            }
            _ => break,
        }
    }
    let width = take_decimal(chars);
    let precision = if matches!(chars.peek(), Some('.')) {
        chars.next();
        Some(take_decimal(chars).unwrap_or(0))
    } else {
        None
    };
    let specifier = chars.next().unwrap_or('%');
    PrintfDirective {
        left_justify,
        zero_pad,
        width,
        precision,
        specifier,
    }
}

fn take_decimal(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<usize> {
    let mut digits = String::new();
    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            chars.next();
        } else {
            break;
        }
    }
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

fn apply_printf_width(mut value: String, directive: &PrintfDirective, numeric: bool) -> String {
    if directive.specifier == 's'
        && let Some(precision) = directive.precision
    {
        value = value.chars().take(precision).collect();
    }
    let Some(width) = directive.width else {
        return value;
    };
    let current = value.chars().count();
    if current >= width {
        return value;
    }
    let padding = width - current;
    if directive.left_justify {
        value.push_str(&" ".repeat(padding));
        return value;
    }
    let pad = if directive.zero_pad && numeric {
        '0'
    } else {
        ' '
    };
    let mut output = pad.to_string().repeat(padding);
    output.push_str(&value);
    output
}

fn parse_printf_i64(value: &str, stderr: &mut String, exit_code: &mut i32) -> i64 {
    value.parse::<i64>().unwrap_or_else(|_| {
        if !value.is_empty() {
            stderr.push_str(&format!("printf: {value}: invalid number\n"));
            *exit_code = 1;
        }
        0
    })
}

fn parse_printf_f64(value: &str, stderr: &mut String, exit_code: &mut i32) -> f64 {
    value.parse::<f64>().unwrap_or_else(|_| {
        if !value.is_empty() {
            stderr.push_str(&format!("printf: {value}: invalid number\n"));
            *exit_code = 1;
        }
        0.0
    })
}

fn command_printenv(state: &ExecState<'_>, args: &[String]) -> CommandResult {
    if args.first().is_some_and(|arg| arg == "--help") {
        return stdout_result("Usage: printenv [NAME]...\nPrint environment variables.\n");
    }
    if args.is_empty() {
        let stdout: String = state
            .env
            .iter()
            .map(|(key, value)| format!("{key}={value}\n"))
            .collect();
        return stdout_result(stdout);
    }
    let mut stdout = String::new();
    let mut exit_code = 0;
    for key in args {
        if let Some(value) = state.env.get(key) {
            stdout.push_str(value);
            stdout.push('\n');
        } else {
            exit_code = 1;
        }
    }
    CommandResult {
        stdout,
        exit_code,
        ..CommandResult::default()
    }
}

fn command_env(state: &mut ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    if args.first().is_some_and(|arg| arg == "--help") {
        return stdout_result(
            "Usage: env [OPTION]... [-] [NAME=VALUE]... [COMMAND [ARG]...]\nPrint or run a command in the environment.\n",
        );
    }

    let mut env = state.env.clone();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        match arg.as_str() {
            "-i" | "--ignore-environment" => {
                env.clear();
                index += 1;
            }
            "-u" | "--unset" => {
                if let Some(name) = args.get(index + 1) {
                    env.remove(name);
                    index += 2;
                } else {
                    return stderr_result(125, "env: option requires an argument -- u\n");
                }
            }
            _ if arg.starts_with("-u") && arg.len() > 2 => {
                env.remove(&arg[2..]);
                index += 1;
            }
            _ if is_assignment(arg) => {
                let (key, value) = arg
                    .split_once('=')
                    .expect("assignment contains an equals sign");
                env.insert(key.to_string(), value.to_string());
                index += 1;
            }
            _ => break,
        }
    }

    if index == args.len() {
        let stdout: String = env
            .iter()
            .map(|(key, value)| format!("{key}={value}\n"))
            .collect();
        return stdout_result(stdout);
    }

    let old_env = std::mem::replace(&mut state.env, env);
    let result = execute_tokens(state, &args[index..], stdin.to_string());
    state.env = old_env;
    result
}

fn command_cd(state: &mut ExecState<'_>, target: Option<&str>) -> CommandResult {
    let target = match target {
        Some("-") => state.previous_cwd.clone(),
        Some("~") | None => state
            .env
            .get("HOME")
            .cloned()
            .unwrap_or_else(|| "/home/user".to_string()),
        Some(path) => path.to_string(),
    };
    let next = resolve_path(&state.cwd, &target);
    let stat = match state.session.inner.fs.lock() {
        Ok(fs) => fs.stat(&next),
        Err(_) => return stderr_result(1, "bash: cd: filesystem lock poisoned\n"),
    };
    match stat {
        Ok(stat) if stat.is_directory => {}
        Ok(_) => return stderr_result(1, format!("bash: cd: {target}: Not a directory\n")),
        Err(_) => {
            return stderr_result(
                1,
                format!("bash: cd: {target}: No such file or directory\n"),
            );
        }
    }
    state.previous_cwd = state.cwd.clone();
    state.cwd = next.clone();
    state.env.insert("PWD".to_string(), next);
    state
        .env
        .insert("OLDPWD".to_string(), state.previous_cwd.clone());
    CommandResult::default()
}

fn command_cat(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let mut number_lines = false;
    let mut paths = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-n" | "--number" => number_lines = true,
            _ => paths.push(arg),
        }
    }
    if paths.is_empty() {
        let stdout = if number_lines {
            number_text(stdin, 1).0
        } else {
            stdin.to_string()
        };
        return stdout_result(stdout);
    }
    let fs = match state.session.inner.fs.lock() {
        Ok(fs) => fs,
        Err(_) => return stderr_result(1, "cat: filesystem lock poisoned\n"),
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code = 0;
    let mut next_line_number = 1;
    for path in paths {
        let content = if path == "-" {
            Ok(stdin.to_string())
        } else {
            let path = resolve_path(&state.cwd, path);
            fs.read_file(&path)
                .map_err(|_| format!("cat: {path}: No such file or directory\n"))
        };
        match content {
            Ok(content) => {
                if number_lines {
                    let (numbered, next) = number_text(&content, next_line_number);
                    stdout.push_str(&numbered);
                    next_line_number = next;
                } else {
                    stdout.push_str(&content);
                }
            }
            Err(error) => {
                stderr.push_str(&error);
                exit_code = 1;
            }
        }
    }
    CommandResult {
        stdout,
        stderr,
        exit_code,
        ..CommandResult::default()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GrepMode {
    Regex,
    Fixed,
}

fn command_grep(
    state: &ExecState<'_>,
    args: &[String],
    stdin: &str,
    mode: GrepMode,
) -> CommandResult {
    let mut ignore_case = false;
    let mut invert = false;
    let mut line_number = false;
    let mut count_only = false;
    let mut files_with_matches = false;
    let mut recursive = false;
    let mut word_regexp = false;
    let mut line_regexp = false;
    let mut only_matching = false;
    let mut no_filename = false;
    let mut before_context = 0usize;
    let mut after_context = 0usize;
    let mut max_count: Option<usize> = None;
    let mut pattern = None;
    let mut paths = Vec::new();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        match arg.as_str() {
            "-i" | "--ignore-case" => ignore_case = true,
            "-v" | "--invert-match" => invert = true,
            "-n" | "--line-number" => line_number = true,
            "-c" | "--count" => count_only = true,
            "-l" | "--files-with-matches" => files_with_matches = true,
            "-r" | "-R" | "--recursive" => recursive = true,
            "-w" | "--word-regexp" => word_regexp = true,
            "-x" | "--line-regexp" => line_regexp = true,
            "-o" | "--only-matching" => only_matching = true,
            "-h" | "--no-filename" => no_filename = true,
            "-E" | "--extended-regexp" => {}
            "-e" => {
                pattern = args.get(index + 1).cloned();
                index += 2;
                break;
            }
            "-A" | "-B" | "-C" | "-m" => {
                let value = args
                    .get(index + 1)
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(0);
                match arg.as_str() {
                    "-A" => after_context = value,
                    "-B" => before_context = value,
                    "-C" => {
                        before_context = value;
                        after_context = value;
                    }
                    "-m" => max_count = Some(value),
                    _ => {}
                }
                index += 2;
                continue;
            }
            _ if arg.starts_with("--max-count=") => {
                max_count = arg["--max-count=".len()..].parse().ok();
            }
            "--max-count" => {
                max_count = args.get(index + 1).and_then(|value| value.parse().ok());
                index += 2;
                continue;
            }
            _ if arg.starts_with("--after-context=") => {
                after_context = arg["--after-context=".len()..].parse().unwrap_or(0);
            }
            _ if arg.starts_with("--before-context=") => {
                before_context = arg["--before-context=".len()..].parse().unwrap_or(0);
            }
            _ if arg.starts_with("--context=") => {
                let value = arg["--context=".len()..].parse().unwrap_or(0);
                before_context = value;
                after_context = value;
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                let flags = arg[1..].chars().collect::<Vec<_>>();
                let mut flag_index = 0usize;
                while let Some(flag) = flags.get(flag_index).copied() {
                    match flag {
                        'i' => ignore_case = true,
                        'v' => invert = true,
                        'n' => line_number = true,
                        'c' => count_only = true,
                        'l' => files_with_matches = true,
                        'r' | 'R' => recursive = true,
                        'w' => word_regexp = true,
                        'x' => line_regexp = true,
                        'o' => only_matching = true,
                        'h' => no_filename = true,
                        'E' | 'F' => {}
                        'A' | 'B' | 'C' | 'm' => {
                            let tail = flags[flag_index + 1..].iter().collect::<String>();
                            let value = if tail.is_empty() {
                                args.get(index + 1)
                                    .and_then(|value| value.parse::<usize>().ok())
                                    .unwrap_or(0)
                            } else {
                                tail.parse::<usize>().unwrap_or(0)
                            };
                            match flag {
                                'A' => after_context = value,
                                'B' => before_context = value,
                                'C' => {
                                    before_context = value;
                                    after_context = value;
                                }
                                'm' => max_count = Some(value),
                                _ => {}
                            }
                            if tail.is_empty() {
                                index += 1;
                            }
                            break;
                        }
                        _ => {}
                    }
                    flag_index += 1;
                }
            }
            _ => {
                pattern = Some(arg.clone());
                index += 1;
                break;
            }
        }
        index += 1;
    }

    let Some(pattern) = pattern else {
        return stderr_result(2, "grep: missing pattern\n");
    };
    paths.extend(args[index..].iter().cloned());
    let inputs = if paths.is_empty() {
        vec![NamedTextInput {
            label: String::new(),
            text: stdin.to_string(),
        }]
    } else {
        match grep_inputs(state, &paths, recursive) {
            Ok(inputs) => inputs,
            Err(error) => return stderr_result(2, format!("grep: {error}\n")),
        }
    };
    let matcher = match LineMatcher::new_with_line_regexp(
        &pattern,
        ignore_case,
        word_regexp,
        line_regexp,
        mode,
    ) {
        Ok(matcher) => matcher,
        Err(error) => return stderr_result(2, format!("grep: {error}\n")),
    };
    let mut stdout = String::new();
    let mut matches = 0;
    let show_filename = !no_filename && (paths.len() > 1 || recursive);
    for input in inputs {
        let mut file_matched = false;
        let mut file_matches = 0;
        let lines = input.text.lines().collect::<Vec<_>>();
        let mut matched_indexes = Vec::new();
        for (line_index, line) in lines.iter().enumerate() {
            let matched = matcher.is_match(line);
            if matched ^ invert {
                matches += 1;
                file_matches += 1;
                file_matched = true;
                matched_indexes.push(line_index);
                if files_with_matches {
                    continue;
                }
                if count_only {
                    if max_count.is_some_and(|limit| file_matches >= limit) {
                        break;
                    }
                    continue;
                }
                if before_context > 0 || after_context > 0 {
                    if max_count.is_some_and(|limit| file_matches >= limit) {
                        break;
                    }
                    continue;
                }
                if only_matching && !invert {
                    let prefix = GrepOutputPrefix {
                        label: &input.label,
                        show_filename,
                        line_number,
                    };
                    for matched_text in matcher.match_texts(line) {
                        push_grep_line(
                            &mut stdout,
                            prefix,
                            Some(line_index + 1),
                            &matched_text,
                            ':',
                        );
                    }
                    if max_count.is_some_and(|limit| file_matches >= limit) {
                        break;
                    }
                    continue;
                }
                if show_filename && !input.label.is_empty() {
                    stdout.push_str(&input.label);
                    stdout.push(':');
                }
                if line_number {
                    stdout.push_str(&(line_index + 1).to_string());
                    stdout.push(':');
                }
                stdout.push_str(line);
                stdout.push('\n');
                if max_count.is_some_and(|limit| file_matches >= limit) {
                    break;
                }
            }
        }
        if files_with_matches && file_matched {
            stdout.push_str(&input.label);
            stdout.push('\n');
        } else if count_only {
            if show_filename && !input.label.is_empty() {
                stdout.push_str(&input.label);
                stdout.push(':');
            }
            stdout.push_str(&format!("{file_matches}\n"));
        } else if (before_context > 0 || after_context > 0) && file_matched {
            push_grep_context(
                &mut stdout,
                GrepOutputPrefix {
                    label: &input.label,
                    show_filename,
                    line_number,
                },
                &lines,
                &matched_indexes,
                before_context,
                after_context,
            );
        }
    }
    CommandResult {
        exit_code: if matches == 0 { 1 } else { 0 },
        stdout,
        ..CommandResult::default()
    }
}

#[derive(Clone, Copy, Debug)]
struct GrepOutputPrefix<'a> {
    label: &'a str,
    show_filename: bool,
    line_number: bool,
}

fn push_grep_line(
    stdout: &mut String,
    prefix: GrepOutputPrefix<'_>,
    line_number_value: Option<usize>,
    text: &str,
    separator: char,
) {
    if prefix.show_filename && !prefix.label.is_empty() {
        stdout.push_str(prefix.label);
        stdout.push(separator);
    }
    if prefix.line_number
        && let Some(line_number_value) = line_number_value
    {
        stdout.push_str(&line_number_value.to_string());
        stdout.push(separator);
    }
    stdout.push_str(text);
    stdout.push('\n');
}

fn push_grep_context(
    stdout: &mut String,
    prefix: GrepOutputPrefix<'_>,
    lines: &[&str],
    matched_indexes: &[usize],
    before_context: usize,
    after_context: usize,
) {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for index in matched_indexes {
        let start = index.saturating_sub(before_context);
        let end = (*index + after_context).min(lines.len().saturating_sub(1));
        if let Some((_, previous_end)) = ranges.last_mut()
            && start <= *previous_end + 1
        {
            *previous_end = (*previous_end).max(end);
            continue;
        }
        ranges.push((start, end));
    }

    let mut first_range = true;
    for (start, end) in ranges {
        if !first_range {
            stdout.push_str("--\n");
        }
        first_range = false;
        for (index, line) in lines.iter().enumerate().take(end + 1).skip(start) {
            let separator = if matched_indexes.contains(&index) {
                ':'
            } else {
                '-'
            };
            push_grep_line(stdout, prefix, Some(index + 1), line, separator);
        }
    }
}

fn command_wc(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let mut show_lines = false;
    let mut show_words = false;
    let mut show_bytes = false;
    let mut show_chars = false;
    let mut paths = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-l" | "--lines" => show_lines = true,
            "-w" | "--words" => show_words = true,
            "-c" | "--bytes" => show_bytes = true,
            "-m" | "--chars" => show_chars = true,
            _ if arg.starts_with('-') && arg.len() > 1 => {
                for flag in arg[1..].chars() {
                    match flag {
                        'l' => show_lines = true,
                        'w' => show_words = true,
                        'c' => show_bytes = true,
                        'm' => show_chars = true,
                        _ => {}
                    }
                }
            }
            _ => paths.push(arg.clone()),
        }
    }
    if !show_lines && !show_words && !show_bytes && !show_chars {
        show_lines = true;
        show_words = true;
        show_bytes = true;
    }

    let inputs = match collect_named_text_inputs(state, &paths, stdin, "wc") {
        Ok(inputs) => inputs,
        Err(error) => return stderr_result(1, format!("wc: {error}\n")),
    };
    let mut stdout = String::new();
    let multiple_files = paths.len() > 1;
    let mut total = TextCounts::default();
    for input in &inputs {
        let counts = TextCounts::from_text(&input.text);
        total.add(counts);
        stdout.push_str(&format_wc_counts(
            counts,
            show_lines,
            show_words,
            show_bytes,
            show_chars,
            (!input.label.is_empty()).then_some(input.label.as_str()),
        ));
    }
    if multiple_files {
        stdout.push_str(&format_wc_counts(
            total,
            show_lines,
            show_words,
            show_bytes,
            show_chars,
            Some("total"),
        ));
    }
    stdout_result(stdout)
}

#[derive(Clone, Debug)]
struct NamedTextInput {
    label: String,
    text: String,
}

#[derive(Clone, Debug)]
enum LineMatcher {
    Fixed {
        pattern: String,
        ignore_case: bool,
        word_regexp: bool,
        line_regexp: bool,
    },
    Regex(Regex),
}

impl LineMatcher {
    fn new_with_line_regexp(
        pattern: &str,
        ignore_case: bool,
        word_regexp: bool,
        line_regexp: bool,
        mode: GrepMode,
    ) -> Result<Self, String> {
        if mode == GrepMode::Fixed {
            return Ok(Self::Fixed {
                pattern: pattern.to_string(),
                ignore_case,
                word_regexp,
                line_regexp,
            });
        }
        let pattern = if line_regexp {
            format!("^(?:{})$", pattern)
        } else if word_regexp {
            format!(r"\b(?:{})\b", pattern)
        } else {
            pattern.to_string()
        };
        RegexBuilder::new(&pattern)
            .case_insensitive(ignore_case)
            .build()
            .map(Self::Regex)
            .map_err(|error| error.to_string())
    }

    fn is_match(&self, line: &str) -> bool {
        match self {
            Self::Regex(regex) => regex.is_match(line),
            Self::Fixed {
                pattern,
                ignore_case,
                word_regexp,
                line_regexp,
            } => fixed_line_match(line, pattern, *ignore_case, *word_regexp, *line_regexp),
        }
    }

    fn match_texts(&self, line: &str) -> Vec<String> {
        match self {
            Self::Regex(regex) => regex
                .find_iter(line)
                .map(|matched| matched.as_str().to_string())
                .collect(),
            Self::Fixed {
                pattern,
                ignore_case,
                word_regexp,
                line_regexp,
            } => fixed_line_matches(line, pattern, *ignore_case, *word_regexp, *line_regexp),
        }
    }
}

fn fixed_line_match(
    line: &str,
    pattern: &str,
    ignore_case: bool,
    word_regexp: bool,
    line_regexp: bool,
) -> bool {
    let line = if ignore_case {
        line.to_ascii_lowercase()
    } else {
        line.to_string()
    };
    let pattern = if ignore_case {
        pattern.to_ascii_lowercase()
    } else {
        pattern.to_string()
    };
    if line_regexp {
        return line == pattern;
    }
    if !word_regexp {
        return line.contains(&pattern);
    }
    line.split(|ch: char| !is_word_char(ch))
        .any(|word| word == pattern)
}

fn fixed_line_matches(
    line: &str,
    pattern: &str,
    ignore_case: bool,
    word_regexp: bool,
    line_regexp: bool,
) -> Vec<String> {
    if !fixed_line_match(line, pattern, ignore_case, word_regexp, line_regexp) {
        return Vec::new();
    }
    if line_regexp {
        return vec![line.to_string()];
    }
    let haystack = if ignore_case {
        line.to_ascii_lowercase()
    } else {
        line.to_string()
    };
    let needle = if ignore_case {
        pattern.to_ascii_lowercase()
    } else {
        pattern.to_string()
    };
    if needle.is_empty() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    let mut offset = 0;
    while let Some(index) = haystack[offset..].find(&needle) {
        let start = offset + index;
        let end = start + needle.len();
        if !word_regexp
            || (line[..start]
                .chars()
                .next_back()
                .is_none_or(|ch| !is_word_char(ch))
                && line[end..]
                    .chars()
                    .next()
                    .is_none_or(|ch| !is_word_char(ch)))
        {
            matches.push(line[start..end].to_string());
        }
        offset = end;
    }
    matches
}

fn is_word_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn grep_inputs(
    state: &ExecState<'_>,
    paths: &[String],
    recursive: bool,
) -> Result<Vec<NamedTextInput>, String> {
    let fs = state
        .session
        .inner
        .fs
        .lock()
        .map_err(|_| "filesystem lock poisoned".to_string())?;
    let mut inputs = Vec::new();
    for path in paths {
        let path = resolve_path(&state.cwd, path);
        let stat = fs
            .stat(&path)
            .map_err(|_| format!("{path}: No such file or directory"))?;
        if stat.is_directory {
            if !recursive {
                return Err(format!("{path}: Is a directory"));
            }
            let mut child_paths = fs
                .get_all_paths()
                .into_iter()
                .filter(|candidate| candidate.starts_with(&format!("{path}/")))
                .collect::<Vec<_>>();
            child_paths.sort();
            for child_path in child_paths {
                let Ok(child_stat) = fs.stat(&child_path) else {
                    continue;
                };
                if !child_stat.is_file {
                    continue;
                }
                let text = fs
                    .read_file(&child_path)
                    .map_err(|_| format!("{child_path}: No such file or directory"))?;
                inputs.push(NamedTextInput {
                    label: child_path,
                    text,
                });
            }
        } else {
            let text = fs
                .read_file(&path)
                .map_err(|_| format!("{path}: No such file or directory"))?;
            inputs.push(NamedTextInput { label: path, text });
        }
    }
    Ok(inputs)
}

#[derive(Clone, Copy, Debug, Default)]
struct TextCounts {
    lines: usize,
    words: usize,
    bytes: usize,
    chars: usize,
}

impl TextCounts {
    fn from_text(text: &str) -> Self {
        Self {
            lines: text.bytes().filter(|byte| *byte == b'\n').count(),
            words: text.split_whitespace().count(),
            bytes: text.len(),
            chars: text.chars().count(),
        }
    }

    fn add(&mut self, other: Self) {
        self.lines += other.lines;
        self.words += other.words;
        self.bytes += other.bytes;
        self.chars += other.chars;
    }
}

fn format_wc_counts(
    counts: TextCounts,
    show_lines: bool,
    show_words: bool,
    show_bytes: bool,
    show_chars: bool,
    label: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if show_lines {
        parts.push(counts.lines.to_string());
    }
    if show_words {
        parts.push(counts.words.to_string());
    }
    if show_bytes {
        parts.push(counts.bytes.to_string());
    }
    if show_chars {
        parts.push(counts.chars.to_string());
    }
    let mut output = parts.join(" ");
    if let Some(label) = label {
        output.push(' ');
        output.push_str(label);
    }
    output.push('\n');
    output
}

fn command_ls(state: &ExecState<'_>, args: &[String]) -> CommandResult {
    let (options, mut paths) = parse_ls_args(args);
    if paths.is_empty() {
        paths.push(".".to_string());
    }
    let fs = match state.session.inner.fs.lock() {
        Ok(fs) => fs,
        Err(_) => return stderr_result(1, "ls: filesystem lock poisoned\n"),
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code = 0;
    let multi = paths.len() > 1;
    for (index, raw_path) in paths.iter().enumerate() {
        let path = resolve_path(&state.cwd, raw_path);
        match fs.stat(&path) {
            Ok(stat) if stat.is_file || (options.directories_only && stat.is_directory) => {
                stdout.push_str(&format_ls_name(raw_path, &path, &stat, &options));
                stdout.push('\n');
            }
            Ok(_) => {
                if multi || options.recursive {
                    stdout.push_str(&path);
                    stdout.push_str(":\n");
                }
                stdout.push_str(&format_ls_directory(&fs, &path, &options));
                if options.recursive {
                    stdout.push_str(&format_ls_recursive_children(&fs, &path, &options));
                }
                if multi && index + 1 < paths.len() {
                    stdout.push('\n');
                }
            }
            Err(_) => {
                stderr.push_str(&format!("ls: {path}: No such file or directory\n"));
                exit_code = 2;
            }
        }
    }
    CommandResult {
        stdout,
        stderr,
        exit_code,
        ..CommandResult::default()
    }
}

fn command_mkdir(state: &mut ExecState<'_>, args: &[String]) -> CommandResult {
    let mut recursive = false;
    let mut paths = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-p" | "--parents" => recursive = true,
            _ if arg.starts_with('-') => {}
            _ => paths.push(arg),
        }
    }
    if paths.is_empty() {
        return stderr_result(1, "mkdir: missing operand\n");
    }
    let mut fs = match state.session.inner.fs.lock() {
        Ok(fs) => fs,
        Err(_) => return stderr_result(1, "mkdir: filesystem lock poisoned\n"),
    };
    for path in paths {
        if let Err(error) = fs.mkdir(&resolve_path(&state.cwd, path), MkdirOptions { recursive }) {
            return stderr_result(1, format_mkdir_error(path, &error));
        }
    }
    CommandResult::default()
}

fn command_touch(state: &mut ExecState<'_>, args: &[String]) -> CommandResult {
    let paths = args
        .iter()
        .filter(|arg| !arg.starts_with('-'))
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return stderr_result(1, "touch: missing file operand\n");
    }
    let mut fs = match state.session.inner.fs.lock() {
        Ok(fs) => fs,
        Err(_) => return stderr_result(1, "touch: filesystem lock poisoned\n"),
    };
    for path in paths {
        if let Err(error) = fs.append_file(&resolve_path(&state.cwd, path), "") {
            return stderr_result(1, format!("touch: {error}\n"));
        }
    }
    CommandResult::default()
}

fn command_rm(state: &mut ExecState<'_>, args: &[String]) -> CommandResult {
    let mut recursive = false;
    let mut force = false;
    let mut paths = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-r" | "-R" | "--recursive" => recursive = true,
            "-f" | "--force" => force = true,
            _ if arg.starts_with('-') && arg.len() > 1 => {
                for flag in arg[1..].chars() {
                    match flag {
                        'r' | 'R' => recursive = true,
                        'f' => force = true,
                        _ => {}
                    }
                }
            }
            _ => paths.push(arg),
        }
    }
    if paths.is_empty() {
        return if force {
            CommandResult::default()
        } else {
            stderr_result(1, "rm: missing operand\n")
        };
    }
    let mut fs = match state.session.inner.fs.lock() {
        Ok(fs) => fs,
        Err(_) => return stderr_result(1, "rm: filesystem lock poisoned\n"),
    };
    for path in paths {
        let resolved = resolve_path(&state.cwd, path);
        match fs.stat(&resolved) {
            Ok(stat) if stat.is_directory && !recursive => {
                return stderr_result(1, format!("rm: cannot remove '{path}': Is a directory\n"));
            }
            Ok(_) => {}
            Err(_) if force => continue,
            Err(_) => {
                return stderr_result(
                    1,
                    format!("rm: cannot remove '{path}': No such file or directory\n"),
                );
            }
        }
        if let Err(error) = fs.rm(&resolved, RmOptions { recursive, force }) {
            return stderr_result(1, format!("rm: {error}\n"));
        }
    }
    CommandResult::default()
}

fn command_cp(state: &mut ExecState<'_>, args: &[String]) -> CommandResult {
    let mut recursive = false;
    let paths = args
        .iter()
        .filter(|arg| match arg.as_str() {
            "-r" | "-R" | "--recursive" => {
                recursive = true;
                false
            }
            _ if arg.starts_with('-') => false,
            _ => true,
        })
        .collect::<Vec<_>>();
    if paths.len() < 2 {
        return stderr_result(1, "cp: missing destination file operand\n");
    }
    let dest_arg = paths[paths.len() - 1];
    let dest = resolve_path(&state.cwd, dest_arg);
    let sources = &paths[..paths.len() - 1];
    let mut fs = match state.session.inner.fs.lock() {
        Ok(fs) => fs,
        Err(_) => return stderr_result(1, "cp: filesystem lock poisoned\n"),
    };
    let dest_is_dir = fs.stat(&dest).is_ok_and(|stat| stat.is_directory);
    if sources.len() > 1 && !dest_is_dir {
        return stderr_result(1, format!("cp: target '{dest_arg}' is not a directory\n"));
    }
    if dest_arg.ends_with('/') && !dest_is_dir {
        return stderr_result(
            1,
            format!("cp: cannot create regular file '{dest_arg}': Not a directory\n"),
        );
    }

    for src_arg in sources {
        let src = resolve_path(&state.cwd, src_arg);
        let stat = match fs.stat(&src) {
            Ok(stat) => stat,
            Err(_) => {
                return stderr_result(
                    1,
                    format!("cp: cannot stat '{src_arg}': No such file or directory\n"),
                );
            }
        };
        if stat.is_directory && !recursive {
            return stderr_result(
                1,
                format!("cp: -r not specified; omitting directory '{src_arg}'\n"),
            );
        }
        let target = if dest_is_dir {
            join_directory_child(&dest, path_basename(&src))
        } else {
            dest.clone()
        };
        if let Err(error) = fs.cp(&src, &target, CpOptions { recursive }) {
            return stderr_result(1, format!("cp: {error}\n"));
        }
    }
    CommandResult::default()
}

fn command_mv(state: &mut ExecState<'_>, args: &[String]) -> CommandResult {
    if args.first().is_some_and(|arg| arg == "--help") {
        return stdout_result(
            "Usage: mv [OPTION]... SOURCE DEST\n  -f, --force\n  -n, --no-clobber\n  -v, --verbose\n",
        );
    }
    let mut no_clobber = false;
    let mut verbose = false;
    let mut paths = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-f" | "--force" => {}
            "-n" | "--no-clobber" => no_clobber = true,
            "-v" | "--verbose" => verbose = true,
            _ if arg.starts_with('-') && arg.len() > 1 => {
                for flag in arg[1..].chars() {
                    match flag {
                        'f' => {}
                        'n' => no_clobber = true,
                        'v' => verbose = true,
                        _ => {
                            return stderr_result(1, format!("mv: invalid option -- '{flag}'\n"));
                        }
                    }
                }
            }
            _ => paths.push(arg),
        }
    }
    if paths.len() < 2 {
        return stderr_result(1, "mv: missing destination file operand\n");
    }
    let dest_arg = paths[paths.len() - 1];
    let dest = resolve_path(&state.cwd, dest_arg);
    let sources = &paths[..paths.len() - 1];
    let mut fs = match state.session.inner.fs.lock() {
        Ok(fs) => fs,
        Err(_) => return stderr_result(1, "mv: filesystem lock poisoned\n"),
    };
    let dest_is_dir = fs.stat(&dest).is_ok_and(|stat| stat.is_directory);
    if sources.len() > 1 && !dest_is_dir {
        return stderr_result(1, format!("mv: target '{dest_arg}' is not a directory\n"));
    }
    if dest_arg.ends_with('/') && !dest_is_dir {
        return stderr_result(1, format!("mv: target '{dest_arg}' is not a directory\n"));
    }

    let mut stdout = String::new();
    for src_arg in sources {
        let src = resolve_path(&state.cwd, src_arg);
        if fs.stat(&src).is_err() {
            return stderr_result(
                1,
                format!("mv: cannot stat '{src_arg}': No such file or directory\n"),
            );
        }
        let target = if dest_is_dir {
            join_directory_child(&dest, path_basename(&src))
        } else {
            dest.clone()
        };
        if no_clobber && fs.exists(&target) {
            continue;
        }
        if let Err(error) = fs.mv(&src, &target) {
            return stderr_result(1, format!("mv: {error}\n"));
        }
        if verbose {
            stdout.push_str(&format!(
                "renamed '{src_arg}' -> '{}'\n",
                display_mv_dest(dest_arg, &target, dest_is_dir)
            ));
        }
    }
    stdout_result(stdout)
}

fn number_text(text: &str, start: usize) -> (String, usize) {
    let mut output = String::new();
    let mut line_number = start;
    for line in text.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        output.push_str(&format!("{line_number:>6}\t{content}"));
        if line.ends_with('\n') {
            output.push('\n');
        }
        line_number += 1;
    }
    (output, line_number)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct LsOptions {
    all: bool,
    almost_all: bool,
    recursive: bool,
    classify: bool,
    reverse: bool,
    directories_only: bool,
}

fn parse_ls_args(args: &[String]) -> (LsOptions, Vec<String>) {
    let mut options = LsOptions::default();
    let mut paths = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--all" => options.all = true,
            _ if arg.starts_with('-') && arg.len() > 1 => {
                for flag in arg[1..].chars() {
                    match flag {
                        'a' => options.all = true,
                        'A' => options.almost_all = true,
                        'R' => options.recursive = true,
                        'F' => options.classify = true,
                        'r' => options.reverse = true,
                        'd' => options.directories_only = true,
                        '1' | 'l' => {}
                        _ => {}
                    }
                }
            }
            _ => paths.push(arg.clone()),
        }
    }
    (options, paths)
}

fn format_ls_directory(fs: &VirtualFileSystem, path: &str, options: &LsOptions) -> String {
    let mut entries = match fs.readdir_with_file_types(path) {
        Ok(entries) => entries,
        Err(_) => return String::new(),
    };
    entries.retain(|entry| options.all || options.almost_all || !entry.name.starts_with('.'));
    if options.all {
        entries.insert(
            0,
            DirentEntry {
                name: "..".to_string(),
                is_file: false,
                is_directory: true,
                is_symbolic_link: false,
            },
        );
        entries.insert(
            0,
            DirentEntry {
                name: ".".to_string(),
                is_file: false,
                is_directory: true,
                is_symbolic_link: false,
            },
        );
    }
    if options.reverse {
        entries.reverse();
    }
    let mut stdout = entries
        .into_iter()
        .map(|entry| format_ls_entry_name(&entry, options))
        .collect::<Vec<_>>()
        .join("\n");
    if !stdout.is_empty() {
        stdout.push('\n');
    }
    stdout
}

fn format_ls_recursive_children(fs: &VirtualFileSystem, path: &str, options: &LsOptions) -> String {
    let mut entries = match fs.readdir_with_file_types(path) {
        Ok(entries) => entries,
        Err(_) => return String::new(),
    };
    entries.retain(|entry| {
        entry.is_directory
            && (options.all || options.almost_all || !entry.name.starts_with('.'))
            && entry.name != "."
            && entry.name != ".."
    });
    if options.reverse {
        entries.reverse();
    }
    let mut stdout = String::new();
    for entry in entries {
        let child = join_directory_child(path, &entry.name);
        stdout.push('\n');
        stdout.push_str(&child);
        stdout.push_str(":\n");
        stdout.push_str(&format_ls_directory(fs, &child, options));
        stdout.push_str(&format_ls_recursive_children(fs, &child, options));
    }
    stdout
}

fn format_ls_entry_name(entry: &DirentEntry, options: &LsOptions) -> String {
    if options.classify && entry.is_directory {
        format!("{}/", entry.name)
    } else if options.classify && entry.is_symbolic_link {
        format!("{}@", entry.name)
    } else {
        entry.name.clone()
    }
}

fn format_ls_name(raw_path: &str, path: &str, stat: &FileStat, options: &LsOptions) -> String {
    let mut name = if raw_path == "." { path } else { raw_path }.to_string();
    if options.classify && stat.is_directory {
        name.push('/');
    }
    name
}

fn format_mkdir_error(path: &str, error: &JustBashError) -> String {
    let detail = match error.kind() {
        JustBashErrorKind::NotFound => "No such file or directory",
        JustBashErrorKind::AlreadyExists => "File exists",
        JustBashErrorKind::NotDirectory => "Not a directory",
        JustBashErrorKind::PermissionDenied => "Permission denied",
        _ => "Cannot create directory",
    };
    format!("mkdir: cannot create directory '{path}': {detail}\n")
}

fn path_basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/"
    } else {
        trimmed.rsplit('/').next().unwrap_or(trimmed)
    }
}

fn join_directory_child(directory: &str, child: &str) -> String {
    if directory == "/" {
        format!("/{child}")
    } else {
        format!("{}/{child}", directory.trim_end_matches('/'))
    }
}

fn display_mv_dest(dest_arg: &str, target: &str, dest_is_dir: bool) -> String {
    if dest_is_dir {
        target.to_string()
    } else {
        dest_arg.to_string()
    }
}

fn command_find(state: &mut ExecState<'_>, args: &[String]) -> CommandResult {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return stdout_result(
            "Usage: find [path...] [expression]\n  -name PATTERN\n  -type f|d\n  -maxdepth N\n  -mindepth N\n  -print\n  -print0\n  -printf FORMAT\n  -delete\n  -exec CMD {} ;\n",
        );
    }
    let query = match parse_find_query(args) {
        Ok(query) => query,
        Err(result) => return result,
    };
    let has_explicit_action = find_expr_has_action(&query.expression);
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code = 0;
    let mut matched_entries = Vec::new();
    {
        let fs = match state.session.inner.fs.lock() {
            Ok(fs) => fs,
            Err(_) => return stderr_result(1, "find: filesystem lock poisoned\n"),
        };
        for (root_index, root) in query.roots.iter().enumerate() {
            let absolute_root = resolve_path(&state.cwd, root);
            if fs.stat(&absolute_root).is_err() {
                stderr.push_str(&format!("find: {root}: No such file or directory\n"));
                exit_code = 1;
                continue;
            }
            let mut root_entries = fs
                .get_all_paths()
                .into_iter()
                .filter(|path| {
                    path == &absolute_root || path.starts_with(&format!("{absolute_root}/"))
                })
                .filter_map(|path| {
                    let stat = fs.stat(&path).ok()?;
                    let depth = find_depth(&absolute_root, &path);
                    if query.options.max_depth.is_some_and(|max| depth > max)
                        || depth < query.options.min_depth
                    {
                        return None;
                    }
                    Some(FindEntry {
                        path: path.clone(),
                        display_path: display_find_path(root, &absolute_root, &path),
                        root_path: absolute_root.clone(),
                        root_display: normalize_find_root_display(root),
                        stat,
                        depth,
                        root_index,
                    })
                })
                .collect::<Vec<_>>();
            if query.options.depth_first {
                root_entries.sort_by(|left, right| {
                    right
                        .depth
                        .cmp(&left.depth)
                        .then_with(|| left.path.cmp(&right.path))
                });
            } else {
                root_entries.sort_by(|left, right| left.path.cmp(&right.path));
            }

            let mut pruned_prefixes = Vec::<String>::new();
            for entry in root_entries {
                if pruned_prefixes
                    .iter()
                    .any(|prefix| entry.path.starts_with(&format!("{prefix}/")))
                {
                    continue;
                }
                let evaluation = evaluate_find_expr(&query.expression, &entry, &fs);
                if evaluation.prune && entry.stat.is_directory {
                    pruned_prefixes.push(entry.path.clone());
                }
                if !evaluation.matched {
                    continue;
                }
                matched_entries.push((entry.clone(), evaluation.actions));
            }
            if root_index + 1 < query.roots.len() {
                matched_entries.sort_by(|left, right| {
                    left.0
                        .root_index
                        .cmp(&right.0.root_index)
                        .then_with(|| left.0.path.cmp(&right.0.path))
                });
            }
        }
    }

    let mut batch_execs = Vec::<(Vec<String>, Vec<String>)>::new();
    for (entry, actions) in matched_entries {
        if has_explicit_action {
            for action in actions {
                if let FindAction::Exec {
                    command,
                    batch_mode: true,
                } = &action
                {
                    if let Some((_, paths)) = batch_execs
                        .iter_mut()
                        .find(|(candidate, _)| candidate == command)
                    {
                        paths.push(entry.display_path.clone());
                    } else {
                        batch_execs.push((command.clone(), vec![entry.display_path.clone()]));
                    }
                    continue;
                }
                let result = run_find_action(state, &entry, &action);
                stdout.push_str(&result.stdout);
                stderr.push_str(&result.stderr);
                if result.exit_code != 0 {
                    exit_code = result.exit_code;
                }
            }
        } else {
            stdout.push_str(&entry.display_path);
            stdout.push('\n');
        }
    }
    for (command, paths) in batch_execs {
        let result = run_find_batch_exec(state, &command, &paths);
        stdout.push_str(&result.stdout);
        stderr.push_str(&result.stderr);
        if result.exit_code != 0 {
            exit_code = result.exit_code;
        }
    }

    CommandResult {
        stdout,
        stderr,
        exit_code,
        exit_requested: false,
    }
}

#[derive(Clone, Debug)]
struct FindQuery {
    roots: Vec<String>,
    expression: FindExpr,
    options: FindOptions,
}

#[derive(Clone, Debug, Default)]
struct FindOptions {
    max_depth: Option<usize>,
    min_depth: usize,
    depth_first: bool,
}

#[derive(Clone, Debug)]
struct FindEntry {
    path: String,
    display_path: String,
    root_path: String,
    root_display: String,
    stat: FileStat,
    depth: usize,
    root_index: usize,
}

#[derive(Clone, Debug)]
enum FindExpr {
    True,
    Name { pattern: String, ignore_case: bool },
    Path { pattern: String, ignore_case: bool },
    Regex { pattern: String, ignore_case: bool },
    Type(char),
    Empty,
    Size(FindComparison),
    Perm { mode: u32, kind: FindPermKind },
    Mtime(FindComparison),
    Newer(String),
    Prune,
    Action(FindAction),
    Not(Box<FindExpr>),
    And(Box<FindExpr>, Box<FindExpr>),
    Or(Box<FindExpr>, Box<FindExpr>),
}

#[derive(Clone, Debug)]
struct FindComparison {
    value: i64,
    unit: FindSizeUnit,
    ordering: FindOrdering,
}

#[derive(Clone, Copy, Debug)]
enum FindSizeUnit {
    Blocks,
    Bytes,
    Kilobytes,
    Megabytes,
    Gigabytes,
}

#[derive(Clone, Copy, Debug)]
enum FindOrdering {
    Exact,
    More,
    Less,
}

#[derive(Clone, Copy, Debug)]
enum FindPermKind {
    Exact,
    All,
    Any,
}

#[derive(Clone, Debug)]
enum FindAction {
    Print,
    Print0,
    Printf(String),
    Delete,
    Exec {
        command: Vec<String>,
        batch_mode: bool,
    },
}

#[derive(Clone, Debug, Default)]
struct FindEval {
    matched: bool,
    prune: bool,
    actions: Vec<FindAction>,
}

fn parse_find_query(args: &[String]) -> Result<FindQuery, CommandResult> {
    let mut roots = Vec::new();
    let mut expression_args = Vec::new();
    let mut expressions_started = false;
    for arg in args {
        if !expressions_started && !is_find_expression_start(arg) {
            roots.push(arg.clone());
            continue;
        }
        expressions_started = true;
        expression_args.push(arg.clone());
    }
    if roots.is_empty() {
        roots.push(".".to_string());
    }

    let mut parser = FindParser {
        args: expression_args,
        pos: 0,
        options: FindOptions::default(),
    };
    let expression = if parser.args.is_empty() {
        FindExpr::True
    } else {
        parser.parse_or()?
    };
    Ok(FindQuery {
        roots,
        expression,
        options: parser.options,
    })
}

fn is_find_expression_start(arg: &str) -> bool {
    arg.starts_with('-') || matches!(arg, "!" | "(" | ")" | "\\(" | "\\)")
}

struct FindParser {
    args: Vec<String>,
    pos: usize,
    options: FindOptions,
}

impl FindParser {
    fn parse_or(&mut self) -> Result<FindExpr, CommandResult> {
        let mut left = self.parse_and()?;
        while self.match_any(&["-o", "-or"]) {
            let right = self.parse_and()?;
            left = FindExpr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<FindExpr, CommandResult> {
        let mut left = self.parse_not()?;
        loop {
            if self.match_any(&["-a", "-and"]) || self.next_starts_primary() {
                let right = self.parse_not()?;
                left = FindExpr::And(Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<FindExpr, CommandResult> {
        if self.match_any(&["-not", "!"]) {
            return Ok(FindExpr::Not(Box::new(self.parse_not()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<FindExpr, CommandResult> {
        if self.pos >= self.args.len() {
            return Ok(FindExpr::True);
        }
        if self.match_any(&["(", "\\("]) {
            let expression = self.parse_or()?;
            self.match_any(&[")", "\\)"]);
            return Ok(expression);
        }
        let arg = self.take().unwrap_or_default();
        match arg.as_str() {
            "-name" => Ok(FindExpr::Name {
                pattern: self.take_value("-name")?,
                ignore_case: false,
            }),
            "-iname" => Ok(FindExpr::Name {
                pattern: self.take_value("-iname")?,
                ignore_case: true,
            }),
            "-path" => Ok(FindExpr::Path {
                pattern: self.take_value("-path")?,
                ignore_case: false,
            }),
            "-ipath" => Ok(FindExpr::Path {
                pattern: self.take_value("-ipath")?,
                ignore_case: true,
            }),
            "-regex" => Ok(FindExpr::Regex {
                pattern: self.take_value("-regex")?,
                ignore_case: false,
            }),
            "-iregex" => Ok(FindExpr::Regex {
                pattern: self.take_value("-iregex")?,
                ignore_case: true,
            }),
            "-type" => {
                let value = self.take_value("-type")?;
                match value.as_str() {
                    "f" => Ok(FindExpr::Type('f')),
                    "d" => Ok(FindExpr::Type('d')),
                    _ => Err(stderr_result(
                        1,
                        format!("find: Unknown argument to -type: {value}\n"),
                    )),
                }
            }
            "-empty" => Ok(FindExpr::Empty),
            "-size" => Ok(FindExpr::Size(parse_find_size(&self.take_value("-size")?))),
            "-perm" => parse_find_perm(&self.take_value("-perm")?),
            "-mtime" => Ok(FindExpr::Mtime(parse_find_mtime(
                &self.take_value("-mtime")?,
            ))),
            "-newer" => Ok(FindExpr::Newer(self.take_value("-newer")?)),
            "-maxdepth" => {
                self.options.max_depth = self.take_value("-maxdepth")?.parse().ok();
                Ok(FindExpr::True)
            }
            "-mindepth" => {
                self.options.min_depth = self.take_value("-mindepth")?.parse().unwrap_or(0);
                Ok(FindExpr::True)
            }
            "-depth" => {
                self.options.depth_first = true;
                Ok(FindExpr::True)
            }
            "-prune" => Ok(FindExpr::Prune),
            "-print" => Ok(FindExpr::Action(FindAction::Print)),
            "-print0" => Ok(FindExpr::Action(FindAction::Print0)),
            "-printf" => Ok(FindExpr::Action(FindAction::Printf(
                self.take_value("-printf")?,
            ))),
            "-delete" => {
                self.options.depth_first = true;
                Ok(FindExpr::Action(FindAction::Delete))
            }
            "-exec" => self.parse_exec_action(),
            ")" | "\\)" => Ok(FindExpr::True),
            other if other.starts_with('-') => Err(stderr_result(
                1,
                format!("find: unknown predicate '{other}'\n"),
            )),
            _ => Ok(FindExpr::True),
        }
    }

    fn parse_exec_action(&mut self) -> Result<FindExpr, CommandResult> {
        let mut command = Vec::new();
        while self.pos < self.args.len() {
            let value = self.take().unwrap_or_default();
            if value == ";" || value == "+" {
                return Ok(FindExpr::Action(FindAction::Exec {
                    command,
                    batch_mode: value == "+",
                }));
            }
            command.push(value);
        }
        Err(stderr_result(1, "find: missing argument to `-exec'\n"))
    }

    fn take_value(&mut self, predicate: &str) -> Result<String, CommandResult> {
        self.take()
            .ok_or_else(|| stderr_result(1, format!("find: missing argument to `{predicate}'\n")))
    }

    fn take(&mut self) -> Option<String> {
        let value = self.args.get(self.pos).cloned()?;
        self.pos += 1;
        Some(value)
    }

    fn match_any(&mut self, values: &[&str]) -> bool {
        if self
            .args
            .get(self.pos)
            .is_some_and(|arg| values.contains(&arg.as_str()))
        {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn next_starts_primary(&self) -> bool {
        self.args
            .get(self.pos)
            .is_some_and(|arg| !matches!(arg.as_str(), "-o" | "-or" | ")" | "\\)"))
    }
}

fn parse_find_size(value: &str) -> FindComparison {
    let (ordering, rest) = parse_find_ordering(value);
    let (number, unit) = match rest.chars().last() {
        Some('c') => (&rest[..rest.len() - 1], FindSizeUnit::Bytes),
        Some('k') => (&rest[..rest.len() - 1], FindSizeUnit::Kilobytes),
        Some('M') => (&rest[..rest.len() - 1], FindSizeUnit::Megabytes),
        Some('G') => (&rest[..rest.len() - 1], FindSizeUnit::Gigabytes),
        Some('b') => (&rest[..rest.len() - 1], FindSizeUnit::Blocks),
        _ => (rest, FindSizeUnit::Blocks),
    };
    FindComparison {
        value: number.parse().unwrap_or(0),
        unit,
        ordering,
    }
}

fn parse_find_mtime(value: &str) -> FindComparison {
    let (ordering, rest) = parse_find_ordering(value);
    FindComparison {
        value: rest.parse().unwrap_or(0),
        unit: FindSizeUnit::Bytes,
        ordering,
    }
}

fn parse_find_ordering(value: &str) -> (FindOrdering, &str) {
    if let Some(rest) = value.strip_prefix('+') {
        (FindOrdering::More, rest)
    } else if let Some(rest) = value.strip_prefix('-') {
        (FindOrdering::Less, rest)
    } else {
        (FindOrdering::Exact, value)
    }
}

fn parse_find_perm(value: &str) -> Result<FindExpr, CommandResult> {
    let (kind, rest) = if let Some(rest) = value.strip_prefix('-') {
        (FindPermKind::All, rest)
    } else if let Some(rest) = value.strip_prefix('/') {
        (FindPermKind::Any, rest)
    } else {
        (FindPermKind::Exact, value)
    };
    let mode = u32::from_str_radix(rest, 8)
        .map_err(|_| stderr_result(1, format!("find: invalid mode `{value}'\n")))?;
    Ok(FindExpr::Perm { mode, kind })
}

fn evaluate_find_expr(expr: &FindExpr, entry: &FindEntry, fs: &VirtualFileSystem) -> FindEval {
    match expr {
        FindExpr::True => FindEval {
            matched: true,
            ..FindEval::default()
        },
        FindExpr::Name {
            pattern,
            ignore_case,
        } => {
            let name = entry
                .display_path
                .rsplit('/')
                .next()
                .unwrap_or(&entry.display_path);
            FindEval {
                matched: find_pattern_match(pattern, name, *ignore_case),
                ..FindEval::default()
            }
        }
        FindExpr::Path {
            pattern,
            ignore_case,
        } => FindEval {
            matched: find_pattern_match(pattern, &entry.display_path, *ignore_case)
                || find_pattern_match(pattern, &entry.path, *ignore_case),
            ..FindEval::default()
        },
        FindExpr::Regex {
            pattern,
            ignore_case,
        } => {
            let matched = RegexBuilder::new(pattern)
                .case_insensitive(*ignore_case)
                .build()
                .is_ok_and(|regex| {
                    regex.is_match(&entry.display_path) || regex.is_match(&entry.path)
                });
            FindEval {
                matched,
                ..FindEval::default()
            }
        }
        FindExpr::Type(file_type) => FindEval {
            matched: match file_type {
                'f' => entry.stat.is_file,
                'd' => entry.stat.is_directory,
                _ => false,
            },
            ..FindEval::default()
        },
        FindExpr::Empty => FindEval {
            matched: if entry.stat.is_file {
                entry.stat.size == 0
            } else if entry.stat.is_directory {
                fs.readdir(&entry.path)
                    .is_ok_and(|children| children.is_empty())
            } else {
                false
            },
            ..FindEval::default()
        },
        FindExpr::Size(comparison) => FindEval {
            matched: compare_find_value(
                entry.stat.size as i64,
                find_size_bytes(comparison),
                comparison.ordering,
            ),
            ..FindEval::default()
        },
        FindExpr::Perm { mode, kind } => {
            let actual = entry.stat.mode & 0o777;
            FindEval {
                matched: match kind {
                    FindPermKind::Exact => actual == *mode,
                    FindPermKind::All => actual & *mode == *mode,
                    FindPermKind::Any => actual & *mode != 0,
                },
                ..FindEval::default()
            }
        }
        FindExpr::Mtime(comparison) => {
            let age_days = 0_i64.saturating_sub(entry.stat.mtime as i64 / 86_400);
            FindEval {
                matched: compare_find_value(age_days, comparison.value, comparison.ordering),
                ..FindEval::default()
            }
        }
        FindExpr::Newer(path) => {
            let resolved = resolve_path(&entry.root_path, path);
            let matched = fs
                .stat(&resolved)
                .is_ok_and(|reference| entry.stat.mtime > reference.mtime);
            FindEval {
                matched,
                ..FindEval::default()
            }
        }
        FindExpr::Prune => FindEval {
            matched: true,
            prune: true,
            actions: Vec::new(),
        },
        FindExpr::Action(action) => FindEval {
            matched: true,
            prune: false,
            actions: vec![action.clone()],
        },
        FindExpr::Not(inner) => {
            let evaluation = evaluate_find_expr(inner, entry, fs);
            FindEval {
                matched: !evaluation.matched,
                prune: false,
                actions: Vec::new(),
            }
        }
        FindExpr::And(left, right) => {
            let mut left = evaluate_find_expr(left, entry, fs);
            if !left.matched {
                return FindEval {
                    matched: false,
                    prune: left.prune,
                    actions: left.actions,
                };
            }
            let right = evaluate_find_expr(right, entry, fs);
            left.matched = right.matched;
            left.prune |= right.prune;
            left.actions.extend(right.actions);
            left
        }
        FindExpr::Or(left, right) => {
            let left = evaluate_find_expr(left, entry, fs);
            if left.matched {
                left
            } else {
                let mut right = evaluate_find_expr(right, entry, fs);
                right.prune |= left.prune;
                right
            }
        }
    }
}

fn compare_find_value(actual: i64, expected: i64, ordering: FindOrdering) -> bool {
    match ordering {
        FindOrdering::Exact => actual == expected,
        FindOrdering::More => actual > expected,
        FindOrdering::Less => actual < expected,
    }
}

fn find_size_bytes(comparison: &FindComparison) -> i64 {
    let multiplier = match comparison.unit {
        FindSizeUnit::Blocks => 512,
        FindSizeUnit::Bytes => 1,
        FindSizeUnit::Kilobytes => 1024,
        FindSizeUnit::Megabytes => 1024 * 1024,
        FindSizeUnit::Gigabytes => 1024 * 1024 * 1024,
    };
    comparison.value.saturating_mul(multiplier)
}

fn find_pattern_match(pattern: &str, value: &str, ignore_case: bool) -> bool {
    if ignore_case {
        wildcard_match(&pattern.to_ascii_lowercase(), &value.to_ascii_lowercase())
    } else {
        wildcard_match(pattern, value)
    }
}

fn find_expr_has_action(expr: &FindExpr) -> bool {
    match expr {
        FindExpr::Action(_) => true,
        FindExpr::Not(inner) => find_expr_has_action(inner),
        FindExpr::And(left, right) | FindExpr::Or(left, right) => {
            find_expr_has_action(left) || find_expr_has_action(right)
        }
        _ => false,
    }
}

fn run_find_action(
    state: &mut ExecState<'_>,
    entry: &FindEntry,
    action: &FindAction,
) -> CommandResult {
    match action {
        FindAction::Print => stdout_result(format!("{}\n", entry.display_path)),
        FindAction::Print0 => stdout_result(format!("{}\0", entry.display_path)),
        FindAction::Printf(format) => stdout_result(format_find_printf(format, entry)),
        FindAction::Delete => match state.session.inner.fs.lock() {
            Ok(mut fs) => match fs.rm(
                &entry.path,
                RmOptions {
                    recursive: entry.stat.is_directory,
                    force: false,
                },
            ) {
                Ok(()) => CommandResult::default(),
                Err(error) => stderr_result(
                    1,
                    format!("find: cannot delete '{}': {error}\n", entry.display_path),
                ),
            },
            Err(_) => stderr_result(1, "find: filesystem lock poisoned\n"),
        },
        FindAction::Exec {
            command,
            batch_mode,
        } => {
            if *batch_mode {
                run_find_batch_exec(state, command, std::slice::from_ref(&entry.display_path))
            } else {
                let tokens = command
                    .iter()
                    .map(|token| {
                        if token == "{}" {
                            entry.display_path.clone()
                        } else {
                            token.replace("{}", &entry.display_path)
                        }
                    })
                    .collect::<Vec<_>>();
                execute_tokens(state, &tokens, String::new())
            }
        }
    }
}

fn run_find_batch_exec(
    state: &mut ExecState<'_>,
    command: &[String],
    paths: &[String],
) -> CommandResult {
    let mut tokens = Vec::new();
    let mut replaced = false;
    let joined_paths = paths.join(" ");
    for token in command {
        if token == "{}" {
            tokens.extend(paths.iter().cloned());
            replaced = true;
        } else if token.contains("{}") {
            tokens.push(token.replace("{}", &joined_paths));
            replaced = true;
        } else {
            tokens.push(token.clone());
        }
    }
    if !replaced {
        tokens.extend(paths.iter().cloned());
    }
    execute_tokens(state, &tokens, String::new())
}

fn format_find_printf(format: &str, entry: &FindEntry) -> String {
    let mut output = String::new();
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => output.push('\n'),
                Some('t') => output.push('\t'),
                Some('0') => output.push('\0'),
                Some('e') => output.push('\x1b'),
                Some('\\') => output.push('\\'),
                Some(other) => {
                    output.push('\\');
                    output.push(other);
                }
                None => output.push('\\'),
            }
            continue;
        }
        if ch != '%' {
            output.push(ch);
            continue;
        }

        let mut left = false;
        if chars.peek() == Some(&'-') {
            left = true;
            chars.next();
        }
        let mut width = String::new();
        while chars.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            width.push(chars.next().unwrap_or_default());
        }
        let mut precision = None;
        if chars.peek() == Some(&'.') {
            chars.next();
            let mut digits = String::new();
            while chars.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                digits.push(chars.next().unwrap_or_default());
            }
            precision = digits.parse::<usize>().ok();
        }

        let directive = chars.next().unwrap_or('%');
        let raw = match directive {
            '%' => "%".to_string(),
            'f' => path_basename(&entry.display_path).to_string(),
            'h' => find_dirname(&entry.display_path),
            'p' => entry.display_path.clone(),
            'P' => find_relative_to_root(entry),
            's' => entry.stat.size.to_string(),
            'd' => entry.depth.to_string(),
            'm' => format!("{:03o}", entry.stat.mode & 0o777),
            'M' => find_symbolic_mode(&entry.stat),
            't' => format!("mtime:{}", entry.stat.mtime),
            'T' => match chars.next() {
                Some('@') => format!("{}.0000000000", entry.stat.mtime),
                Some('Y') => "1970".to_string(),
                Some('m') => "01".to_string(),
                Some('d') => "01".to_string(),
                Some('H') => "00".to_string(),
                Some('M') => "00".to_string(),
                Some('S') => "00.0000000000".to_string(),
                Some('T') => "00:00:00".to_string(),
                Some('F') => "1970-01-01".to_string(),
                Some(other) => format!("%T{other}"),
                None => "%T".to_string(),
            },
            other => format!("%{other}"),
        };
        output.push_str(&apply_find_width(raw, width.parse().ok(), precision, left));
    }
    output
}

fn apply_find_width(
    mut value: String,
    width: Option<usize>,
    precision: Option<usize>,
    left: bool,
) -> String {
    if let Some(precision) = precision {
        value = value.chars().take(precision).collect();
    }
    let Some(width) = width else {
        return value;
    };
    let len = value.chars().count();
    if len >= width {
        return value;
    }
    let padding = " ".repeat(width - len);
    if left {
        format!("{value}{padding}")
    } else {
        format!("{padding}{value}")
    }
}

fn find_symbolic_mode(stat: &FileStat) -> String {
    let mut value = String::new();
    value.push(if stat.is_directory { 'd' } else { '-' });
    for bit in [
        0o400, 0o200, 0o100, 0o040, 0o020, 0o010, 0o004, 0o002, 0o001,
    ] {
        value.push(if stat.mode & bit != 0 {
            match bit {
                0o400 | 0o040 | 0o004 => 'r',
                0o200 | 0o020 | 0o002 => 'w',
                _ => 'x',
            }
        } else {
            '-'
        });
    }
    value
}

fn find_depth(root: &str, path: &str) -> usize {
    if root == path {
        0
    } else {
        path.strip_prefix(&format!("{root}/"))
            .unwrap_or("")
            .split('/')
            .filter(|part| !part.is_empty())
            .count()
    }
}

fn display_find_path(root_arg: &str, root: &str, path: &str) -> String {
    let normalized_root_arg = normalize_find_root_display(root_arg);
    if normalized_root_arg == "/" {
        return path.to_string();
    }
    if root_arg.starts_with('/') {
        return path.to_string();
    }
    let suffix = if path == root {
        ""
    } else {
        path.strip_prefix(root)
            .unwrap_or("")
            .trim_start_matches('/')
    };
    if suffix.is_empty() {
        normalized_root_arg
    } else if normalized_root_arg == "." {
        format!("./{suffix}")
    } else {
        format!("{}/{suffix}", normalized_root_arg.trim_end_matches('/'))
    }
}

fn normalize_find_root_display(root: &str) -> String {
    let trimmed = root.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

fn find_relative_to_root(entry: &FindEntry) -> String {
    if entry.display_path == entry.root_display {
        String::new()
    } else {
        entry
            .display_path
            .strip_prefix(&format!("{}/", entry.root_display.trim_end_matches('/')))
            .unwrap_or_else(|| {
                entry
                    .display_path
                    .strip_prefix("./")
                    .unwrap_or(&entry.display_path)
            })
            .to_string()
    }
}

fn find_dirname(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(dir, _)| if dir.is_empty() { "/" } else { dir })
        .unwrap_or(".")
        .to_string()
}

fn command_curl(state: &mut ExecState<'_>, args: &[String]) -> CommandResult {
    if args.iter().any(|arg| arg == "--help") {
        return stdout_result(
            "Usage: curl [options...] <url>\n  -X, --request METHOD\n  -H, --header HEADER\n  -d, --data DATA\n  -o, --output FILE\n  -I, --head\n  -i, --include\n  -s, --silent\n  -f, --fail\n  -w, --write-out FORMAT\n",
        );
    }
    let mut options = match parse_curl_options(args) {
        Ok(options) => options,
        Err(result) => return result,
    };
    let Some(url) = options.url.take() else {
        return stderr_result(2, "curl: no URL specified\n");
    };
    let url = normalize_curl_url(&url);
    let Some(policy) = state.session.inner.network_policy.as_ref() else {
        return curl_error(&options, 1, "curl: network access is not configured\n");
    };

    let body = match prepare_curl_body(state, &mut options) {
        Ok(body) => body,
        Err(result) => return result,
    };
    if let Some(user) = &options.user {
        let encoded = bytes_to_string(user.as_bytes(), BufferEncoding::Base64);
        options
            .headers
            .insert("authorization".to_string(), format!("Basic {encoded}"));
    }
    if let Some(cookie) = &options.cookie {
        options
            .headers
            .insert("cookie".to_string(), cookie.to_string());
    }

    let request = NetworkRequest {
        url: url.clone(),
        method: options.method,
        headers: options.headers.clone(),
        body,
        timeout_ms: options.timeout_ms,
        follow_redirects: options.follow_redirects,
    };
    let mut transport = StaticNetworkTransport::new();
    for response in state.session.inner.network_responses.values().cloned() {
        transport = transport.with_response(response);
    }
    let resolver = |_hostname: &str| Ok(vec![crate::security::DnsAddress::new("93.184.216.34", 4)]);
    let response = match execute_network_request(policy, request, &resolver, &mut transport) {
        Ok(response) => response,
        Err(error) => {
            return curl_error(&options, 1, format!("curl: {error}\n"));
        }
    };

    if options.fail_silently && response.status >= 400 {
        return curl_error(
            &options,
            22,
            format!(
                "curl: The requested URL returned error: {}\n",
                response.status
            ),
        );
    }

    if let Some(cookie_jar) = &options.cookie_jar
        && let Some(set_cookie) = response.headers.get("set-cookie")
    {
        let path = resolve_path(&state.cwd, cookie_jar);
        if let Ok(mut fs) = state.session.inner.fs.lock() {
            let _ = fs.write_file(&path, set_cookie.clone());
        }
    }

    let output = build_curl_output(&options, &response, &url);
    if let Some(path) = options
        .output_file
        .clone()
        .or_else(|| options.use_remote_name.then(|| curl_remote_name(&url)))
    {
        let path = resolve_path(&state.cwd, &path);
        return match state.session.inner.fs.lock() {
            Ok(mut fs) => match fs.write_file(&path, response.body.clone()) {
                Ok(()) => {
                    let mut result = stdout_result(apply_curl_write_out(
                        options.write_out.as_deref(),
                        "",
                        &response,
                    ));
                    result.exit_code = 0;
                    result
                }
                Err(error) => stderr_result(1, format!("curl: cannot write output: {error}\n")),
            },
            Err(_) => stderr_result(1, "curl: filesystem lock poisoned\n"),
        };
    }

    stdout_result(output)
}

#[derive(Clone, Debug)]
struct CurlOptions {
    url: Option<String>,
    method: HttpMethod,
    headers: BTreeMap<String, String>,
    data: Option<String>,
    data_file: Option<CurlDataFile>,
    data_binary: bool,
    urlencode_files: Vec<CurlUrlencodeFile>,
    form_fields: Vec<CurlFormField>,
    user: Option<String>,
    cookie: Option<String>,
    cookie_jar: Option<String>,
    upload_file: Option<String>,
    timeout_ms: Option<u64>,
    output_file: Option<String>,
    use_remote_name: bool,
    head_only: bool,
    include_headers: bool,
    silent: bool,
    show_error: bool,
    fail_silently: bool,
    follow_redirects: bool,
    verbose: bool,
    write_out: Option<String>,
}

#[derive(Clone, Debug)]
struct CurlDataFile {
    path: String,
    ascii_mode: bool,
}

#[derive(Clone, Debug)]
struct CurlUrlencodeFile {
    name: Option<String>,
    path: String,
}

#[derive(Clone, Debug)]
struct CurlFormField {
    name: String,
    value: String,
    content_type: Option<String>,
}

impl Default for CurlOptions {
    fn default() -> Self {
        Self {
            url: None,
            method: HttpMethod::Get,
            headers: BTreeMap::new(),
            data: None,
            data_file: None,
            data_binary: false,
            urlencode_files: Vec::new(),
            form_fields: Vec::new(),
            user: None,
            cookie: None,
            cookie_jar: None,
            upload_file: None,
            timeout_ms: None,
            output_file: None,
            use_remote_name: false,
            head_only: false,
            include_headers: false,
            silent: false,
            show_error: false,
            fail_silently: false,
            follow_redirects: true,
            verbose: false,
            write_out: None,
        }
    }
}

fn parse_curl_options(args: &[String]) -> Result<CurlOptions, CommandResult> {
    let mut options = CurlOptions::default();
    let mut implies_post = false;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "-X" | "--request" => {
                index += 1;
                options.method = parse_curl_method(args.get(index).map_or("GET", String::as_str))?;
            }
            "-H" | "--header" => {
                index += 1;
                if let Some(header) = args.get(index) {
                    apply_curl_header(&mut options, header);
                }
            }
            "-d" | "--data" => {
                index += 1;
                apply_curl_data(
                    &mut options,
                    args.get(index).map_or("", String::as_str),
                    false,
                    true,
                );
                implies_post = true;
            }
            "--data-raw" => {
                index += 1;
                apply_curl_data(
                    &mut options,
                    args.get(index).map_or("", String::as_str),
                    false,
                    false,
                );
                implies_post = true;
            }
            "--data-binary" => {
                index += 1;
                apply_curl_data(
                    &mut options,
                    args.get(index).map_or("", String::as_str),
                    true,
                    true,
                );
                implies_post = true;
            }
            "--data-urlencode" => {
                index += 1;
                apply_curl_urlencode(&mut options, args.get(index).map_or("", String::as_str));
                implies_post = true;
            }
            "-F" | "--form" => {
                index += 1;
                if let Some(field) = args
                    .get(index)
                    .and_then(|value| parse_curl_form_field(value))
                {
                    options.form_fields.push(field);
                }
                implies_post = true;
            }
            "-u" | "--user" => {
                index += 1;
                options.user = args.get(index).cloned();
            }
            "-A" | "--user-agent" => {
                index += 1;
                options.headers.insert(
                    "user-agent".to_string(),
                    args.get(index).cloned().unwrap_or_default(),
                );
            }
            "-e" | "--referer" => {
                index += 1;
                options.headers.insert(
                    "referer".to_string(),
                    args.get(index).cloned().unwrap_or_default(),
                );
            }
            "-b" | "--cookie" => {
                index += 1;
                options.cookie = args.get(index).cloned();
            }
            "-c" | "--cookie-jar" => {
                index += 1;
                options.cookie_jar = args.get(index).cloned();
            }
            "-T" | "--upload-file" => {
                index += 1;
                options.upload_file = args.get(index).cloned();
                if options.method == HttpMethod::Get {
                    options.method = HttpMethod::Put;
                }
            }
            "-m" | "--max-time" | "--connect-timeout" => {
                index += 1;
                options.timeout_ms = args
                    .get(index)
                    .and_then(|value| parse_curl_seconds_ms(value));
            }
            "-o" | "--output" => {
                index += 1;
                options.output_file = args.get(index).cloned();
            }
            "-O" | "--remote-name" => options.use_remote_name = true,
            "-I" | "--head" => {
                options.head_only = true;
                options.method = HttpMethod::Head;
            }
            "-i" | "--include" => options.include_headers = true,
            "-s" | "--silent" => options.silent = true,
            "-S" | "--show-error" => options.show_error = true,
            "-f" | "--fail" => options.fail_silently = true,
            "-L" | "--location" => options.follow_redirects = true,
            "-w" | "--write-out" => {
                index += 1;
                options.write_out = args.get(index).cloned();
            }
            "-v" | "--verbose" => options.verbose = true,
            "--max-redirs" => index += 1,
            "--" => {}
            _ if arg.starts_with("--request=") => {
                options.method = parse_curl_method(&arg[10..])?;
            }
            _ if arg.starts_with("--header=") => apply_curl_header(&mut options, &arg[9..]),
            _ if arg.starts_with("--data=") => {
                apply_curl_data(&mut options, &arg[7..], false, true);
                implies_post = true;
            }
            _ if arg.starts_with("--data-raw=") => {
                apply_curl_data(&mut options, &arg[11..], false, false);
                implies_post = true;
            }
            _ if arg.starts_with("--data-binary=") => {
                apply_curl_data(&mut options, &arg[14..], true, true);
                implies_post = true;
            }
            _ if arg.starts_with("--data-urlencode=") => {
                apply_curl_urlencode(&mut options, &arg[17..]);
                implies_post = true;
            }
            _ if arg.starts_with("--form=") => {
                if let Some(field) = parse_curl_form_field(&arg[7..]) {
                    options.form_fields.push(field);
                }
                implies_post = true;
            }
            _ if arg.starts_with("--user=") => options.user = Some(arg[7..].to_string()),
            _ if arg.starts_with("--user-agent=") => {
                options
                    .headers
                    .insert("user-agent".to_string(), arg[13..].to_string());
            }
            _ if arg.starts_with("--referer=") => {
                options
                    .headers
                    .insert("referer".to_string(), arg[10..].to_string());
            }
            _ if arg.starts_with("--cookie=") => options.cookie = Some(arg[9..].to_string()),
            _ if arg.starts_with("--cookie-jar=") => {
                options.cookie_jar = Some(arg[13..].to_string());
            }
            _ if arg.starts_with("--upload-file=") => {
                options.upload_file = Some(arg[14..].to_string());
                if options.method == HttpMethod::Get {
                    options.method = HttpMethod::Put;
                }
            }
            _ if arg.starts_with("--max-time=") => {
                options.timeout_ms = parse_curl_seconds_ms(&arg[11..]);
            }
            _ if arg.starts_with("--connect-timeout=") => {
                options
                    .timeout_ms
                    .get_or_insert_with(|| parse_curl_seconds_ms(&arg[18..]).unwrap_or(0));
            }
            _ if arg.starts_with("--output=") => options.output_file = Some(arg[9..].to_string()),
            _ if arg.starts_with("--write-out=") => {
                options.write_out = Some(arg[12..].to_string());
            }
            _ if arg.starts_with("-X") && arg.len() > 2 => {
                options.method = parse_curl_method(&arg[2..])?;
            }
            _ if arg.starts_with("-u") && arg.len() > 2 => {
                options.user = Some(arg[2..].to_string())
            }
            _ if arg.starts_with("-A") && arg.len() > 2 => {
                options
                    .headers
                    .insert("user-agent".to_string(), arg[2..].to_string());
            }
            _ if arg.starts_with("-e") && arg.len() > 2 => {
                options
                    .headers
                    .insert("referer".to_string(), arg[2..].to_string());
            }
            _ if arg.starts_with("-b") && arg.len() > 2 => {
                options.cookie = Some(arg[2..].to_string())
            }
            _ if arg.starts_with("-d") && arg.len() > 2 => {
                apply_curl_data(&mut options, &arg[2..], false, true);
                implies_post = true;
            }
            _ if arg.starts_with("--") => {
                return Err(stderr_result(2, format!("curl: unknown option {arg}\n")));
            }
            _ if arg.starts_with('-') && arg != "-" => {
                parse_curl_short_cluster(&mut options, arg)?;
            }
            _ => options.url = Some(arg.clone()),
        }
        index += 1;
    }
    if implies_post && options.method == HttpMethod::Get {
        options.method = HttpMethod::Post;
    }
    Ok(options)
}

fn parse_curl_method(value: &str) -> Result<HttpMethod, CommandResult> {
    value
        .parse()
        .map_err(|_| stderr_result(2, format!("curl: unsupported request method {value}\n")))
}

fn parse_curl_short_cluster(options: &mut CurlOptions, arg: &str) -> Result<(), CommandResult> {
    for flag in arg[1..].chars() {
        match flag {
            's' => options.silent = true,
            'S' => options.show_error = true,
            'f' => options.fail_silently = true,
            'L' => options.follow_redirects = true,
            'I' => {
                options.head_only = true;
                options.method = HttpMethod::Head;
            }
            'i' => options.include_headers = true,
            'O' => options.use_remote_name = true,
            'v' => options.verbose = true,
            other => return Err(stderr_result(2, format!("curl: unknown option -{other}\n"))),
        }
    }
    Ok(())
}

fn apply_curl_header(options: &mut CurlOptions, header: &str) {
    if let Some((name, value)) = header.split_once(':') {
        let key = name.trim().to_ascii_lowercase();
        if !key.is_empty() {
            options.headers.insert(key, value.trim().to_string());
        }
    }
}

fn apply_curl_data(options: &mut CurlOptions, value: &str, binary: bool, allow_file: bool) {
    if allow_file && value.starts_with('@') {
        options.data_file = Some(CurlDataFile {
            path: value.trim_start_matches('@').to_string(),
            ascii_mode: !binary,
        });
        options.data = None;
    } else {
        options.data = Some(value.to_string());
        options.data_file = None;
    }
    options.data_binary = binary;
}

fn apply_curl_urlencode(options: &mut CurlOptions, value: &str) {
    if let Some(path) = value.strip_prefix('@') {
        options.urlencode_files.push(CurlUrlencodeFile {
            name: None,
            path: path.to_string(),
        });
        return;
    }
    let at = value.find('@');
    let eq = value.find('=');
    if let Some(at) = at
        && at > 0
        && eq.is_none_or(|eq| at < eq)
    {
        options.urlencode_files.push(CurlUrlencodeFile {
            name: Some(value[..at].to_string()),
            path: value[at + 1..].to_string(),
        });
        return;
    }
    let encoded = encode_curl_form_data(value);
    options.data = Some(match options.data.take() {
        Some(existing) => format!("{existing}&{encoded}"),
        None => encoded,
    });
}

fn parse_curl_form_field(value: &str) -> Option<CurlFormField> {
    let (name, rest) = value.split_once('=')?;
    let (value, content_type) = rest
        .split_once(";type=")
        .map_or((rest, None), |(value, content_type)| {
            (value, Some(content_type))
        });
    Some(CurlFormField {
        name: name.to_string(),
        value: value.to_string(),
        content_type: content_type.map(str::to_string),
    })
}

fn parse_curl_seconds_ms(value: &str) -> Option<u64> {
    let seconds = value.parse::<f64>().ok()?;
    (seconds > 0.0).then_some((seconds * 1000.0) as u64)
}

fn prepare_curl_body(
    state: &ExecState<'_>,
    options: &mut CurlOptions,
) -> Result<Option<Vec<u8>>, CommandResult> {
    if let Some(path) = &options.upload_file {
        let path = resolve_path(&state.cwd, path);
        let fs = state
            .session
            .inner
            .fs
            .lock()
            .map_err(|_| stderr_result(1, "curl: filesystem lock poisoned\n"))?;
        return fs
            .read_file_buffer(&path)
            .map(Some)
            .map_err(|_| stderr_result(1, format!("curl: cannot read upload file {path}\n")));
    }

    if !options.form_fields.is_empty() {
        let mut body = String::new();
        let boundary = "----just-bash-rust-boundary";
        let fs = state
            .session
            .inner
            .fs
            .lock()
            .map_err(|_| stderr_result(1, "curl: filesystem lock poisoned\n"))?;
        for field in &options.form_fields {
            body.push_str(&format!("--{boundary}\r\n"));
            body.push_str(&format!(
                "Content-Disposition: form-data; name=\"{}\"",
                field.name
            ));
            let mut value = field.value.clone();
            if let Some(path) = value
                .strip_prefix('@')
                .or_else(|| value.strip_prefix('<'))
                .map(str::to_string)
            {
                let resolved = resolve_path(&state.cwd, &path);
                value = fs.read_file(&resolved).unwrap_or_default();
                body.push_str(&format!("; filename=\"{}\"", path_basename(&path)));
            }
            body.push_str("\r\n");
            if let Some(content_type) = &field.content_type {
                body.push_str(&format!("Content-Type: {content_type}\r\n"));
            }
            body.push_str("\r\n");
            body.push_str(&value);
            body.push_str("\r\n");
        }
        body.push_str(&format!("--{boundary}--\r\n"));
        options
            .headers
            .entry("content-type".to_string())
            .or_insert_with(|| format!("multipart/form-data; boundary={boundary}"));
        return Ok(Some(body.into_bytes()));
    }

    let mut data = if let Some(data_file) = &options.data_file {
        let path = resolve_path(&state.cwd, &data_file.path);
        let fs = state
            .session
            .inner
            .fs
            .lock()
            .map_err(|_| stderr_result(1, "curl: filesystem lock poisoned\n"))?;
        let mut content = fs
            .read_file(&path)
            .map_err(|_| stderr_result(1, format!("curl: cannot read data file {path}\n")))?;
        if data_file.ascii_mode {
            content.retain(|ch| ch != '\r' && ch != '\n');
        }
        Some(content)
    } else {
        options.data.clone()
    };

    if !options.urlencode_files.is_empty() {
        let fs = state
            .session
            .inner
            .fs
            .lock()
            .map_err(|_| stderr_result(1, "curl: filesystem lock poisoned\n"))?;
        let mut parts = data.take().into_iter().collect::<Vec<_>>();
        for entry in &options.urlencode_files {
            let path = resolve_path(&state.cwd, &entry.path);
            let content = fs
                .read_file(&path)
                .map_err(|_| stderr_result(1, format!("curl: cannot read data file {path}\n")))?;
            let encoded = percent_encode(&content);
            parts.push(match &entry.name {
                Some(name) => format!("{}={encoded}", percent_encode(name)),
                None => encoded,
            });
        }
        data = Some(parts.join("&"));
    }

    Ok(data.map(String::into_bytes))
}

fn build_curl_output(
    options: &CurlOptions,
    response: &NetworkResponse,
    request_url: &str,
) -> String {
    let mut output = String::new();
    if options.verbose {
        output.push_str(&format!("> {} {request_url}\n", options.method));
        for (name, value) in &options.headers {
            output.push_str(&format!("> {name}: {value}\n"));
        }
        output.push_str(">\n");
        output.push_str(&format!(
            "< HTTP/1.1 {} {}\n",
            response.status, response.status_text
        ));
        for (name, value) in &response.headers {
            output.push_str(&format!("< {name}: {value}\n"));
        }
        output.push_str("<\n");
    } else if options.include_headers || options.head_only {
        output.push_str(&format!(
            "HTTP/1.1 {} {}\r\n",
            response.status, response.status_text
        ));
        for (name, value) in &response.headers {
            output.push_str(&format!("{name}: {value}\r\n"));
        }
        output.push_str("\r\n");
    }
    if !options.head_only {
        output.push_str(&bytes_to_string(&response.body, BufferEncoding::Binary));
    }
    apply_curl_write_out(options.write_out.as_deref(), &output, response)
}

fn apply_curl_write_out(write_out: Option<&str>, base: &str, response: &NetworkResponse) -> String {
    let Some(format) = write_out else {
        return base.to_string();
    };
    let mut output = base.to_string();
    let content_type = response
        .headers
        .get("content-type")
        .cloned()
        .unwrap_or_default();
    let expanded = format
        .replace("\\n", "\n")
        .replace("%{http_code}", &format!("{:03}", response.status))
        .replace("%{response_code}", &format!("{:03}", response.status))
        .replace("%{content_type}", &content_type)
        .replace("%{url_effective}", &response.url)
        .replace("%{size_download}", &response.body.len().to_string());
    output.push_str(&expanded);
    output
}

fn curl_error(options: &CurlOptions, exit_code: i32, message: impl Into<String>) -> CommandResult {
    let stderr = if options.silent && !options.show_error {
        String::new()
    } else {
        message.into()
    };
    CommandResult {
        stdout: String::new(),
        stderr,
        exit_code,
        exit_requested: false,
    }
}

fn normalize_curl_url(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("https://{url}")
    }
}

fn curl_remote_name(url: &str) -> String {
    url.split('?')
        .next()
        .unwrap_or(url)
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("index.html")
        .to_string()
}

fn encode_curl_form_data(value: &str) -> String {
    if let Some((name, body)) = value.split_once('=') {
        format!("{}={}", percent_encode(name), percent_encode(body))
    } else if let Some(body) = value.strip_prefix('=') {
        percent_encode(body)
    } else {
        percent_encode(value)
    }
}

fn percent_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            output.push(char::from(byte));
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

#[derive(Clone, Debug)]
struct HtmlMarkdownOptions {
    bullet: String,
    code_fence: String,
    hr: String,
    heading_style: HtmlHeadingStyle,
    path: Option<String>,
}

impl Default for HtmlMarkdownOptions {
    fn default() -> Self {
        Self {
            bullet: "-".to_string(),
            code_fence: "```".to_string(),
            hr: "---".to_string(),
            heading_style: HtmlHeadingStyle::Atx,
            path: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HtmlHeadingStyle {
    Atx,
    Setext,
}

fn command_html_to_markdown(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let options = match parse_html_markdown_options(args) {
        Ok(HtmlMarkdownParse::Help) => return stdout_result(html_to_markdown_help()),
        Ok(HtmlMarkdownParse::Options(options)) => options,
        Err(result) => return result,
    };
    let input = if let Some(path) = &options.path {
        let resolved = resolve_path(&state.cwd, path);
        match state
            .session
            .inner
            .fs
            .lock()
            .map_err(|_| stderr_result(1, "html-to-markdown: filesystem lock poisoned\n"))
            .and_then(|fs| {
                fs.read_file(&resolved).map_err(|_| {
                    stderr_result(
                        1,
                        format!("html-to-markdown: {resolved}: No such file or directory\n"),
                    )
                })
            }) {
            Ok(input) => input,
            Err(result) => return result,
        }
    } else {
        stdin.to_string()
    };
    stdout_result(html_to_markdown(&input, &options))
}

enum HtmlMarkdownParse {
    Help,
    Options(HtmlMarkdownOptions),
}

fn parse_html_markdown_options(args: &[String]) -> Result<HtmlMarkdownParse, CommandResult> {
    let mut options = HtmlMarkdownOptions::default();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "--help" || arg == "-h" {
            return Ok(HtmlMarkdownParse::Help);
        }
        if let Some(value) = arg.strip_prefix("--bullet=") {
            options.bullet = value.to_string();
            index += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--heading-style=") {
            options.heading_style = match value {
                "setext" => HtmlHeadingStyle::Setext,
                "atx" => HtmlHeadingStyle::Atx,
                _ => {
                    return Err(stderr_result(
                        1,
                        format!("html-to-markdown: unsupported heading style: {value}\n"),
                    ));
                }
            };
            index += 1;
            continue;
        }
        match arg.as_str() {
            "-b" | "--bullet" | "-c" | "--code-fence" | "-r" | "--hr" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(stderr_result(
                        1,
                        format!("html-to-markdown: missing argument to {arg}\n"),
                    ));
                };
                match arg.as_str() {
                    "-b" | "--bullet" => options.bullet = value.clone(),
                    "-c" | "--code-fence" => options.code_fence = value.clone(),
                    "-r" | "--hr" => options.hr = value.clone(),
                    _ => {}
                }
                index += 2;
                continue;
            }
            _ if arg.starts_with('-') => {
                return Err(stderr_result(
                    1,
                    format!("html-to-markdown: unrecognized option: {arg}\n"),
                ));
            }
            _ => options.path = Some(arg.clone()),
        }
        index += 1;
    }
    Ok(HtmlMarkdownParse::Options(options))
}

fn html_to_markdown_help() -> String {
    [
        "html-to-markdown - convert HTML to Markdown",
        "",
        "Description:",
        "  BashEnv extension backed by deterministic turndown-style conversion.",
        "",
        "Usage: html-to-markdown [options] [FILE]",
        "",
        "Options:",
        "  -b, --bullet MARKER       Bullet marker for unordered lists",
        "  -c, --code-fence FENCE    Code fence marker",
        "  -r, --hr RULE             Horizontal rule marker",
        "      --heading-style=STYLE Heading style: atx or setext",
        "",
        "Supported HTML elements: Headings, Links, Bold, Italic, Lists, Code, Images, Blockquotes",
        "",
        "Examples:",
        "  echo '<h1>Hello</h1>' | html-to-markdown",
        "  curl https://example.com | html-to-markdown",
        "",
    ]
    .join("\n")
}

fn html_to_markdown(input: &str, options: &HtmlMarkdownOptions) -> String {
    let mut output = input.trim().to_string();
    if output.is_empty() {
        return String::new();
    }
    for pattern in [
        r"(?is)<script\b[^>]*>.*?</script>",
        r"(?is)<style\b[^>]*>.*?</style>",
    ] {
        output = Regex::new(pattern)
            .unwrap()
            .replace_all(&output, "")
            .into_owned();
    }
    output = Regex::new(r"(?is)<pre\b[^>]*>\s*<code\b[^>]*>(.*?)</code>\s*</pre>")
        .unwrap()
        .replace_all(&output, |captures: &regex::Captures<'_>| {
            format!(
                "\n{fence}\n{}\n{fence}\n",
                html_inline_text(&captures[1]),
                fence = options.code_fence
            )
        })
        .into_owned();
    output = Regex::new(r#"(?is)<img\b[^>]*src=["']([^"']+)["'][^>]*alt=["']([^"']*)["'][^>]*>"#)
        .unwrap()
        .replace_all(&output, |captures: &regex::Captures<'_>| {
            format!(
                "![{}]({})",
                html_inline_text(&captures[2]),
                captures[1].trim()
            )
        })
        .into_owned();
    output = Regex::new(r#"(?is)<a\b[^>]*href=["']([^"']+)["'][^>]*>(.*?)</a>"#)
        .unwrap()
        .replace_all(&output, |captures: &regex::Captures<'_>| {
            format!(
                "[{}]({})",
                html_inline_text(&captures[2]),
                captures[1].trim()
            )
        })
        .into_owned();
    for (pattern, replacement) in [
        (r"(?is)<(?:strong|b)\b[^>]*>(.*?)</(?:strong|b)>", "**"),
        (r"(?is)<(?:em|i)\b[^>]*>(.*?)</(?:em|i)>", "_"),
        (r"(?is)<code\b[^>]*>(.*?)</code>", "`"),
    ] {
        output = Regex::new(pattern)
            .unwrap()
            .replace_all(&output, |captures: &regex::Captures<'_>| {
                format!(
                    "{replacement}{}{replacement}",
                    html_inline_text(&captures[1])
                )
            })
            .into_owned();
    }
    for level in 1..=6 {
        let pattern = format!(r"(?is)<h{level}\b[^>]*>(.*?)</h{level}>");
        output = Regex::new(&pattern)
            .unwrap()
            .replace_all(&output, |captures: &regex::Captures<'_>| {
                let text = html_inline_text(&captures[1]);
                if level <= 2 && options.heading_style == HtmlHeadingStyle::Setext {
                    let underline = if level == 1 { "=" } else { "-" };
                    format!(
                        "\n{text}\n{}\n",
                        underline.repeat(text.chars().count().max(1))
                    )
                } else {
                    format!("\n{} {text}\n", "#".repeat(level))
                }
            })
            .into_owned();
    }
    output = convert_html_lists(&output, "ul", false, &options.bullet);
    output = convert_html_lists(&output, "ol", true, &options.bullet);
    output = Regex::new(r"(?is)<blockquote\b[^>]*>(.*?)</blockquote>")
        .unwrap()
        .replace_all(&output, |captures: &regex::Captures<'_>| {
            format!("\n> {}\n", html_inline_text(&captures[1]))
        })
        .into_owned();
    output = Regex::new(r"(?is)<p\b[^>]*>(.*?)</p>")
        .unwrap()
        .replace_all(&output, |captures: &regex::Captures<'_>| {
            format!("\n{}\n", html_inline_text(&captures[1]))
        })
        .into_owned();
    output = Regex::new(r"(?is)<hr\b[^>]*>")
        .unwrap()
        .replace_all(&output, format!("\n{}\n", options.hr))
        .into_owned();
    output = Regex::new(r"(?is)</?(?:div|section|article|main|span)\b[^>]*>")
        .unwrap()
        .replace_all(&output, "\n")
        .into_owned();
    normalize_markdown_output(&html_inline_text(&output))
}

fn convert_html_lists(input: &str, tag: &str, ordered: bool, bullet: &str) -> String {
    let pattern = format!(r"(?is)<{tag}\b[^>]*>(.*?)</{tag}>");
    Regex::new(&pattern)
        .unwrap()
        .replace_all(input, |captures: &regex::Captures<'_>| {
            let item_re = Regex::new(r"(?is)<li\b[^>]*>(.*?)</li>").unwrap();
            let mut lines = Vec::new();
            for (index, item) in item_re.captures_iter(&captures[1]).enumerate() {
                let text = html_inline_text(&item[1]);
                if ordered {
                    lines.push(format!("{}.  {text}", index + 1));
                } else {
                    lines.push(format!("{bullet}   {text}"));
                }
            }
            format!("\n{}\n", lines.join("\n"))
        })
        .into_owned()
}

fn html_inline_text(input: &str) -> String {
    decode_html_entities(
        &Regex::new(r"(?is)<[^>]+>")
            .unwrap()
            .replace_all(input, "")
            .replace('\r', ""),
    )
    .trim()
    .to_string()
}

fn normalize_markdown_output(input: &str) -> String {
    let mut output = input.replace('\r', "");
    output = Regex::new(r"[ \t]+\n")
        .unwrap()
        .replace_all(&output, "\n")
        .into_owned();
    output = Regex::new(r"\n{3,}")
        .unwrap()
        .replace_all(&output, "\n\n")
        .into_owned();
    output = output
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if output.is_empty() {
        String::new()
    } else {
        output.push('\n');
        output
    }
}

fn command_rg(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let request = match parse_rg_args(args) {
        Ok(RgParseResult::Help) => {
            return stdout_result(
                "rg recursively search the current directory for lines matching a pattern\n",
            );
        }
        Ok(RgParseResult::TypeList) => return stdout_result(rg_type_list()),
        Ok(RgParseResult::Search(request)) => *request,
        Err(result) => return result,
    };

    if request.options.files {
        return command_rg_files(state, &request);
    }

    let mut patterns = request.patterns.clone();
    if let Err(result) = rg_load_pattern_files(state, stdin, &request.pattern_files, &mut patterns)
    {
        return result;
    }
    if patterns.is_empty() {
        return stderr_result(2, "rg: no pattern given\n");
    }

    let ignore_case = request.options.ignore_case.unwrap_or_else(|| {
        patterns
            .iter()
            .all(|pattern| rg_smart_case_ignores_case(pattern))
    });
    let mode = if request.options.fixed {
        GrepMode::Fixed
    } else {
        GrepMode::Regex
    };
    let matchers = match patterns
        .iter()
        .map(|pattern| {
            LineMatcher::new_with_line_regexp(
                pattern,
                ignore_case,
                request.options.word_regexp,
                request.options.line_regexp,
                mode,
            )
        })
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(matchers) => matchers,
        Err(error) => return stderr_result(2, format!("rg: {error}\n")),
    };

    let inputs = if request.roots.is_empty() && !stdin.is_empty() {
        vec![RgInput {
            label: "<stdin>".to_string(),
            text: stdin.to_string(),
            explicit_file: false,
        }]
    } else {
        match rg_inputs(state, &request) {
            Ok(inputs) => inputs,
            Err(error) => return stderr_result(2, format!("rg: {error}\n")),
        }
    };

    rg_render_search(&request, &matchers, &inputs)
}

#[derive(Clone, Debug)]
struct RgRequest {
    options: RgOptions,
    patterns: Vec<String>,
    pattern_files: Vec<String>,
    roots: Vec<String>,
}

#[derive(Clone, Debug)]
struct RgOptions {
    line_number: Option<bool>,
    ignore_case: Option<bool>,
    fixed: bool,
    word_regexp: bool,
    line_regexp: bool,
    invert: bool,
    count: bool,
    count_matches: bool,
    files_with_matches: bool,
    files_without_match: bool,
    only_matching: bool,
    quiet: bool,
    no_filename: bool,
    hidden: bool,
    no_ignore: bool,
    text: bool,
    files: bool,
    null_separator: bool,
    include_zero: bool,
    heading: bool,
    before_context: usize,
    after_context: usize,
    context_separator: String,
    max_count: Option<usize>,
    max_depth: Option<usize>,
    globs: Vec<String>,
    type_includes: Vec<String>,
    type_excludes: Vec<String>,
}

impl Default for RgOptions {
    fn default() -> Self {
        Self {
            line_number: None,
            ignore_case: None,
            fixed: false,
            word_regexp: false,
            line_regexp: false,
            invert: false,
            count: false,
            count_matches: false,
            files_with_matches: false,
            files_without_match: false,
            only_matching: false,
            quiet: false,
            no_filename: false,
            hidden: false,
            no_ignore: false,
            text: false,
            files: false,
            null_separator: false,
            include_zero: false,
            heading: false,
            before_context: 0,
            after_context: 0,
            context_separator: "--".to_string(),
            max_count: None,
            max_depth: None,
            globs: Vec::new(),
            type_includes: Vec::new(),
            type_excludes: Vec::new(),
        }
    }
}

enum RgParseResult {
    Help,
    TypeList,
    Search(Box<RgRequest>),
}

fn parse_rg_args(args: &[String]) -> Result<RgParseResult, CommandResult> {
    let mut options = RgOptions::default();
    let mut patterns = Vec::new();
    let mut pattern_files = Vec::new();
    let mut roots = Vec::new();
    let mut parsing_roots = false;
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if !parsing_roots && arg == "--" {
            parsing_roots = true;
            index += 1;
            continue;
        }
        if !parsing_roots && arg.starts_with('-') && arg != "-" {
            if let Some(action) = parse_rg_option(
                args,
                &mut index,
                &mut options,
                &mut patterns,
                &mut pattern_files,
            )? {
                return Ok(action);
            }
            continue;
        }
        if options.files || !patterns.is_empty() || !pattern_files.is_empty() {
            roots.push(arg.clone());
        } else {
            patterns.push(arg.clone());
        }
        parsing_roots = true;
        index += 1;
    }

    Ok(RgParseResult::Search(Box::new(RgRequest {
        options,
        patterns,
        pattern_files,
        roots,
    })))
}

fn parse_rg_option(
    args: &[String],
    index: &mut usize,
    options: &mut RgOptions,
    patterns: &mut Vec<String>,
    pattern_files: &mut Vec<String>,
) -> Result<Option<RgParseResult>, CommandResult> {
    let arg = &args[*index];
    match arg.as_str() {
        "--help" => return Ok(Some(RgParseResult::Help)),
        "--type-list" => return Ok(Some(RgParseResult::TypeList)),
        "--files" => options.files = true,
        "--hidden" => options.hidden = true,
        "--no-ignore" | "--no-ignore-vcs" | "--no-ignore-dot" => options.no_ignore = true,
        "--ignore-case" => options.ignore_case = Some(true),
        "--case-sensitive" => options.ignore_case = Some(false),
        "--smart-case" => options.ignore_case = None,
        "--fixed-strings" => options.fixed = true,
        "--word-regexp" => options.word_regexp = true,
        "--line-regexp" => options.line_regexp = true,
        "--invert-match" => options.invert = true,
        "--line-number" => options.line_number = Some(true),
        "--no-line-number" => options.line_number = Some(false),
        "--count" => options.count = true,
        "--count-matches" => options.count_matches = true,
        "--files-with-matches" => options.files_with_matches = true,
        "--files-without-match" => options.files_without_match = true,
        "--only-matching" => options.only_matching = true,
        "--quiet" => options.quiet = true,
        "--no-filename" => options.no_filename = true,
        "--text" => options.text = true,
        "--include-zero" => options.include_zero = true,
        "--heading" => options.heading = true,
        "--pcre2" => return Err(stderr_result(2, "rg: PCRE2 is not available\n")),
        "--sort" => {
            rg_option_value(args, index, "--sort")?;
        }
        "--glob" => options.globs.push(rg_option_value(args, index, "--glob")?),
        "--type" => options
            .type_includes
            .push(rg_option_value(args, index, "--type")?),
        "--type-not" => options
            .type_excludes
            .push(rg_option_value(args, index, "--type-not")?),
        "--max-count" => {
            options.max_count = Some(rg_parse_usize(&rg_option_value(
                args,
                index,
                "--max-count",
            )?));
        }
        "--max-depth" => {
            options.max_depth = Some(rg_parse_usize(&rg_option_value(
                args,
                index,
                "--max-depth",
            )?));
        }
        "--after-context" => {
            options.after_context =
                rg_parse_usize(&rg_option_value(args, index, "--after-context")?);
        }
        "--before-context" => {
            options.before_context =
                rg_parse_usize(&rg_option_value(args, index, "--before-context")?);
        }
        "--context" => {
            let value = rg_parse_usize(&rg_option_value(args, index, "--context")?);
            options.before_context = value;
            options.after_context = value;
        }
        "--context-separator" => {
            options.context_separator = rg_option_value(args, index, "--context-separator")?;
        }
        "-P" => return Err(stderr_result(2, "rg: PCRE2 is not available\n")),
        "-n" => options.line_number = Some(true),
        "-N" => options.line_number = Some(false),
        "-i" => options.ignore_case = Some(true),
        "-s" => options.ignore_case = Some(false),
        "-S" => options.ignore_case = None,
        "-F" => options.fixed = true,
        "-w" => options.word_regexp = true,
        "-x" => options.line_regexp = true,
        "-v" => options.invert = true,
        "-c" => options.count = true,
        "-l" => options.files_with_matches = true,
        "-q" => options.quiet = true,
        "-o" => options.only_matching = true,
        "-I" => options.no_filename = true,
        "-a" => options.text = true,
        "-0" => options.null_separator = true,
        "-L" => {}
        "-e" => patterns.push(rg_option_value(args, index, "-e")?),
        "-f" => pattern_files.push(rg_option_value(args, index, "-f")?),
        "-g" => options.globs.push(rg_option_value(args, index, "-g")?),
        "-t" => options
            .type_includes
            .push(rg_option_value(args, index, "-t")?),
        "-T" => options
            .type_excludes
            .push(rg_option_value(args, index, "-T")?),
        "-m" => options.max_count = Some(rg_parse_usize(&rg_option_value(args, index, "-m")?)),
        "-d" => options.max_depth = Some(rg_parse_usize(&rg_option_value(args, index, "-d")?)),
        "-A" => options.after_context = rg_parse_usize(&rg_option_value(args, index, "-A")?),
        "-B" => options.before_context = rg_parse_usize(&rg_option_value(args, index, "-B")?),
        "-C" => {
            let value = rg_parse_usize(&rg_option_value(args, index, "-C")?);
            options.before_context = value;
            options.after_context = value;
        }
        _ if arg.starts_with("--glob=") => {
            options.globs.push(arg["--glob=".len()..].to_string());
        }
        _ if arg.starts_with("--type=") => {
            options
                .type_includes
                .push(arg["--type=".len()..].to_string());
        }
        _ if arg.starts_with("--type-not=") => {
            options
                .type_excludes
                .push(arg["--type-not=".len()..].to_string());
        }
        _ if arg.starts_with("--max-count=") => {
            options.max_count = Some(rg_parse_usize(&arg["--max-count=".len()..]));
        }
        _ if arg.starts_with("--max-depth=") => {
            options.max_depth = Some(rg_parse_usize(&arg["--max-depth=".len()..]));
        }
        _ if arg.starts_with("--after-context=") => {
            options.after_context = rg_parse_usize(&arg["--after-context=".len()..]);
        }
        _ if arg.starts_with("--before-context=") => {
            options.before_context = rg_parse_usize(&arg["--before-context=".len()..]);
        }
        _ if arg.starts_with("--context=") => {
            let value = rg_parse_usize(&arg["--context=".len()..]);
            options.before_context = value;
            options.after_context = value;
        }
        _ if arg.starts_with("--context-separator=") => {
            options.context_separator = arg["--context-separator=".len()..].to_string();
        }
        _ if arg.starts_with("--") => {
            return Err(stderr_result(
                1,
                format!("rg: unrecognized option '{arg}'\n"),
            ));
        }
        _ if arg.starts_with("-m") && arg.len() > 2 => {
            options.max_count = Some(rg_parse_usize(&arg[2..]));
        }
        _ if arg.starts_with("-d") && arg.len() > 2 => {
            options.max_depth = Some(rg_parse_usize(&arg[2..]));
        }
        _ if arg.starts_with("-A") && arg.len() > 2 => {
            options.after_context = rg_parse_usize(&arg[2..]);
        }
        _ if arg.starts_with("-B") && arg.len() > 2 => {
            options.before_context = rg_parse_usize(&arg[2..]);
        }
        _ if arg.starts_with("-C") && arg.len() > 2 => {
            let value = rg_parse_usize(&arg[2..]);
            options.before_context = value;
            options.after_context = value;
        }
        _ if arg.starts_with("-e") && arg.len() > 2 => patterns.push(arg[2..].to_string()),
        _ if arg.starts_with("-g") && arg.len() > 2 => options.globs.push(arg[2..].to_string()),
        _ if arg.starts_with("-t") && arg.len() > 2 => {
            options.type_includes.push(arg[2..].to_string());
        }
        _ if arg.starts_with("-T") && arg.len() > 2 => {
            options.type_excludes.push(arg[2..].to_string());
        }
        _ if arg.starts_with('-') => parse_rg_short_flags(arg, options)?,
        _ => {}
    }
    *index += 1;
    Ok(None)
}

fn parse_rg_short_flags(arg: &str, options: &mut RgOptions) -> Result<(), CommandResult> {
    let mut unrestricted = 0;
    for flag in arg[1..].chars() {
        match flag {
            'n' => options.line_number = Some(true),
            'N' => options.line_number = Some(false),
            'i' => options.ignore_case = Some(true),
            's' => options.ignore_case = Some(false),
            'S' => options.ignore_case = None,
            'F' => options.fixed = true,
            'w' => options.word_regexp = true,
            'x' => options.line_regexp = true,
            'v' => options.invert = true,
            'c' => options.count = true,
            'l' => options.files_with_matches = true,
            'q' => options.quiet = true,
            'o' => options.only_matching = true,
            'I' => options.no_filename = true,
            'a' => options.text = true,
            '0' => options.null_separator = true,
            'L' => {}
            'u' => unrestricted += 1,
            _ => {
                return Err(stderr_result(
                    1,
                    format!("rg: unrecognized option '-{flag}'\n"),
                ));
            }
        }
    }
    if unrestricted > 0 {
        options.no_ignore = true;
    }
    if unrestricted > 1 {
        options.hidden = true;
    }
    Ok(())
}

fn rg_option_value(
    args: &[String],
    index: &mut usize,
    option: &str,
) -> Result<String, CommandResult> {
    let Some(value) = args.get(*index + 1) else {
        return Err(stderr_result(
            2,
            format!("rg: option '{option}' requires an argument\n"),
        ));
    };
    *index += 1;
    Ok(value.clone())
}

fn rg_parse_usize(value: &str) -> usize {
    value.parse().unwrap_or(0)
}

fn rg_type_list() -> String {
    [
        "js: *.js, *.jsx",
        "ts: *.ts, *.tsx",
        "py: *.py",
        "rust: *.rs",
        "md: *.md, *.markdown, *.mdown",
        "json: *.json",
        "html: *.html, *.htm",
    ]
    .join("\n")
        + "\n"
}

#[derive(Clone, Debug)]
struct RgInput {
    label: String,
    text: String,
    explicit_file: bool,
}

fn rg_load_pattern_files(
    state: &ExecState<'_>,
    stdin: &str,
    pattern_files: &[String],
    patterns: &mut Vec<String>,
) -> Result<(), CommandResult> {
    if pattern_files.is_empty() {
        return Ok(());
    }
    let fs = state
        .session
        .inner
        .fs
        .lock()
        .map_err(|_| stderr_result(1, "rg: filesystem lock poisoned\n"))?;
    for pattern_file in pattern_files {
        let content = if pattern_file == "-" {
            stdin.to_string()
        } else {
            let path = resolve_path(&state.cwd, pattern_file);
            fs.read_file(&path)
                .map_err(|_| stderr_result(2, format!("rg: {path}: No such file or directory\n")))?
        };
        patterns.extend(content.lines().map(ToString::to_string));
    }
    Ok(())
}

fn command_rg_files(state: &ExecState<'_>, request: &RgRequest) -> CommandResult {
    let inputs = match rg_inputs(state, request) {
        Ok(inputs) => inputs,
        Err(error) => return stderr_result(2, format!("rg: {error}\n")),
    };
    if request.options.quiet {
        return CommandResult {
            exit_code: if inputs.is_empty() { 1 } else { 0 },
            ..CommandResult::default()
        };
    }
    let separator = if request.options.null_separator {
        '\0'
    } else {
        '\n'
    };
    let stdout = inputs
        .iter()
        .map(|input| format!("{}{}", input.label, separator))
        .collect::<String>();
    CommandResult {
        exit_code: if inputs.is_empty() { 1 } else { 0 },
        stdout,
        ..CommandResult::default()
    }
}

fn rg_inputs(state: &ExecState<'_>, request: &RgRequest) -> Result<Vec<RgInput>, String> {
    let fs = state
        .session
        .inner
        .fs
        .lock()
        .map_err(|_| "filesystem lock poisoned".to_string())?;
    let ignore_rules = if request.options.no_ignore {
        Vec::new()
    } else {
        rg_ignore_rules(&fs)
    };
    let roots = if request.roots.is_empty() {
        vec![state.cwd.clone()]
    } else {
        request
            .roots
            .iter()
            .map(|root| resolve_path(&state.cwd, root))
            .collect()
    };
    let mut inputs = Vec::new();
    for root in roots {
        let root_stat = fs
            .stat(&root)
            .map_err(|_| format!("{root}: No such file or directory"))?;
        if root_stat.is_file {
            rg_push_input(
                &state.cwd,
                &fs,
                request,
                &ignore_rules,
                &mut inputs,
                RgCandidate {
                    root: &root,
                    path: &root,
                    explicit_file: true,
                },
            );
            continue;
        }
        let mut paths = fs
            .get_all_paths()
            .into_iter()
            .filter(|path| path.starts_with(&format!("{root}/")))
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            rg_push_input(
                &state.cwd,
                &fs,
                request,
                &ignore_rules,
                &mut inputs,
                RgCandidate {
                    root: &root,
                    path: &path,
                    explicit_file: false,
                },
            );
        }
    }
    inputs.sort_by(|left, right| left.label.cmp(&right.label));
    Ok(inputs)
}

struct RgCandidate<'a> {
    root: &'a str,
    path: &'a str,
    explicit_file: bool,
}

fn rg_push_input(
    cwd: &str,
    fs: &VirtualFileSystem,
    request: &RgRequest,
    ignore_rules: &[RgIgnoreRule],
    inputs: &mut Vec<RgInput>,
    candidate: RgCandidate<'_>,
) {
    let Ok(stat) = fs.stat(candidate.path) else {
        return;
    };
    if !stat.is_file {
        return;
    }
    let label = relative_display_path(cwd, candidate.path);
    if !rg_file_passes_filters(
        candidate.path,
        &label,
        candidate.root,
        candidate.explicit_file,
        request,
        ignore_rules,
    ) {
        return;
    }
    let Ok(text) = fs.read_file(candidate.path) else {
        return;
    };
    if !request.options.text && text.contains('\0') {
        return;
    }
    inputs.push(RgInput {
        label,
        text,
        explicit_file: candidate.explicit_file,
    });
}

fn rg_file_passes_filters(
    path: &str,
    label: &str,
    root: &str,
    explicit_file: bool,
    request: &RgRequest,
    ignore_rules: &[RgIgnoreRule],
) -> bool {
    if let Some(max_depth) = request.options.max_depth
        && !explicit_file
        && rg_depth(root, path) >= max_depth
    {
        return false;
    }
    if !request.options.hidden && rg_path_is_hidden(label) {
        return false;
    }
    if !ignore_rules.is_empty() && rg_is_ignored(path, ignore_rules) {
        return false;
    }
    if !rg_type_filters_match(path, &request.options) {
        return false;
    }
    rg_globs_match(label, &request.options.globs)
}

fn rg_depth(root: &str, path: &str) -> usize {
    path.strip_prefix(root)
        .and_then(|relative| relative.strip_prefix('/'))
        .unwrap_or(path)
        .matches('/')
        .count()
}

fn rg_path_is_hidden(path: &str) -> bool {
    path.split('/')
        .filter(|part| !part.is_empty())
        .any(|part| part.starts_with('.'))
}

fn rg_type_filters_match(path: &str, options: &RgOptions) -> bool {
    if !options.type_includes.is_empty()
        && !options
            .type_includes
            .iter()
            .any(|type_name| rg_path_matches_type(path, type_name).unwrap_or(false))
    {
        return false;
    }
    !options
        .type_excludes
        .iter()
        .any(|type_name| rg_path_matches_type(path, type_name).unwrap_or(false))
}

fn rg_path_matches_type(path: &str, type_name: &str) -> Option<bool> {
    let extension = path.rsplit_once('.').map(|(_, extension)| {
        extension
            .chars()
            .flat_map(char::to_lowercase)
            .collect::<String>()
    })?;
    let extensions = match type_name {
        "js" | "javascript" => &["js", "jsx"][..],
        "ts" | "typescript" => &["ts", "tsx"][..],
        "py" | "python" => &["py"][..],
        "rs" | "rust" => &["rs"][..],
        "css" => &["css"][..],
        "md" | "markdown" => &["md", "markdown", "mdown"][..],
        "json" => &["json"][..],
        "html" => &["html", "htm"][..],
        "txt" | "text" => &["txt"][..],
        "log" => &["log"][..],
        _ => return None,
    };
    Some(extensions.contains(&extension.as_str()))
}

fn rg_globs_match(label: &str, globs: &[String]) -> bool {
    let mut saw_positive = false;
    let mut positive_match = false;
    for glob in globs {
        let (negated, pattern) = glob
            .strip_prefix('!')
            .map_or((false, glob.as_str()), |pattern| (true, pattern));
        let matched = rg_glob_matches_path(pattern, label);
        if negated && matched {
            return false;
        }
        if !negated {
            saw_positive = true;
            positive_match |= matched;
        }
    }
    !saw_positive || positive_match
}

#[derive(Clone, Debug)]
struct RgIgnoreRule {
    base: String,
    pattern: String,
    negated: bool,
    directory_only: bool,
    rooted: bool,
}

fn rg_ignore_rules(fs: &VirtualFileSystem) -> Vec<RgIgnoreRule> {
    let mut paths = fs
        .get_all_paths()
        .into_iter()
        .filter(|path| path.ends_with("/.gitignore"))
        .collect::<Vec<_>>();
    paths.sort();
    let mut rules = Vec::new();
    for path in paths {
        let Ok(content) = fs.read_file(&path) else {
            continue;
        };
        let base = path
            .rsplit_once('/')
            .map_or("/", |(base, _)| if base.is_empty() { "/" } else { base });
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (negated, line) = line
                .strip_prefix('!')
                .map_or((false, line), |line| (true, line));
            let directory_only = line.ends_with('/');
            let line = line.trim_end_matches('/');
            let rooted = line.starts_with('/');
            let pattern = line.trim_start_matches('/').to_string();
            if pattern.is_empty() {
                continue;
            }
            rules.push(RgIgnoreRule {
                base: base.to_string(),
                pattern,
                negated,
                directory_only,
                rooted,
            });
        }
    }
    rules
}

fn rg_is_ignored(path: &str, rules: &[RgIgnoreRule]) -> bool {
    let mut ignored = false;
    for rule in rules {
        if rg_ignore_rule_matches(rule, path) {
            ignored = !rule.negated;
        }
    }
    ignored
}

fn rg_ignore_rule_matches(rule: &RgIgnoreRule, path: &str) -> bool {
    let Some(relative) = path
        .strip_prefix(&rule.base)
        .and_then(|relative| relative.strip_prefix('/'))
    else {
        return false;
    };
    if relative.is_empty() {
        return false;
    }
    if rule.directory_only {
        if rule.rooted {
            return relative == rule.pattern || relative.starts_with(&format!("{}/", rule.pattern));
        }
        let directories = relative
            .rsplit_once('/')
            .map(|(directories, _)| directories)
            .unwrap_or("");
        return directories.split('/').any(|part| part == rule.pattern)
            || rg_glob_matches_path(&rule.pattern, directories);
    }
    if rule.rooted {
        return rg_glob_match(&rule.pattern, relative);
    }
    if rule.pattern.contains('/') {
        return rg_glob_matches_path(&rule.pattern, relative)
            || relative.ends_with(&format!("/{}", rule.pattern));
    }
    relative
        .split('/')
        .any(|part| rg_glob_match(&rule.pattern, part))
}

fn rg_glob_matches_path(pattern: &str, path: &str) -> bool {
    if let Some(unrooted) = pattern.strip_prefix("**/")
        && rg_glob_matches_path(unrooted, path)
    {
        return true;
    }
    if pattern.contains('/') {
        rg_glob_match(pattern, path)
    } else {
        let basename = path.rsplit('/').next().unwrap_or(path);
        rg_glob_match(pattern, basename)
    }
}

fn rg_glob_match(pattern: &str, text: &str) -> bool {
    Regex::new(&rg_glob_regex(pattern))
        .map(|regex| regex.is_match(text))
        .unwrap_or_else(|_| wildcard_match(pattern, text))
}

fn rg_glob_regex(pattern: &str) -> String {
    let mut regex = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                regex.push_str(".*");
            }
            '*' => regex.push_str("[^/]*"),
            '?' => regex.push_str("[^/]"),
            '[' => {
                regex.push('[');
                if chars.peek() == Some(&'!') {
                    chars.next();
                    regex.push('^');
                }
                for class_ch in chars.by_ref() {
                    regex.push(class_ch);
                    if class_ch == ']' {
                        break;
                    }
                }
            }
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '\\' => {
                regex.push('\\');
                regex.push(ch);
            }
            _ => regex.push(ch),
        }
    }
    regex.push('$');
    regex
}

#[derive(Clone, Debug)]
struct RgLineMatch {
    index: usize,
    line: String,
    only_matches: Vec<String>,
}

fn rg_render_search(
    request: &RgRequest,
    matchers: &[LineMatcher],
    inputs: &[RgInput],
) -> CommandResult {
    let mut stdout = String::new();
    let mut total_matches = 0;
    for input in inputs {
        let line_matches = rg_line_matches(input, matchers, &request.options);
        let file_match_count = if request.options.count_matches {
            line_matches
                .iter()
                .map(|line_match| line_match.only_matches.len().max(1))
                .sum()
        } else {
            line_matches.len()
        };
        total_matches += file_match_count;
        if request.options.quiet && file_match_count > 0 {
            return CommandResult {
                exit_code: 0,
                ..CommandResult::default()
            };
        }
        if request.options.files_with_matches {
            if file_match_count > 0 {
                rg_push_file_label(&mut stdout, input, &request.options);
            }
            continue;
        }
        if request.options.files_without_match {
            if file_match_count == 0 {
                rg_push_file_label(&mut stdout, input, &request.options);
                total_matches += 1;
            }
            continue;
        }
        if request.options.count || request.options.count_matches {
            if file_match_count > 0 || request.options.include_zero {
                rg_push_count(&mut stdout, request, input, file_match_count);
            }
            continue;
        }
        rg_push_matches(&mut stdout, request, input, &line_matches);
    }
    CommandResult {
        exit_code: if total_matches == 0 { 1 } else { 0 },
        stdout,
        ..CommandResult::default()
    }
}

fn rg_line_matches(
    input: &RgInput,
    matchers: &[LineMatcher],
    options: &RgOptions,
) -> Vec<RgLineMatch> {
    let max_count = options.max_count.filter(|count| *count > 0);
    let mut matches = Vec::new();
    for (index, line) in input.text.lines().enumerate() {
        let only_matches = matchers
            .iter()
            .flat_map(|matcher| matcher.match_texts(line))
            .collect::<Vec<_>>();
        let matched = !only_matches.is_empty();
        if matched ^ options.invert {
            matches.push(RgLineMatch {
                index,
                line: line.to_string(),
                only_matches,
            });
            if max_count.is_some_and(|limit| matches.len() >= limit) {
                break;
            }
        }
    }
    matches
}

fn rg_push_file_label(stdout: &mut String, input: &RgInput, options: &RgOptions) {
    stdout.push_str(&input.label);
    stdout.push(if options.null_separator { '\0' } else { '\n' });
}

fn rg_push_count(stdout: &mut String, request: &RgRequest, input: &RgInput, count: usize) {
    if rg_show_filename(request, input) {
        stdout.push_str(&input.label);
        stdout.push(':');
    }
    stdout.push_str(&format!("{count}\n"));
}

fn rg_push_matches(
    stdout: &mut String,
    request: &RgRequest,
    input: &RgInput,
    line_matches: &[RgLineMatch],
) {
    if line_matches.is_empty() {
        return;
    }
    if request.options.heading && rg_show_filename(request, input) {
        stdout.push_str(&input.label);
        stdout.push('\n');
    }
    if request.options.only_matching && !request.options.invert {
        rg_push_only_matches(stdout, request, input, line_matches);
        return;
    }
    if request.options.before_context == 0 && request.options.after_context == 0 {
        for line_match in line_matches {
            rg_push_line(
                stdout,
                request,
                input,
                line_match.index,
                &line_match.line,
                true,
            );
        }
        return;
    }
    rg_push_context_matches(stdout, request, input, line_matches);
}

fn rg_push_only_matches(
    stdout: &mut String,
    request: &RgRequest,
    input: &RgInput,
    line_matches: &[RgLineMatch],
) {
    for line_match in line_matches {
        for only_match in &line_match.only_matches {
            rg_push_line(stdout, request, input, line_match.index, only_match, true);
        }
    }
}

fn rg_push_context_matches(
    stdout: &mut String,
    request: &RgRequest,
    input: &RgInput,
    line_matches: &[RgLineMatch],
) {
    let lines = input.text.lines().collect::<Vec<_>>();
    let mut previous_end = None;
    for line_match in line_matches {
        let start = line_match
            .index
            .saturating_sub(request.options.before_context);
        let end = (line_match.index + request.options.after_context).min(lines.len() - 1);
        if let Some(previous_end) = previous_end
            && start > previous_end + 1
        {
            stdout.push_str(&request.options.context_separator);
            stdout.push('\n');
        }
        let first = previous_end.map_or(start, |previous_end| start.max(previous_end + 1));
        for (index, line) in lines.iter().enumerate().take(end + 1).skip(first) {
            rg_push_line(
                stdout,
                request,
                input,
                index,
                line,
                index == line_match.index,
            );
        }
        previous_end = Some(end);
    }
}

fn rg_push_line(
    stdout: &mut String,
    request: &RgRequest,
    input: &RgInput,
    line_index: usize,
    text: &str,
    matched: bool,
) {
    let show_filename = (request.options.only_matching || !request.options.heading)
        && rg_show_filename(request, input);
    let show_line_number = rg_show_line_number(request, input);
    let separator = if matched { ':' } else { '-' };
    if show_filename {
        stdout.push_str(&input.label);
        stdout.push(separator);
    }
    if show_line_number {
        stdout.push_str(&(line_index + 1).to_string());
        stdout.push(separator);
    }
    stdout.push_str(text);
    stdout.push('\n');
}

fn rg_show_filename(request: &RgRequest, input: &RgInput) -> bool {
    !request.options.no_filename && !input.explicit_file
}

fn rg_show_line_number(request: &RgRequest, input: &RgInput) -> bool {
    if request.options.only_matching {
        return request.options.line_number.unwrap_or(false);
    }
    request.options.line_number.unwrap_or(!input.explicit_file)
}

fn rg_smart_case_ignores_case(pattern: &str) -> bool {
    !pattern.chars().any(char::is_uppercase)
}

fn relative_display_path(cwd: &str, path: &str) -> String {
    path.strip_prefix(cwd)
        .and_then(|relative| relative.strip_prefix('/'))
        .filter(|relative| !relative.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn command_sed(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let mut quiet = false;
    let mut scripts = Vec::new();
    let mut paths = Vec::new();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        match arg.as_str() {
            "-n" => {
                quiet = true;
                index += 1;
            }
            "-e" => {
                if let Some(script) = args.get(index + 1) {
                    scripts.push(script.clone());
                    index += 2;
                } else {
                    return stderr_result(1, "sed: option requires an argument -- e\n");
                }
            }
            _ if scripts.is_empty() => {
                scripts.push(arg.clone());
                index += 1;
            }
            _ => {
                paths.push(arg.clone());
                index += 1;
            }
        }
    }
    if scripts.is_empty() {
        return stderr_result(1, "sed: missing script\n");
    };
    let input = match collect_text_inputs(state, &paths, stdin) {
        Ok(input) => input,
        Err(error) => return stderr_result(1, format!("sed: {error}\n")),
    };
    let mut lines = input.lines().map(ToString::to_string).collect::<Vec<_>>();
    let mut explicit_print = Vec::new();
    for script in &scripts {
        let command = match parse_sed_command(script) {
            Ok(command) => command,
            Err(error) => return stderr_result(1, format!("sed: {error}\n")),
        };
        match command {
            SedCommand::Substitute {
                address,
                pattern,
                replacement,
                global,
                ignore_case,
            } => {
                let regex = match RegexBuilder::new(&pattern)
                    .case_insensitive(ignore_case)
                    .build()
                {
                    Ok(regex) => regex,
                    Err(error) => return stderr_result(1, format!("sed: {error}\n")),
                };
                let line_count = lines.len();
                for (line_index, line) in lines.iter_mut().enumerate() {
                    if !sed_address_matches(address.as_ref(), line_index, line, line_count) {
                        continue;
                    }
                    let replacement = sed_replacement_to_regex(&replacement);
                    *line = if global {
                        regex.replace_all(line, replacement.as_str()).into_owned()
                    } else {
                        regex.replace(line, replacement.as_str()).into_owned()
                    };
                }
            }
            SedCommand::Print(address) => {
                for (line_index, line) in lines.iter().enumerate() {
                    if sed_address_matches(Some(&address), line_index, line, lines.len()) {
                        explicit_print.push(line.clone());
                    }
                }
            }
            SedCommand::Delete(address) => {
                let line_count = lines.len();
                lines = lines
                    .into_iter()
                    .enumerate()
                    .filter_map(|(line_index, line)| {
                        (!sed_address_matches(Some(&address), line_index, &line, line_count))
                            .then_some(line)
                    })
                    .collect();
            }
        }
    }
    let output_lines = if quiet { explicit_print } else { lines };
    let output = join_lines_with_newline(&output_lines);
    stdout_result(output)
}

fn command_awk(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let mut separator = AwkSeparator::Whitespace;
    let mut variables = BTreeMap::new();
    let mut program = None;
    let mut paths = Vec::new();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        match arg.as_str() {
            "--help" if program.is_none() => {
                return stdout_result(
                    "awk - pattern scanning and processing language\n\
Usage: awk [-F fs] [-v var=value] 'program' [file ...]\n",
                );
            }
            "-F" => {
                if let Some(value) = args.get(index + 1) {
                    separator = AwkSeparator::from_value(value);
                    index += 2;
                } else {
                    return stderr_result(1, "awk: option requires an argument -- F\n");
                }
            }
            _ if arg.starts_with("-F") && arg.len() > 2 => {
                separator = AwkSeparator::from_value(&arg[2..]);
                index += 1;
            }
            "-v" => {
                if let Some(value) = args.get(index + 1) {
                    if let Err(error) = assign_awk_variable(value, &mut variables) {
                        return stderr_result(1, format!("awk: {error}\n"));
                    }
                    index += 2;
                } else {
                    return stderr_result(1, "awk: option requires an argument -- v\n");
                }
            }
            _ if arg.starts_with("-v") && arg.len() > 2 => {
                if let Err(error) = assign_awk_variable(&arg[2..], &mut variables) {
                    return stderr_result(1, format!("awk: {error}\n"));
                }
                index += 1;
            }
            _ if program.is_none() => {
                program = Some(arg.clone());
                index += 1;
            }
            _ => {
                paths.push(arg.clone());
                index += 1;
            }
        }
    }
    let Some(program) = program else {
        return stderr_result(1, "awk: missing program\n");
    };
    let program = match parse_awk_program(&program) {
        Ok(program) => program,
        Err(error) => return stderr_result(1, format!("awk: {error}\n")),
    };
    let inputs = match collect_named_text_inputs(state, &paths, stdin, "awk") {
        Ok(inputs) => inputs,
        Err(error) => return stderr_result(1, format!("awk: {error}\n")),
    };
    let mut stdout = String::new();
    let mut runtime = AwkRuntime {
        separator,
        ofs: " ".to_string(),
        ors: "\n".to_string(),
        variables,
        arrays: BTreeMap::new(),
        range_active: BTreeMap::new(),
    };
    let mut nr = 0usize;
    let mut last_filename = String::new();

    for rule in program
        .rules
        .iter()
        .filter(|rule| matches!(rule.pattern, AwkPattern::Begin))
    {
        let mut context = AwkRecordContext::empty(nr, &last_filename);
        if let Err(error) = execute_awk_actions(
            rule.actions.as_slice(),
            &mut context,
            &mut runtime,
            &mut stdout,
        ) {
            return stderr_result(1, format!("awk: {error}\n"));
        }
    }

    for input in &inputs {
        last_filename.clone_from(&input.label);
        for (line_index, line) in input.text.lines().enumerate() {
            nr += 1;
            let fnr = line_index + 1;
            let fields = awk_fields(line, &runtime.separator);
            let mut context = AwkRecordContext {
                line: line.to_string(),
                fields,
                nr,
                fnr,
                filename: input.label.clone(),
            };
            for (rule_index, rule) in program
                .rules
                .iter()
                .enumerate()
                .filter(|(_, rule)| rule.pattern.is_record())
            {
                let matches = match awk_pattern_matches(
                    &rule.pattern,
                    rule_index,
                    &mut context,
                    &mut runtime,
                ) {
                    Ok(matches) => matches,
                    Err(error) => return stderr_result(1, format!("awk: {error}\n")),
                };
                if matches {
                    match execute_awk_actions(
                        rule.actions.as_slice(),
                        &mut context,
                        &mut runtime,
                        &mut stdout,
                    ) {
                        Ok(AwkFlow::Continue) => {}
                        Ok(AwkFlow::Next) => break,
                        Ok(AwkFlow::Exit(code)) => {
                            return CommandResult {
                                stdout,
                                exit_code: code,
                                ..CommandResult::default()
                            };
                        }
                        Err(error) => return stderr_result(1, format!("awk: {error}\n")),
                    }
                }
            }
        }
    }

    for rule in program
        .rules
        .iter()
        .filter(|rule| matches!(rule.pattern, AwkPattern::End))
    {
        let mut context = AwkRecordContext::empty(nr, &last_filename);
        if let Err(error) = execute_awk_actions(
            rule.actions.as_slice(),
            &mut context,
            &mut runtime,
            &mut stdout,
        ) {
            return stderr_result(1, format!("awk: {error}\n"));
        }
    }
    stdout_result(stdout)
}

fn command_head(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let mut lines = 10;
    let mut paths = Vec::new();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "-n" {
            if let Some(value) = args
                .get(index + 1)
                .and_then(|value| value.parse::<usize>().ok())
            {
                lines = value;
            }
            index += 2;
        } else if let Some(value) = arg.strip_prefix("-n").and_then(|value| value.parse().ok()) {
            lines = value;
            index += 1;
        } else if let Some(value) = arg
            .strip_prefix('-')
            .and_then(|value| value.parse::<usize>().ok())
        {
            lines = value;
            index += 1;
        } else {
            paths.push(arg.clone());
            index += 1;
        }
    }
    let inputs = match collect_named_text_inputs(state, &paths, stdin, "head") {
        Ok(input) => input,
        Err(error) => return stderr_result(1, format!("head: {error}\n")),
    };
    let mut stdout = String::new();
    let multiple_files = paths.len() > 1;
    for (input_index, input) in inputs.iter().enumerate() {
        if multiple_files {
            if input_index > 0 {
                stdout.push('\n');
            }
            stdout.push_str(&format!("==> {} <==\n", input.label));
        }
        let selected = input
            .text
            .lines()
            .take(lines)
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        stdout.push_str(&join_lines_with_newline(&selected));
    }
    stdout_result(stdout)
}

fn command_tail(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let mut lines = TailLines::Last(10);
    let mut paths = Vec::new();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "-n" {
            if let Some(value) = args
                .get(index + 1)
                .and_then(|value| parse_tail_lines(value))
            {
                lines = value;
            }
            index += 2;
        } else if let Some(value) = arg.strip_prefix("-n").and_then(parse_tail_lines) {
            lines = value;
            index += 1;
        } else if let Some(value) = arg.strip_prefix('-').and_then(|value| value.parse().ok()) {
            lines = TailLines::Last(value);
            index += 1;
        } else {
            paths.push(arg.clone());
            index += 1;
        }
    }
    let inputs = match collect_named_text_inputs(state, &paths, stdin, "tail") {
        Ok(input) => input,
        Err(error) => return stderr_result(1, format!("tail: {error}\n")),
    };
    let mut stdout = String::new();
    let multiple_files = paths.len() > 1;
    for (input_index, input) in inputs.iter().enumerate() {
        if multiple_files {
            if input_index > 0 {
                stdout.push('\n');
            }
            stdout.push_str(&format!("==> {} <==\n", input.label));
        }
        let all_lines = input
            .text
            .lines()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        let selected = match lines {
            TailLines::Last(count) => all_lines
                .iter()
                .skip(all_lines.len().saturating_sub(count))
                .cloned()
                .collect::<Vec<_>>(),
            TailLines::From(start) => {
                if start == 0 {
                    all_lines.clone()
                } else if start > all_lines.len() {
                    vec![String::new()]
                } else {
                    all_lines
                        .iter()
                        .skip(start - 1)
                        .cloned()
                        .collect::<Vec<_>>()
                }
            }
        };
        stdout.push_str(&join_lines_with_newline(&selected));
    }
    stdout_result(stdout)
}

#[derive(Clone, Debug)]
enum SedAddress {
    Line(usize),
    Last,
    Range(usize, usize),
    Pattern(String),
}

#[derive(Clone, Debug)]
enum SedCommand {
    Substitute {
        address: Option<SedAddress>,
        pattern: String,
        replacement: String,
        global: bool,
        ignore_case: bool,
    },
    Print(SedAddress),
    Delete(SedAddress),
}

fn parse_sed_command(script: &str) -> Result<SedCommand, String> {
    let script = script.trim();
    if let Some(address) = script.strip_suffix('p').and_then(parse_sed_address) {
        return Ok(SedCommand::Print(address));
    }
    if let Some(address) = script.strip_suffix('d').and_then(parse_sed_address) {
        return Ok(SedCommand::Delete(address));
    }
    let (address, substitution) = split_sed_address_and_command(script);
    let Some((pattern, replacement, flags)) = parse_sed_substitution_parts(&substitution) else {
        return Err("unsupported script".to_string());
    };
    Ok(SedCommand::Substitute {
        address,
        pattern,
        replacement,
        global: flags.contains('g'),
        ignore_case: flags.contains('i'),
    })
}

fn sed_replacement_to_regex(replacement: &str) -> String {
    let mut output = String::new();
    let mut chars = replacement.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '&' => output.push_str("$0"),
            '\\' => match chars.next() {
                Some('&') => push_regex_replacement_literal(&mut output, '&'),
                Some(next) => {
                    push_regex_replacement_literal(&mut output, '\\');
                    push_regex_replacement_literal(&mut output, next);
                }
                None => push_regex_replacement_literal(&mut output, '\\'),
            },
            other => push_regex_replacement_literal(&mut output, other),
        }
    }
    output
}

fn push_regex_replacement_literal(output: &mut String, ch: char) {
    if ch == '$' {
        output.push_str("$$");
    } else {
        output.push(ch);
    }
}

fn split_sed_address_and_command(script: &str) -> (Option<SedAddress>, String) {
    let trimmed = script.trim_start();
    if trimmed.starts_with('s') {
        return (None, trimmed.to_string());
    }
    if let Some((prefix, rest)) = trimmed.split_once('s')
        && let Some(address) = parse_sed_address(prefix.trim())
    {
        return (
            Some(address),
            rest.strip_prefix('/').map_or_else(
                || rest.to_string(),
                |tail| {
                    let mut command = String::from("s/");
                    command.push_str(tail);
                    command
                },
            ),
        );
    }
    (None, trimmed.to_string())
}

fn parse_sed_address(value: &str) -> Option<SedAddress> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    if value == "$" {
        return Some(SedAddress::Last);
    }
    if let Some((start, end)) = value.split_once(',') {
        return Some(SedAddress::Range(
            start.trim().parse().ok()?,
            end.trim().parse().ok()?,
        ));
    }
    if value.starts_with('/') && value.ends_with('/') && value.len() >= 2 {
        return Some(SedAddress::Pattern(value[1..value.len() - 1].to_string()));
    }
    value.parse().ok().map(SedAddress::Line)
}

fn parse_sed_substitution_parts(script: &str) -> Option<(String, String, String)> {
    let mut chars = script.chars();
    if chars.next()? != 's' {
        return None;
    }
    let delimiter = chars.next()?;
    let rest = chars.as_str();
    let (from, rest) = rest.split_once(delimiter)?;
    let (to, flags) = rest.split_once(delimiter)?;
    Some((from.to_string(), to.to_string(), flags.to_string()))
}

fn sed_address_matches(
    address: Option<&SedAddress>,
    line_index: usize,
    line: &str,
    line_count: usize,
) -> bool {
    let line_number = line_index + 1;
    match address {
        None => true,
        Some(SedAddress::Line(target)) => line_number == *target,
        Some(SedAddress::Last) => line_number == line_count,
        Some(SedAddress::Range(start, end)) => line_number >= *start && line_number <= *end,
        Some(SedAddress::Pattern(pattern)) => Regex::new(pattern)
            .map(|regex| regex.is_match(line))
            .unwrap_or(false),
    }
}

#[derive(Clone, Debug)]
enum AwkSeparator {
    Whitespace,
    Pattern(String),
}

#[derive(Clone, Debug)]
struct AwkProgram {
    rules: Vec<AwkRule>,
}

#[derive(Clone, Debug)]
struct AwkRule {
    pattern: AwkPattern,
    actions: Vec<AwkAction>,
}

#[derive(Clone, Debug)]
struct AwkRuntime {
    separator: AwkSeparator,
    ofs: String,
    ors: String,
    variables: BTreeMap<String, String>,
    arrays: BTreeMap<String, BTreeMap<String, String>>,
    range_active: BTreeMap<usize, bool>,
}

#[derive(Clone, Debug)]
struct AwkRecordContext {
    line: String,
    fields: Vec<String>,
    nr: usize,
    fnr: usize,
    filename: String,
}

#[derive(Clone, Debug)]
enum AwkPattern {
    Begin,
    End,
    Always,
    Expr(AwkExpr),
    Range {
        start: Box<AwkPattern>,
        end: Box<AwkPattern>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AwkBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    And,
    Or,
    RegexMatch,
    RegexNoMatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AwkUnaryOp {
    Plus,
    Minus,
    Not,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AwkAssignOp {
    Assign,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AwkFlow {
    Continue,
    Next,
    Exit(i32),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AwkTarget {
    Variable(String),
    Field(Box<AwkExpr>),
    ArrayElement { name: String, key: Box<AwkExpr> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AwkAction {
    Assign {
        target: AwkTarget,
        op: AwkAssignOp,
        value: AwkExpr,
    },
    Increment {
        target: AwkTarget,
        delta: i32,
    },
    Expr(AwkExpr),
    Print(Vec<AwkExpr>),
    Printf {
        format: AwkExpr,
        args: Vec<AwkExpr>,
    },
    Next,
    Exit(Option<AwkExpr>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AwkExpr {
    Literal(String),
    RegexLiteral(String),
    Number(String),
    WholeLine,
    Field(Box<AwkExpr>),
    Identifier(String),
    ArrayRef {
        name: String,
        key: Box<AwkExpr>,
    },
    Unary {
        op: AwkUnaryOp,
        expr: Box<AwkExpr>,
    },
    Binary {
        left: Box<AwkExpr>,
        op: AwkBinaryOp,
        right: Box<AwkExpr>,
    },
    Ternary {
        condition: Box<AwkExpr>,
        consequent: Box<AwkExpr>,
        alternate: Box<AwkExpr>,
    },
    FunctionCall {
        name: String,
        args: Vec<AwkExpr>,
    },
    Increment {
        target: AwkTarget,
        delta: i32,
        prefix: bool,
    },
    Concat(Vec<AwkExpr>),
}

impl AwkSeparator {
    fn from_value(value: &str) -> Self {
        let value = awk_unescape_string(value);
        if value == " " {
            Self::Whitespace
        } else {
            Self::Pattern(value)
        }
    }
}

impl AwkRecordContext {
    fn empty(nr: usize, filename: &str) -> Self {
        Self {
            line: String::new(),
            fields: Vec::new(),
            nr,
            fnr: 0,
            filename: filename.to_string(),
        }
    }

    fn rebuild_line(&mut self, ofs: &str) {
        self.line = self.fields.join(ofs);
    }

    fn replace_line(&mut self, line: String, separator: &AwkSeparator) {
        self.line = line;
        self.fields = awk_fields(&self.line, separator);
    }
}

impl AwkPattern {
    fn is_record(&self) -> bool {
        !matches!(self, Self::Begin | Self::End)
    }
}

fn assign_awk_variable(
    assignment: &str,
    variables: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    let Some((name, value)) = assignment.split_once('=') else {
        return Err(format!("invalid variable assignment: {assignment}"));
    };
    let name = name.trim();
    if !is_awk_identifier(name) {
        return Err(format!("invalid variable name: {name}"));
    }
    variables.insert(name.to_string(), awk_unescape_string(value));
    Ok(())
}

fn parse_awk_program(program: &str) -> Result<AwkProgram, String> {
    let mut cursor = 0;
    let mut rules = Vec::new();
    let source = program.trim();
    while cursor < source.len() {
        cursor = skip_awk_whitespace(source, cursor);
        if cursor >= source.len() {
            break;
        }

        let (pattern, block_start) = if starts_awk_keyword(source, cursor, "BEGIN") {
            cursor += "BEGIN".len();
            cursor = skip_awk_whitespace(source, cursor);
            (AwkPattern::Begin, expect_awk_block(source, cursor)?)
        } else if starts_awk_keyword(source, cursor, "END") {
            cursor += "END".len();
            cursor = skip_awk_whitespace(source, cursor);
            (AwkPattern::End, expect_awk_block(source, cursor)?)
        } else if source.as_bytes().get(cursor) == Some(&b'{') {
            (AwkPattern::Always, cursor)
        } else if let Some(block_start) = find_next_awk_block(source, cursor) {
            let pattern = parse_awk_pattern(source[cursor..block_start].trim())?;
            (pattern, block_start)
        } else {
            let pattern = parse_awk_pattern(source[cursor..].trim())?;
            rules.push(AwkRule {
                pattern,
                actions: vec![AwkAction::Print(vec![AwkExpr::whole_line()])],
            });
            cursor = source.len();
            continue;
        };

        let block_end = find_matching_awk_brace(source, block_start)
            .ok_or_else(|| "unterminated action block".to_string())?;
        let body = &source[block_start + 1..block_end];
        let actions = parse_awk_actions(body)?;
        rules.push(AwkRule { pattern, actions });
        cursor = block_end + 1;
    }

    if rules.is_empty() {
        return Err("unsupported program".to_string());
    }
    Ok(AwkProgram { rules })
}

impl AwkExpr {
    fn whole_line() -> Self {
        Self::WholeLine
    }
}

fn parse_awk_pattern(pattern: &str) -> Result<AwkPattern, String> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Ok(AwkPattern::Always);
    }
    if let Some(index) = find_awk_top_level_char(pattern, ',') {
        return Ok(AwkPattern::Range {
            start: Box::new(parse_awk_pattern(&pattern[..index])?),
            end: Box::new(parse_awk_pattern(&pattern[index + 1..])?),
        });
    }
    Ok(AwkPattern::Expr(parse_awk_expr(pattern)?))
}

fn parse_awk_actions(body: &str) -> Result<Vec<AwkAction>, String> {
    let mut actions = Vec::new();
    for statement in split_awk_statements(body) {
        let statement = statement.trim();
        if statement.is_empty() {
            continue;
        }
        if statement == "next" {
            actions.push(AwkAction::Next);
            continue;
        }
        if let Some(rest) = strip_awk_keyword(statement, "exit") {
            let rest = rest.trim();
            actions.push(AwkAction::Exit(
                (!rest.is_empty())
                    .then(|| parse_awk_expr(rest))
                    .transpose()?,
            ));
            continue;
        }
        if let Some(rest) = strip_awk_keyword(statement, "print") {
            actions.push(AwkAction::Print(parse_awk_print_exprs(rest)?));
            continue;
        }
        if let Some(rest) = strip_awk_keyword(statement, "printf") {
            actions.push(parse_awk_printf_action(rest)?);
            continue;
        }
        if let Some(target) = statement.strip_suffix("++") {
            actions.push(AwkAction::Increment {
                target: parse_awk_target(target.trim())?,
                delta: 1,
            });
            continue;
        }
        if let Some(target) = statement.strip_suffix("--") {
            actions.push(AwkAction::Increment {
                target: parse_awk_target(target.trim())?,
                delta: -1,
            });
            continue;
        }
        if let Some(target) = statement.strip_prefix("++") {
            actions.push(AwkAction::Increment {
                target: parse_awk_target(target.trim())?,
                delta: 1,
            });
            continue;
        }
        if let Some(target) = statement.strip_prefix("--") {
            actions.push(AwkAction::Increment {
                target: parse_awk_target(target.trim())?,
                delta: -1,
            });
            continue;
        }
        if let Some((index, op)) = find_awk_assignment(statement) {
            let target = parse_awk_target(statement[..index].trim())?;
            let value = parse_awk_expr(statement[index + op.token_len()..].trim())?;
            actions.push(AwkAction::Assign { target, op, value });
            continue;
        }
        actions.push(AwkAction::Expr(parse_awk_expr(statement)?));
    }
    Ok(actions)
}

fn parse_awk_printf_action(rest: &str) -> Result<AwkAction, String> {
    let rest = strip_awk_parentheses(rest.trim());
    let parts = split_awk_top_level(rest, ',');
    let Some(format) = parts.first() else {
        return Err("unsupported program".to_string());
    };
    Ok(AwkAction::Printf {
        format: parse_awk_expr(format.trim())?,
        args: parts
            .iter()
            .skip(1)
            .map(|part| parse_awk_expr(part.trim()))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

fn parse_awk_print_exprs(rest: &str) -> Result<Vec<AwkExpr>, String> {
    let rest = rest.trim();
    if rest.is_empty() {
        return Ok(vec![AwkExpr::whole_line()]);
    }
    split_awk_top_level(rest, ',')
        .into_iter()
        .map(|entry| parse_awk_expr(entry.trim()))
        .collect()
}

fn parse_awk_expr(value: &str) -> Result<AwkExpr, String> {
    let mut parser = AwkExprParser::new(value);
    let expression = parser.parse_expression()?;
    parser.skip_whitespace();
    if parser.is_done() {
        Ok(expression)
    } else {
        Err("unsupported program".to_string())
    }
}

fn parse_awk_target(value: &str) -> Result<AwkTarget, String> {
    let value = value.trim();
    if let Some(field) = value.strip_prefix('$') {
        return Ok(AwkTarget::Field(Box::new(parse_awk_field_expr(field)?)));
    }
    if let Some(identifier) = take_awk_identifier(value)
        && identifier.len() < value.len()
        && value[identifier.len()..].trim_start().starts_with('[')
    {
        let bracket_start = value
            .char_indices()
            .find_map(|(index, ch)| (ch == '[').then_some(index))
            .ok_or_else(|| "unsupported program".to_string())?;
        let bracket_end = find_matching_awk_bracket(value, bracket_start)
            .ok_or_else(|| "unsupported program".to_string())?;
        if !value[bracket_end + 1..].trim().is_empty() {
            return Err("unsupported program".to_string());
        }
        return Ok(AwkTarget::ArrayElement {
            name: identifier.to_string(),
            key: Box::new(parse_awk_array_key(&value[bracket_start + 1..bracket_end])?),
        });
    }
    if is_awk_identifier(value) {
        return Ok(AwkTarget::Variable(value.to_string()));
    }
    Err("unsupported program".to_string())
}

fn parse_awk_field_expr(value: &str) -> Result<AwkExpr, String> {
    let value = value.trim();
    if value.starts_with('(') && value.ends_with(')') {
        parse_awk_expr(&value[1..value.len() - 1])
    } else {
        parse_awk_expr(value)
    }
}

fn parse_awk_array_key(value: &str) -> Result<AwkExpr, String> {
    let parts = split_awk_top_level(value, ',');
    if parts.len() <= 1 {
        return parse_awk_expr(value);
    }
    Ok(AwkExpr::FunctionCall {
        name: "__subsep".to_string(),
        args: parts
            .into_iter()
            .map(|part| parse_awk_expr(part.trim()))
            .collect::<Result<Vec<_>, _>>()?,
    })
}

impl AwkAssignOp {
    fn token_len(self) -> usize {
        match self {
            Self::Assign => 1,
            Self::Add | Self::Sub | Self::Mul | Self::Div | Self::Mod | Self::Pow => 2,
        }
    }
}

struct AwkExprParser<'a> {
    source: &'a str,
    cursor: usize,
}

impl<'a> AwkExprParser<'a> {
    fn new(source: &'a str) -> Self {
        Self { source, cursor: 0 }
    }

    fn parse_expression(&mut self) -> Result<AwkExpr, String> {
        self.parse_ternary()
    }

    fn parse_ternary(&mut self) -> Result<AwkExpr, String> {
        let condition = self.parse_or()?;
        self.skip_whitespace();
        if !self.consume_char('?') {
            return Ok(condition);
        }
        let consequent = self.parse_expression()?;
        self.skip_whitespace();
        if !self.consume_char(':') {
            return Err("unsupported program".to_string());
        }
        let alternate = self.parse_expression()?;
        Ok(AwkExpr::Ternary {
            condition: Box::new(condition),
            consequent: Box::new(consequent),
            alternate: Box::new(alternate),
        })
    }

    fn parse_or(&mut self) -> Result<AwkExpr, String> {
        let mut expression = self.parse_and()?;
        while self.consume_token("||") {
            let right = self.parse_and()?;
            expression = AwkExpr::Binary {
                left: Box::new(expression),
                op: AwkBinaryOp::Or,
                right: Box::new(right),
            };
        }
        Ok(expression)
    }

    fn parse_and(&mut self) -> Result<AwkExpr, String> {
        let mut expression = self.parse_comparison()?;
        while self.consume_token("&&") {
            let right = self.parse_comparison()?;
            expression = AwkExpr::Binary {
                left: Box::new(expression),
                op: AwkBinaryOp::And,
                right: Box::new(right),
            };
        }
        Ok(expression)
    }

    fn parse_comparison(&mut self) -> Result<AwkExpr, String> {
        let mut expression = self.parse_concat()?;
        loop {
            let op = if self.consume_token("!~") {
                Some(AwkBinaryOp::RegexNoMatch)
            } else if self.consume_token("~") {
                Some(AwkBinaryOp::RegexMatch)
            } else if self.consume_token(">=") {
                Some(AwkBinaryOp::Ge)
            } else if self.consume_token("<=") {
                Some(AwkBinaryOp::Le)
            } else if self.consume_token("==") {
                Some(AwkBinaryOp::Eq)
            } else if self.consume_token("!=") {
                Some(AwkBinaryOp::Ne)
            } else if self.consume_token(">") {
                Some(AwkBinaryOp::Gt)
            } else if self.consume_token("<") {
                Some(AwkBinaryOp::Lt)
            } else {
                None
            };
            let Some(op) = op else {
                break;
            };
            let right = self.parse_concat()?;
            expression = AwkExpr::Binary {
                left: Box::new(expression),
                op,
                right: Box::new(right),
            };
        }
        Ok(expression)
    }

    fn parse_concat(&mut self) -> Result<AwkExpr, String> {
        let mut parts = vec![self.parse_add_sub()?];
        loop {
            self.skip_whitespace();
            if !self.next_starts_expression() {
                break;
            }
            parts.push(self.parse_add_sub()?);
        }
        if parts.len() == 1 {
            Ok(parts.remove(0))
        } else {
            Ok(AwkExpr::Concat(parts))
        }
    }

    fn parse_add_sub(&mut self) -> Result<AwkExpr, String> {
        let mut expression = self.parse_mul_div()?;
        loop {
            let op = if self.consume_token("+") {
                Some(AwkBinaryOp::Add)
            } else if self.consume_token("-") {
                Some(AwkBinaryOp::Sub)
            } else {
                None
            };
            let Some(op) = op else {
                break;
            };
            let right = self.parse_mul_div()?;
            expression = AwkExpr::Binary {
                left: Box::new(expression),
                op,
                right: Box::new(right),
            };
        }
        Ok(expression)
    }

    fn parse_mul_div(&mut self) -> Result<AwkExpr, String> {
        let mut expression = self.parse_power()?;
        loop {
            let op = if self.consume_token("*") {
                if self.consume_token("*") {
                    self.cursor = self.cursor.saturating_sub(2);
                    None
                } else {
                    Some(AwkBinaryOp::Mul)
                }
            } else if self.consume_token("/") {
                Some(AwkBinaryOp::Div)
            } else if self.consume_token("%") {
                Some(AwkBinaryOp::Mod)
            } else {
                None
            };
            let Some(op) = op else {
                break;
            };
            let right = self.parse_power()?;
            expression = AwkExpr::Binary {
                left: Box::new(expression),
                op,
                right: Box::new(right),
            };
        }
        Ok(expression)
    }

    fn parse_power(&mut self) -> Result<AwkExpr, String> {
        let expression = self.parse_unary()?;
        if self.consume_token("**") || self.consume_token("^") {
            let right = self.parse_power()?;
            return Ok(AwkExpr::Binary {
                left: Box::new(expression),
                op: AwkBinaryOp::Pow,
                right: Box::new(right),
            });
        }
        Ok(expression)
    }

    fn parse_unary(&mut self) -> Result<AwkExpr, String> {
        if self.consume_token("++") {
            return Ok(AwkExpr::Increment {
                target: self.parse_target_from_cursor()?,
                delta: 1,
                prefix: true,
            });
        }
        if self.consume_token("--") {
            return Ok(AwkExpr::Increment {
                target: self.parse_target_from_cursor()?,
                delta: -1,
                prefix: true,
            });
        }
        if self.consume_token("+") {
            return Ok(AwkExpr::Unary {
                op: AwkUnaryOp::Plus,
                expr: Box::new(self.parse_unary()?),
            });
        }
        if self.consume_token("-") {
            return Ok(AwkExpr::Unary {
                op: AwkUnaryOp::Minus,
                expr: Box::new(self.parse_unary()?),
            });
        }
        if self.consume_token("!") {
            return Ok(AwkExpr::Unary {
                op: AwkUnaryOp::Not,
                expr: Box::new(self.parse_unary()?),
            });
        }
        self.parse_postfix()
    }

    fn parse_postfix(&mut self) -> Result<AwkExpr, String> {
        let expression = self.parse_primary()?;
        self.skip_whitespace();
        if self.consume_token("++") {
            return Ok(AwkExpr::Increment {
                target: awk_expr_to_target(&expression)?,
                delta: 1,
                prefix: false,
            });
        }
        if self.consume_token("--") {
            return Ok(AwkExpr::Increment {
                target: awk_expr_to_target(&expression)?,
                delta: -1,
                prefix: false,
            });
        }
        Ok(expression)
    }

    fn parse_primary(&mut self) -> Result<AwkExpr, String> {
        self.skip_whitespace();
        if self.consume_char('(') {
            let expression = self.parse_expression()?;
            self.skip_whitespace();
            if !self.consume_char(')') {
                return Err("unsupported program".to_string());
            }
            return Ok(expression);
        }
        if self.peek_char() == Some('"') {
            let start = self.cursor;
            let end = find_awk_string_end(self.source, start)
                .ok_or_else(|| "unterminated string literal".to_string())?;
            self.cursor = end + 1;
            return Ok(AwkExpr::Literal(awk_unescape_string(
                &self.source[start + 1..end],
            )));
        }
        if self.peek_char() == Some('/') {
            let (pattern, next_cursor) = parse_awk_regex_literal(self.source, self.cursor)?;
            self.cursor = next_cursor;
            return Ok(AwkExpr::RegexLiteral(pattern));
        }
        if self.consume_char('$') {
            return Ok(AwkExpr::Field(Box::new(self.parse_primary()?)));
        }
        if let Some(number) = self.take_number() {
            return Ok(AwkExpr::Number(number.to_string()));
        }
        if let Some(identifier) = self.take_identifier() {
            self.skip_whitespace();
            if self.consume_char('(') {
                let args_source = self.take_until_matching_paren()?;
                let args = if args_source.trim().is_empty() {
                    Vec::new()
                } else {
                    split_awk_top_level(&args_source, ',')
                        .into_iter()
                        .map(|part| parse_awk_expr(part.trim()))
                        .collect::<Result<Vec<_>, _>>()?
                };
                return Ok(AwkExpr::FunctionCall {
                    name: identifier.to_string(),
                    args,
                });
            }
            if self.consume_char('[') {
                let key_source = self.take_until_matching_bracket()?;
                return Ok(AwkExpr::ArrayRef {
                    name: identifier.to_string(),
                    key: Box::new(parse_awk_array_key(&key_source)?),
                });
            }
            return Ok(AwkExpr::Identifier(identifier.to_string()));
        }
        Err("unsupported program".to_string())
    }

    fn parse_target_from_cursor(&mut self) -> Result<AwkTarget, String> {
        let start = self.cursor;
        let expression = self.parse_primary()?;
        awk_expr_to_target(&expression).inspect_err(|_| {
            self.cursor = start;
        })
    }

    fn take_identifier(&mut self) -> Option<&'a str> {
        self.skip_whitespace();
        let identifier = take_awk_identifier(&self.source[self.cursor..])?;
        self.cursor += identifier.len();
        Some(identifier)
    }

    fn take_number(&mut self) -> Option<&'a str> {
        self.skip_whitespace();
        let number = take_awk_number(&self.source[self.cursor..])?;
        self.cursor += number.len();
        Some(number)
    }

    fn take_until_matching_paren(&mut self) -> Result<String, String> {
        let start = self.cursor;
        let end = find_matching_awk_paren(self.source, start.saturating_sub(1))
            .ok_or_else(|| "unsupported program".to_string())?;
        self.cursor = end + 1;
        Ok(self.source[start..end].to_string())
    }

    fn take_until_matching_bracket(&mut self) -> Result<String, String> {
        let start = self.cursor;
        let end = find_matching_awk_bracket(self.source, start.saturating_sub(1))
            .ok_or_else(|| "unsupported program".to_string())?;
        self.cursor = end + 1;
        Ok(self.source[start..end].to_string())
    }

    fn next_starts_expression(&mut self) -> bool {
        self.skip_whitespace();
        let rest = &self.source[self.cursor..];
        if rest.is_empty() {
            return false;
        }
        if rest.starts_with("++") || rest.starts_with("--") {
            return true;
        }
        let Some(ch) = rest.chars().next() else {
            return false;
        };
        matches!(ch, '"' | '$' | '(' | '.')
            || ch.is_ascii_alphabetic()
            || ch == '_'
            || ch.is_ascii_digit()
    }

    fn consume_token(&mut self, token: &str) -> bool {
        self.skip_whitespace();
        if self.source[self.cursor..].starts_with(token) {
            self.cursor += token.len();
            true
        } else {
            false
        }
    }

    fn consume_char(&mut self, ch: char) -> bool {
        self.skip_whitespace();
        if self.peek_char() == Some(ch) {
            self.cursor += ch.len_utf8();
            true
        } else {
            false
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.source[self.cursor..].chars().next()
    }

    fn skip_whitespace(&mut self) {
        self.cursor = skip_awk_whitespace(self.source, self.cursor);
    }

    fn is_done(&self) -> bool {
        self.cursor >= self.source.len()
    }
}

fn awk_expr_to_target(expression: &AwkExpr) -> Result<AwkTarget, String> {
    match expression {
        AwkExpr::Identifier(name) => Ok(AwkTarget::Variable(name.clone())),
        AwkExpr::Field(index) => Ok(AwkTarget::Field(index.clone())),
        AwkExpr::ArrayRef { name, key } => Ok(AwkTarget::ArrayElement {
            name: name.clone(),
            key: key.clone(),
        }),
        _ => Err("unsupported program".to_string()),
    }
}

fn execute_awk_actions(
    actions: &[AwkAction],
    context: &mut AwkRecordContext,
    runtime: &mut AwkRuntime,
    stdout: &mut String,
) -> Result<AwkFlow, String> {
    for action in actions {
        match action {
            AwkAction::Assign { target, op, value } => {
                let current = get_awk_target_value(target, context, runtime)?;
                let value = eval_awk_expr(value, context, runtime)?;
                let value = apply_awk_assignment(&current, *op, &value);
                set_awk_target_value(target, value, context, runtime)?;
            }
            AwkAction::Increment { target, delta } => {
                increment_awk_target(target, *delta, context, runtime)?;
            }
            AwkAction::Expr(expression) => {
                let _ = eval_awk_expr(expression, context, runtime)?;
            }
            AwkAction::Print(expressions) => {
                let values = expressions
                    .iter()
                    .map(|expr| eval_awk_expr(expr, context, runtime))
                    .collect::<Result<Vec<_>, _>>()?;
                stdout.push_str(&values.join(&runtime.ofs));
                stdout.push_str(&runtime.ors);
            }
            AwkAction::Printf { format, args } => {
                let format = eval_awk_expr(format, context, runtime)?;
                let values = args
                    .iter()
                    .map(|expr| eval_awk_expr(expr, context, runtime))
                    .collect::<Result<Vec<_>, _>>()?;
                stdout.push_str(&format_awk_printf(&format, &values)?);
            }
            AwkAction::Next => return Ok(AwkFlow::Next),
            AwkAction::Exit(code) => {
                let code = match code {
                    Some(code) => awk_to_number(&eval_awk_expr(code, context, runtime)?) as i32,
                    None => 0,
                };
                return Ok(AwkFlow::Exit(code));
            }
        }
    }
    Ok(AwkFlow::Continue)
}

fn eval_awk_expr(
    expression: &AwkExpr,
    context: &mut AwkRecordContext,
    runtime: &mut AwkRuntime,
) -> Result<String, String> {
    match expression {
        AwkExpr::Literal(value) | AwkExpr::RegexLiteral(value) | AwkExpr::Number(value) => {
            Ok(value.clone())
        }
        AwkExpr::WholeLine => Ok(context.line.clone()),
        AwkExpr::Field(index) => {
            let index = awk_to_number(&eval_awk_expr(index, context, runtime)?) as isize;
            Ok(awk_field_value(index, context))
        }
        AwkExpr::Identifier(identifier) => Ok(awk_identifier_value(identifier, context, runtime)),
        AwkExpr::ArrayRef { name, key } => {
            let key = eval_awk_array_key(key, context, runtime)?;
            Ok(runtime
                .arrays
                .get(name)
                .and_then(|array| array.get(&key))
                .cloned()
                .unwrap_or_default())
        }
        AwkExpr::Unary { op, expr } => {
            let value = eval_awk_expr(expr, context, runtime)?;
            Ok(match op {
                AwkUnaryOp::Plus => format_awk_number(awk_to_number(&value)),
                AwkUnaryOp::Minus => format_awk_number(-awk_to_number(&value)),
                AwkUnaryOp::Not => awk_bool(!awk_truthy(&value)),
            })
        }
        AwkExpr::Binary { left, op, right } => eval_awk_binary(left, *op, right, context, runtime),
        AwkExpr::Ternary {
            condition,
            consequent,
            alternate,
        } => {
            if eval_awk_truth(condition, context, runtime)? {
                eval_awk_expr(consequent, context, runtime)
            } else {
                eval_awk_expr(alternate, context, runtime)
            }
        }
        AwkExpr::FunctionCall { name, args } => eval_awk_function(name, args, context, runtime),
        AwkExpr::Increment {
            target,
            delta,
            prefix,
        } => {
            let previous = get_awk_target_value(target, context, runtime)?;
            let updated = increment_awk_target(target, *delta, context, runtime)?;
            Ok(if *prefix { updated } else { previous })
        }
        AwkExpr::Concat(expressions) => Ok(expressions
            .iter()
            .map(|expr| eval_awk_expr(expr, context, runtime))
            .collect::<Result<Vec<_>, _>>()?
            .join("")),
    }
}

fn eval_awk_binary(
    left: &AwkExpr,
    op: AwkBinaryOp,
    right: &AwkExpr,
    context: &mut AwkRecordContext,
    runtime: &mut AwkRuntime,
) -> Result<String, String> {
    match op {
        AwkBinaryOp::And => {
            if !eval_awk_truth(left, context, runtime)? {
                return Ok("0".to_string());
            }
            Ok(awk_bool(eval_awk_truth(right, context, runtime)?))
        }
        AwkBinaryOp::Or => {
            if eval_awk_truth(left, context, runtime)? {
                return Ok("1".to_string());
            }
            Ok(awk_bool(eval_awk_truth(right, context, runtime)?))
        }
        AwkBinaryOp::RegexMatch | AwkBinaryOp::RegexNoMatch => {
            let left = eval_awk_expr(left, context, runtime)?;
            let pattern = eval_awk_expr(right, context, runtime)?;
            let matched = Regex::new(&pattern)
                .map_err(|error| error.to_string())?
                .is_match(&left);
            Ok(awk_bool(if matches!(op, AwkBinaryOp::RegexMatch) {
                matched
            } else {
                !matched
            }))
        }
        AwkBinaryOp::Eq
        | AwkBinaryOp::Ne
        | AwkBinaryOp::Gt
        | AwkBinaryOp::Ge
        | AwkBinaryOp::Lt
        | AwkBinaryOp::Le => {
            let left = eval_awk_expr(left, context, runtime)?;
            let right = eval_awk_expr(right, context, runtime)?;
            Ok(awk_bool(compare_awk_values(&left, op, &right)))
        }
        AwkBinaryOp::Add
        | AwkBinaryOp::Sub
        | AwkBinaryOp::Mul
        | AwkBinaryOp::Div
        | AwkBinaryOp::Mod
        | AwkBinaryOp::Pow => {
            let left = awk_to_number(&eval_awk_expr(left, context, runtime)?);
            let right = awk_to_number(&eval_awk_expr(right, context, runtime)?);
            let value = match op {
                AwkBinaryOp::Add => left + right,
                AwkBinaryOp::Sub => left - right,
                AwkBinaryOp::Mul => left * right,
                AwkBinaryOp::Div => left / right,
                AwkBinaryOp::Mod => left % right,
                AwkBinaryOp::Pow => left.powf(right),
                _ => unreachable!(),
            };
            Ok(format_awk_number(value))
        }
    }
}

fn eval_awk_truth(
    expression: &AwkExpr,
    context: &mut AwkRecordContext,
    runtime: &mut AwkRuntime,
) -> Result<bool, String> {
    if let AwkExpr::RegexLiteral(pattern) = expression {
        return Regex::new(pattern)
            .map(|regex| regex.is_match(&context.line))
            .map_err(|error| error.to_string());
    }
    Ok(awk_truthy(&eval_awk_expr(expression, context, runtime)?))
}

fn awk_identifier_value(
    identifier: &str,
    context: &AwkRecordContext,
    runtime: &AwkRuntime,
) -> String {
    match identifier {
        "NR" => context.nr.to_string(),
        "FNR" => context.fnr.to_string(),
        "NF" => context.fields.len().to_string(),
        "FILENAME" => context.filename.clone(),
        "FS" => runtime.separator.as_value(),
        "OFS" => runtime.ofs.clone(),
        "ORS" => runtime.ors.clone(),
        "RSTART" => runtime
            .variables
            .get("RSTART")
            .cloned()
            .unwrap_or_else(|| "0".to_string()),
        "RLENGTH" => runtime
            .variables
            .get("RLENGTH")
            .cloned()
            .unwrap_or_else(|| "-1".to_string()),
        _ => runtime
            .variables
            .get(identifier)
            .cloned()
            .unwrap_or_default(),
    }
}

fn awk_field_value(index: isize, context: &AwkRecordContext) -> String {
    if index == 0 {
        return context.line.clone();
    }
    if index < 0 {
        return String::new();
    }
    context
        .fields
        .get(index as usize - 1)
        .cloned()
        .unwrap_or_default()
}

fn eval_awk_array_key(
    key: &AwkExpr,
    context: &mut AwkRecordContext,
    runtime: &mut AwkRuntime,
) -> Result<String, String> {
    eval_awk_expr(key, context, runtime)
}

fn get_awk_target_value(
    target: &AwkTarget,
    context: &mut AwkRecordContext,
    runtime: &mut AwkRuntime,
) -> Result<String, String> {
    match target {
        AwkTarget::Variable(name) => Ok(awk_identifier_value(name, context, runtime)),
        AwkTarget::Field(index) => {
            let index = awk_to_number(&eval_awk_expr(index, context, runtime)?) as isize;
            Ok(awk_field_value(index, context))
        }
        AwkTarget::ArrayElement { name, key } => {
            let key = eval_awk_array_key(key, context, runtime)?;
            Ok(runtime
                .arrays
                .get(name)
                .and_then(|array| array.get(&key))
                .cloned()
                .unwrap_or_default())
        }
    }
}

fn set_awk_target_value(
    target: &AwkTarget,
    value: String,
    context: &mut AwkRecordContext,
    runtime: &mut AwkRuntime,
) -> Result<(), String> {
    match target {
        AwkTarget::Variable(name) => match name.as_str() {
            "FS" => runtime.separator = AwkSeparator::from_value(&value),
            "OFS" => runtime.ofs = value,
            "ORS" => runtime.ors = value,
            "NF" => resize_awk_fields(value, context, &runtime.ofs),
            _ => {
                runtime.variables.insert(name.clone(), value);
            }
        },
        AwkTarget::Field(index) => {
            let index = awk_to_number(&eval_awk_expr(index, context, runtime)?) as isize;
            set_awk_field_value(index, value, context, &runtime.separator, &runtime.ofs);
        }
        AwkTarget::ArrayElement { name, key } => {
            let key = eval_awk_array_key(key, context, runtime)?;
            runtime
                .arrays
                .entry(name.clone())
                .or_default()
                .insert(key, value);
        }
    }
    Ok(())
}

fn increment_awk_target(
    target: &AwkTarget,
    delta: i32,
    context: &mut AwkRecordContext,
    runtime: &mut AwkRuntime,
) -> Result<String, String> {
    let current = get_awk_target_value(target, context, runtime)?;
    let updated = format_awk_number(awk_to_number(&current) + f64::from(delta));
    set_awk_target_value(target, updated.clone(), context, runtime)?;
    Ok(updated)
}

fn apply_awk_assignment(current: &str, op: AwkAssignOp, value: &str) -> String {
    if op == AwkAssignOp::Assign {
        return value.to_string();
    }
    let left = awk_to_number(current);
    let right = awk_to_number(value);
    let value = match op {
        AwkAssignOp::Assign => unreachable!(),
        AwkAssignOp::Add => left + right,
        AwkAssignOp::Sub => left - right,
        AwkAssignOp::Mul => left * right,
        AwkAssignOp::Div => left / right,
        AwkAssignOp::Mod => left % right,
        AwkAssignOp::Pow => left.powf(right),
    };
    format_awk_number(value)
}

fn set_awk_field_value(
    index: isize,
    value: String,
    context: &mut AwkRecordContext,
    separator: &AwkSeparator,
    ofs: &str,
) {
    if index == 0 {
        context.replace_line(value, separator);
        return;
    }
    if index < 0 {
        return;
    }
    let index = index as usize;
    if context.fields.len() < index {
        context.fields.resize(index, String::new());
    }
    context.fields[index - 1] = value;
    context.rebuild_line(ofs);
}

fn resize_awk_fields(value: String, context: &mut AwkRecordContext, ofs: &str) {
    let new_len = awk_to_number(&value).max(0.0) as usize;
    context.fields.resize(new_len, String::new());
    context.rebuild_line(ofs);
}

fn compare_awk_values(left: &str, op: AwkBinaryOp, right: &str) -> bool {
    let numeric = awk_numeric_value(left).zip(awk_numeric_value(right));
    if let Some((left, right)) = numeric {
        return match op {
            AwkBinaryOp::Eq => left == right,
            AwkBinaryOp::Ne => left != right,
            AwkBinaryOp::Gt => left > right,
            AwkBinaryOp::Ge => left >= right,
            AwkBinaryOp::Lt => left < right,
            AwkBinaryOp::Le => left <= right,
            _ => unreachable!(),
        };
    }
    match op {
        AwkBinaryOp::Eq => left == right,
        AwkBinaryOp::Ne => left != right,
        AwkBinaryOp::Gt => left > right,
        AwkBinaryOp::Ge => left >= right,
        AwkBinaryOp::Lt => left < right,
        AwkBinaryOp::Le => left <= right,
        _ => unreachable!(),
    }
}

fn eval_awk_function(
    name: &str,
    args: &[AwkExpr],
    context: &mut AwkRecordContext,
    runtime: &mut AwkRuntime,
) -> Result<String, String> {
    match name {
        "__subsep" => Ok(args
            .iter()
            .map(|arg| eval_awk_expr(arg, context, runtime))
            .collect::<Result<Vec<_>, _>>()?
            .join("\x1c")),
        "length" => {
            let value = if let Some(arg) = args.first() {
                eval_awk_expr(arg, context, runtime)?
            } else {
                context.line.clone()
            };
            Ok(value.chars().count().to_string())
        }
        "substr" => {
            let value =
                eval_awk_expr(args.first().ok_or("unsupported program")?, context, runtime)?;
            let start = args
                .get(1)
                .map(|arg| eval_awk_expr(arg, context, runtime))
                .transpose()?
                .map(|value| awk_to_number(&value) as isize)
                .unwrap_or(1)
                .max(1) as usize;
            let length = args
                .get(2)
                .map(|arg| eval_awk_expr(arg, context, runtime))
                .transpose()?
                .map(|value| awk_to_number(&value).max(0.0) as usize);
            let chars = value.chars().collect::<Vec<_>>();
            if start > chars.len() {
                return Ok(String::new());
            }
            let offset = start.saturating_sub(1);
            let end = length.map_or(chars.len(), |length| (offset + length).min(chars.len()));
            Ok(chars[offset..end].iter().collect())
        }
        "index" => {
            let haystack =
                eval_awk_expr(args.first().ok_or("unsupported program")?, context, runtime)?;
            let needle =
                eval_awk_expr(args.get(1).ok_or("unsupported program")?, context, runtime)?;
            if needle.is_empty() {
                return Ok("1".to_string());
            }
            Ok(haystack
                .find(&needle)
                .map(|index| haystack[..index].chars().count() + 1)
                .unwrap_or(0)
                .to_string())
        }
        "tolower" => {
            Ok(
                eval_awk_expr(args.first().ok_or("unsupported program")?, context, runtime)?
                    .to_lowercase(),
            )
        }
        "toupper" => {
            Ok(
                eval_awk_expr(args.first().ok_or("unsupported program")?, context, runtime)?
                    .to_uppercase(),
            )
        }
        "int" => Ok(format_awk_number(
            awk_to_number(&eval_awk_expr(
                args.first().ok_or("unsupported program")?,
                context,
                runtime,
            )?)
            .floor(),
        )),
        "sqrt" => Ok(format_awk_number(
            awk_to_number(&eval_awk_expr(
                args.first().ok_or("unsupported program")?,
                context,
                runtime,
            )?)
            .sqrt(),
        )),
        "exp" => Ok(format_awk_number(
            awk_to_number(&eval_awk_expr(
                args.first().ok_or("unsupported program")?,
                context,
                runtime,
            )?)
            .exp(),
        )),
        "log" => Ok(format_awk_number(
            awk_to_number(&eval_awk_expr(
                args.first().ok_or("unsupported program")?,
                context,
                runtime,
            )?)
            .ln(),
        )),
        "sin" => Ok(format_awk_number(
            awk_to_number(&eval_awk_expr(
                args.first().ok_or("unsupported program")?,
                context,
                runtime,
            )?)
            .sin(),
        )),
        "cos" => Ok(format_awk_number(
            awk_to_number(&eval_awk_expr(
                args.first().ok_or("unsupported program")?,
                context,
                runtime,
            )?)
            .cos(),
        )),
        "atan2" => {
            let y = awk_to_number(&eval_awk_expr(
                args.first().ok_or("unsupported program")?,
                context,
                runtime,
            )?);
            let x = awk_to_number(&eval_awk_expr(
                args.get(1).ok_or("unsupported program")?,
                context,
                runtime,
            )?);
            Ok(format_awk_number(y.atan2(x)))
        }
        "sprintf" => {
            let format =
                eval_awk_expr(args.first().ok_or("unsupported program")?, context, runtime)?;
            let values = args
                .iter()
                .skip(1)
                .map(|arg| eval_awk_expr(arg, context, runtime))
                .collect::<Result<Vec<_>, _>>()?;
            format_awk_printf(&format, &values)
        }
        "match" => {
            let value =
                eval_awk_expr(args.first().ok_or("unsupported program")?, context, runtime)?;
            let pattern =
                eval_awk_expr(args.get(1).ok_or("unsupported program")?, context, runtime)?;
            let regex = Regex::new(&pattern).map_err(|error| error.to_string())?;
            if let Some(matched) = regex.find(&value) {
                let start = value[..matched.start()].chars().count() + 1;
                let length = value[matched.start()..matched.end()].chars().count();
                runtime
                    .variables
                    .insert("RSTART".to_string(), start.to_string());
                runtime
                    .variables
                    .insert("RLENGTH".to_string(), length.to_string());
                Ok(start.to_string())
            } else {
                runtime
                    .variables
                    .insert("RSTART".to_string(), "0".to_string());
                runtime
                    .variables
                    .insert("RLENGTH".to_string(), "-1".to_string());
                Ok("0".to_string())
            }
        }
        "gensub" => {
            let pattern =
                eval_awk_expr(args.first().ok_or("unsupported program")?, context, runtime)?;
            let replacement =
                eval_awk_expr(args.get(1).ok_or("unsupported program")?, context, runtime)?;
            let flag = eval_awk_expr(args.get(2).ok_or("unsupported program")?, context, runtime)?;
            let input = args
                .get(3)
                .map(|arg| eval_awk_expr(arg, context, runtime))
                .transpose()?
                .unwrap_or_else(|| context.line.clone());
            let occurrence = if flag == "g" {
                AwkReplaceOccurrence::All
            } else {
                AwkReplaceOccurrence::Nth(awk_to_number(&flag).max(1.0) as usize)
            };
            awk_replace(&input, &pattern, &replacement, occurrence).map(|(value, _)| value)
        }
        "sub" | "gsub" => {
            let pattern =
                eval_awk_expr(args.first().ok_or("unsupported program")?, context, runtime)?;
            let replacement =
                eval_awk_expr(args.get(1).ok_or("unsupported program")?, context, runtime)?;
            let target = args
                .get(2)
                .map(awk_expr_to_target)
                .transpose()?
                .unwrap_or_else(|| AwkTarget::Field(Box::new(AwkExpr::Number("0".to_string()))));
            let input = get_awk_target_value(&target, context, runtime)?;
            let occurrence = if name == "gsub" {
                AwkReplaceOccurrence::All
            } else {
                AwkReplaceOccurrence::First
            };
            let (value, count) = awk_replace(&input, &pattern, &replacement, occurrence)?;
            set_awk_target_value(&target, value, context, runtime)?;
            Ok(count.to_string())
        }
        _ => Err("unsupported program".to_string()),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AwkReplaceOccurrence {
    First,
    All,
    Nth(usize),
}

fn awk_replace(
    input: &str,
    pattern: &str,
    replacement: &str,
    occurrence: AwkReplaceOccurrence,
) -> Result<(String, usize), String> {
    let regex = Regex::new(pattern).map_err(|error| error.to_string())?;
    let mut count = 0usize;
    let mut seen = 0usize;
    let output = regex
        .replace_all(input, |captures: &regex::Captures<'_>| {
            seen += 1;
            let replace = match occurrence {
                AwkReplaceOccurrence::All => true,
                AwkReplaceOccurrence::First => seen == 1,
                AwkReplaceOccurrence::Nth(target) => seen == target,
            };
            if replace {
                count += 1;
                awk_expand_replacement(replacement, captures)
            } else {
                captures
                    .get(0)
                    .map(|matched| matched.as_str().to_string())
                    .unwrap_or_default()
            }
        })
        .to_string();
    Ok((output, count))
}

fn awk_expand_replacement(replacement: &str, captures: &regex::Captures<'_>) -> String {
    let matched = captures
        .get(0)
        .map(|matched| matched.as_str())
        .unwrap_or("");
    replacement.replace('&', matched)
}

fn awk_pattern_matches(
    pattern: &AwkPattern,
    rule_index: usize,
    context: &mut AwkRecordContext,
    runtime: &mut AwkRuntime,
) -> Result<bool, String> {
    match pattern {
        AwkPattern::Begin | AwkPattern::End => Ok(false),
        AwkPattern::Always => Ok(true),
        AwkPattern::Expr(expression) => eval_awk_truth(expression, context, runtime),
        AwkPattern::Range { start, end } => {
            let active = runtime
                .range_active
                .get(&rule_index)
                .copied()
                .unwrap_or(false);
            let matches = active || awk_pattern_matches_stateless(start, context, runtime)?;
            if matches {
                let end_matches = awk_pattern_matches_stateless(end, context, runtime)?;
                runtime.range_active.insert(rule_index, !end_matches);
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }
}

fn awk_pattern_matches_stateless(
    pattern: &AwkPattern,
    context: &mut AwkRecordContext,
    runtime: &mut AwkRuntime,
) -> Result<bool, String> {
    match pattern {
        AwkPattern::Begin | AwkPattern::End => Ok(false),
        AwkPattern::Always => Ok(true),
        AwkPattern::Expr(expression) => eval_awk_truth(expression, context, runtime),
        AwkPattern::Range { .. } => Err("unsupported program".to_string()),
    }
}

fn awk_truthy(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    awk_numeric_value(value) != Some(0.0)
}

fn awk_bool(value: bool) -> String {
    if value {
        "1".to_string()
    } else {
        "0".to_string()
    }
}

fn awk_to_number(value: &str) -> f64 {
    awk_numeric_value(value).unwrap_or(0.0)
}

fn awk_numeric_value(value: &str) -> Option<f64> {
    let value = value.trim_start();
    let mut end = 0usize;
    let mut seen_digit = false;
    let mut chars = value.char_indices().peekable();
    if let Some((_, '+' | '-')) = chars.peek().copied() {
        end = 1;
        chars.next();
    }
    while let Some((index, ch)) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            seen_digit = true;
            end = index + ch.len_utf8();
            chars.next();
        } else {
            break;
        }
    }
    if let Some((index, '.')) = chars.peek().copied() {
        end = index + 1;
        chars.next();
        while let Some((index, ch)) = chars.peek().copied() {
            if ch.is_ascii_digit() {
                seen_digit = true;
                end = index + ch.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
    }
    if !seen_digit {
        return None;
    }
    if let Some((exponent_index, 'e' | 'E')) = chars.peek().copied() {
        let mut exponent_end = exponent_index + 1;
        let mut exponent_seen_digit = false;
        chars.next();
        if let Some((index, '+' | '-')) = chars.peek().copied() {
            exponent_end = index + 1;
            chars.next();
        }
        while let Some((index, ch)) = chars.peek().copied() {
            if ch.is_ascii_digit() {
                exponent_seen_digit = true;
                exponent_end = index + ch.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        if exponent_seen_digit {
            end = exponent_end;
        }
    }
    value[..end].parse().ok()
}

fn format_awk_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    if value.is_finite() && (value.fract()).abs() < f64::EPSILON {
        return (value as i64).to_string();
    }
    let formatted = value.to_string();
    formatted
        .strip_suffix(".0")
        .unwrap_or(&formatted)
        .to_string()
}

fn format_awk_printf(format: &str, args: &[String]) -> Result<String, String> {
    let mut output = String::new();
    let chars = format.chars().collect::<Vec<_>>();
    let mut cursor = 0usize;
    let mut arg_index = 0usize;
    while cursor < chars.len() {
        let ch = chars[cursor];
        cursor += 1;
        if ch != '%' {
            output.push(ch);
            continue;
        }
        if chars.get(cursor) == Some(&'%') {
            output.push('%');
            cursor += 1;
            continue;
        }
        let mut left_justify = false;
        let mut zero_pad = false;
        while let Some(flag) = chars.get(cursor).copied() {
            match flag {
                '-' => left_justify = true,
                '0' => zero_pad = true,
                _ => break,
            }
            cursor += 1;
        }
        let mut dynamic_width = false;
        let width = if chars.get(cursor) == Some(&'*') {
            cursor += 1;
            dynamic_width = true;
            None
        } else {
            let start = cursor;
            while chars.get(cursor).is_some_and(|ch| ch.is_ascii_digit()) {
                cursor += 1;
            }
            (cursor > start)
                .then(|| {
                    chars[start..cursor]
                        .iter()
                        .collect::<String>()
                        .parse::<isize>()
                })
                .transpose()
                .map_err(|_| "invalid printf format".to_string())?
        };
        let precision = if chars.get(cursor) == Some(&'.') {
            cursor += 1;
            let start = cursor;
            while chars.get(cursor).is_some_and(|ch| ch.is_ascii_digit()) {
                cursor += 1;
            }
            Some(if cursor > start {
                chars[start..cursor]
                    .iter()
                    .collect::<String>()
                    .parse::<usize>()
                    .map_err(|_| "invalid printf format".to_string())?
            } else {
                0
            })
        } else {
            None
        };
        while chars
            .get(cursor)
            .is_some_and(|ch| matches!(ch, 'h' | 'l' | 'L'))
        {
            cursor += 1;
        }
        let Some(specifier) = chars.get(cursor).copied() else {
            return Err("invalid printf format".to_string());
        };
        cursor += 1;
        let width = if dynamic_width {
            let value = args.get(arg_index).cloned().unwrap_or_default();
            arg_index += 1;
            Some(awk_to_number(&value) as isize)
        } else {
            width
        };
        let value = args.get(arg_index).cloned().unwrap_or_default();
        arg_index += 1;
        let formatted = match specifier {
            's' => value,
            'd' | 'i' => (awk_to_number(&value) as i64).to_string(),
            'f' => {
                let precision = precision.unwrap_or(6);
                format!("{:.*}", precision, awk_to_number(&value))
            }
            'x' => format!("{:x}", awk_to_number(&value) as i64),
            'X' => format!("{:X}", awk_to_number(&value) as i64),
            'o' => format!("{:o}", awk_to_number(&value) as i64),
            'c' => {
                let number = awk_to_number(&value) as u32;
                char::from_u32(number)
                    .map(|ch| ch.to_string())
                    .unwrap_or_default()
            }
            'e' => {
                let precision = precision.unwrap_or(6);
                format!("{:.*e}", precision, awk_to_number(&value))
            }
            _ => return Err("unsupported printf format".to_string()),
        };
        output.push_str(&apply_awk_printf_width(
            &formatted,
            width,
            left_justify,
            zero_pad && !left_justify,
        ));
    }
    Ok(output)
}

fn apply_awk_printf_width(
    value: &str,
    width: Option<isize>,
    left_justify: bool,
    zero_pad: bool,
) -> String {
    let Some(width) = width else {
        return value.to_string();
    };
    let left_justify = left_justify || width < 0;
    let width = width.unsigned_abs();
    let len = value.chars().count();
    if len >= width {
        return value.to_string();
    }
    let padding =
        std::iter::repeat_n(if zero_pad { '0' } else { ' ' }, width - len).collect::<String>();
    if left_justify {
        format!("{value}{padding}")
    } else {
        format!("{padding}{value}")
    }
}

fn parse_awk_regex_literal(source: &str, start: usize) -> Result<(String, usize), String> {
    let mut cursor = start + 1;
    while cursor < source.len() {
        if source.as_bytes()[cursor] == b'/' && !is_awk_escaped(source, cursor) {
            let pattern = &source[start + 1..cursor];
            Regex::new(pattern).map_err(|error| error.to_string())?;
            return Ok((pattern.to_string(), cursor + 1));
        }
        cursor += 1;
    }
    Err("unterminated regex pattern".to_string())
}

fn expect_awk_block(source: &str, cursor: usize) -> Result<usize, String> {
    if source.as_bytes().get(cursor) == Some(&b'{') {
        Ok(cursor)
    } else {
        Err("unsupported program".to_string())
    }
}

fn find_next_awk_block(source: &str, start: usize) -> Option<usize> {
    let mut cursor = start;
    let mut in_string = false;
    let mut in_regex = false;
    while cursor < source.len() {
        let ch = source.as_bytes()[cursor];
        if ch == b'"' && !is_awk_escaped(source, cursor) {
            in_string = !in_string;
        } else if !in_string && awk_regex_delimiter(source, cursor, in_regex) {
            in_regex = !in_regex;
        } else if ch == b'{' && !in_string && !in_regex {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

fn find_matching_awk_brace(source: &str, start: usize) -> Option<usize> {
    let mut cursor = start;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut in_regex = false;
    while cursor < source.len() {
        let ch = source.as_bytes()[cursor];
        if ch == b'"' && !is_awk_escaped(source, cursor) {
            in_string = !in_string;
        } else if !in_string && awk_regex_delimiter(source, cursor, in_regex) {
            in_regex = !in_regex;
        } else if !in_string && !in_regex {
            if ch == b'{' {
                depth += 1;
            } else if ch == b'}' {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(cursor);
                }
            }
        }
        cursor += 1;
    }
    None
}

fn find_awk_string_end(source: &str, start: usize) -> Option<usize> {
    let mut cursor = start + 1;
    while cursor < source.len() {
        if source.as_bytes()[cursor] == b'"' && !is_awk_escaped(source, cursor) {
            return Some(cursor);
        }
        cursor += 1;
    }
    None
}

fn find_awk_assignment(statement: &str) -> Option<(usize, AwkAssignOp)> {
    let mut cursor = 0usize;
    let mut in_string = false;
    let mut in_regex = false;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    while cursor < statement.len() {
        let ch = statement.as_bytes()[cursor];
        if ch == b'"' && !is_awk_escaped(statement, cursor) {
            in_string = !in_string;
        } else if !in_string {
            if awk_regex_delimiter(statement, cursor, in_regex) {
                in_regex = !in_regex;
            } else if !in_regex {
                match ch {
                    b'(' => paren_depth += 1,
                    b')' => paren_depth = paren_depth.saturating_sub(1),
                    b'[' => bracket_depth += 1,
                    b']' => bracket_depth = bracket_depth.saturating_sub(1),
                    b'=' if paren_depth == 0 && bracket_depth == 0 => {
                        let previous = cursor
                            .checked_sub(1)
                            .and_then(|index| statement.as_bytes().get(index));
                        let next = statement.as_bytes().get(cursor + 1);
                        if matches!(previous, Some(b'!' | b'<' | b'>' | b'='))
                            || next == Some(&b'=')
                        {
                            cursor += 1;
                            continue;
                        }
                        let op = match previous {
                            Some(b'+') => Some((cursor - 1, AwkAssignOp::Add)),
                            Some(b'-') => Some((cursor - 1, AwkAssignOp::Sub)),
                            Some(b'*') => Some((cursor - 1, AwkAssignOp::Mul)),
                            Some(b'/') => Some((cursor - 1, AwkAssignOp::Div)),
                            Some(b'%') => Some((cursor - 1, AwkAssignOp::Mod)),
                            Some(b'^') => Some((cursor - 1, AwkAssignOp::Pow)),
                            _ => Some((cursor, AwkAssignOp::Assign)),
                        };
                        return op;
                    }
                    _ => {}
                }
            }
        }
        cursor += 1;
    }
    None
}

fn split_awk_top_level(value: &str, delimiter: char) -> Vec<String> {
    let delimiter = delimiter as u8;
    let bytes = value.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut cursor = 0usize;
    let mut in_string = false;
    let mut in_regex = false;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' if !is_awk_escaped(value, cursor) => in_string = !in_string,
            b'/' if !in_string && awk_regex_delimiter(value, cursor, in_regex) => {
                in_regex = !in_regex
            }
            b'(' if !in_string && !in_regex => paren_depth += 1,
            b')' if !in_string && !in_regex => paren_depth = paren_depth.saturating_sub(1),
            b'[' if !in_string && !in_regex => bracket_depth += 1,
            b']' if !in_string && !in_regex => bracket_depth = bracket_depth.saturating_sub(1),
            ch if ch == delimiter
                && !in_string
                && !in_regex
                && paren_depth == 0
                && bracket_depth == 0 =>
            {
                parts.push(value[start..cursor].to_string());
                start = cursor + 1;
            }
            _ => {}
        }
        cursor += 1;
    }
    parts.push(value[start..].to_string());
    parts
}

fn split_awk_statements(value: &str) -> Vec<String> {
    let bytes = value.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut cursor = 0usize;
    let mut in_string = false;
    let mut in_regex = false;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' if !is_awk_escaped(value, cursor) => in_string = !in_string,
            b'/' if !in_string && awk_regex_delimiter(value, cursor, in_regex) => {
                in_regex = !in_regex
            }
            b'(' if !in_string && !in_regex => paren_depth += 1,
            b')' if !in_string && !in_regex => paren_depth = paren_depth.saturating_sub(1),
            b'[' if !in_string && !in_regex => bracket_depth += 1,
            b']' if !in_string && !in_regex => bracket_depth = bracket_depth.saturating_sub(1),
            b';' | b'\n' if !in_string && !in_regex && paren_depth == 0 && bracket_depth == 0 => {
                parts.push(value[start..cursor].to_string());
                start = cursor + 1;
            }
            _ => {}
        }
        cursor += 1;
    }
    parts.push(value[start..].to_string());
    parts
}

fn find_awk_top_level_char(value: &str, delimiter: char) -> Option<usize> {
    let delimiter = delimiter as u8;
    let bytes = value.as_bytes();
    let mut cursor = 0usize;
    let mut in_string = false;
    let mut in_regex = false;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' if !is_awk_escaped(value, cursor) => in_string = !in_string,
            b'/' if !in_string && awk_regex_delimiter(value, cursor, in_regex) => {
                in_regex = !in_regex
            }
            b'(' if !in_string && !in_regex => paren_depth += 1,
            b')' if !in_string && !in_regex => paren_depth = paren_depth.saturating_sub(1),
            b'[' if !in_string && !in_regex => bracket_depth += 1,
            b']' if !in_string && !in_regex => bracket_depth = bracket_depth.saturating_sub(1),
            ch if ch == delimiter
                && !in_string
                && !in_regex
                && paren_depth == 0
                && bracket_depth == 0 =>
            {
                return Some(cursor);
            }
            _ => {}
        }
        cursor += 1;
    }
    None
}

fn strip_awk_parentheses(value: &str) -> &str {
    let value = value.trim();
    if value.starts_with('(')
        && value.ends_with(')')
        && find_matching_awk_paren(value, 0) == Some(value.len() - 1)
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn find_matching_awk_paren(source: &str, start: usize) -> Option<usize> {
    let mut cursor = start;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut in_regex = false;
    while cursor < source.len() {
        let ch = source.as_bytes()[cursor];
        if ch == b'"' && !is_awk_escaped(source, cursor) {
            in_string = !in_string;
        } else if !in_string && awk_regex_delimiter(source, cursor, in_regex) {
            in_regex = !in_regex;
        } else if !in_string && !in_regex {
            if ch == b'(' {
                depth += 1;
            } else if ch == b')' {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(cursor);
                }
            }
        }
        cursor += 1;
    }
    None
}

fn find_matching_awk_bracket(source: &str, start: usize) -> Option<usize> {
    let mut cursor = start;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut in_regex = false;
    while cursor < source.len() {
        let ch = source.as_bytes()[cursor];
        if ch == b'"' && !is_awk_escaped(source, cursor) {
            in_string = !in_string;
        } else if !in_string && awk_regex_delimiter(source, cursor, in_regex) {
            in_regex = !in_regex;
        } else if !in_string && !in_regex {
            if ch == b'[' {
                depth += 1;
            } else if ch == b']' {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(cursor);
                }
            }
        }
        cursor += 1;
    }
    None
}

fn strip_awk_keyword<'a>(value: &'a str, keyword: &str) -> Option<&'a str> {
    if !value.starts_with(keyword) {
        return None;
    }
    let rest = &value[keyword.len()..];
    if rest
        .chars()
        .next()
        .is_some_and(|ch| ch == '_' || ch.is_ascii_alphanumeric())
    {
        return None;
    }
    Some(rest.trim_start())
}

fn starts_awk_keyword(source: &str, cursor: usize, keyword: &str) -> bool {
    source[cursor..].starts_with(keyword)
        && source[cursor + keyword.len()..]
            .chars()
            .next()
            .is_none_or(|ch| !(ch == '_' || ch.is_ascii_alphanumeric()))
}

fn skip_awk_whitespace(value: &str, start: usize) -> usize {
    let mut cursor = start;
    while value
        .as_bytes()
        .get(cursor)
        .is_some_and(u8::is_ascii_whitespace)
    {
        cursor += 1;
    }
    cursor
}

fn is_awk_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn take_awk_identifier(value: &str) -> Option<&str> {
    let mut end = 0usize;
    for (index, ch) in value.char_indices() {
        if index == 0 {
            if !(ch == '_' || ch.is_ascii_alphabetic()) {
                return None;
            }
        } else if !(ch == '_' || ch.is_ascii_alphanumeric()) {
            break;
        }
        end = index + ch.len_utf8();
    }
    Some(&value[..end])
}

fn take_awk_number(value: &str) -> Option<&str> {
    let mut seen_digit = false;
    let mut seen_dot = false;
    let mut end = 0usize;
    for (index, ch) in value.char_indices() {
        if ch.is_ascii_digit() {
            seen_digit = true;
            end = index + ch.len_utf8();
        } else if ch == '.' && !seen_dot {
            seen_dot = true;
            end = index + ch.len_utf8();
        } else {
            break;
        }
    }
    if seen_digit {
        Some(&value[..end])
    } else {
        None
    }
}

fn is_awk_escaped(value: &str, index: usize) -> bool {
    let mut backslashes = 0usize;
    let mut cursor = index;
    while cursor > 0 && value.as_bytes()[cursor - 1] == b'\\' {
        backslashes += 1;
        cursor -= 1;
    }
    backslashes % 2 == 1
}

fn awk_slash_starts_regex(value: &str, index: usize) -> bool {
    if value.as_bytes().get(index) != Some(&b'/') || is_awk_escaped(value, index) {
        return false;
    }
    let previous = value[..index].chars().rev().find(|ch| !ch.is_whitespace());
    previous.is_none_or(|ch| {
        matches!(
            ch,
            '(' | '{' | '[' | ',' | ';' | '=' | '!' | '~' | '<' | '>' | '&' | '|' | '?' | ':'
        )
    })
}

fn awk_regex_delimiter(value: &str, index: usize, in_regex: bool) -> bool {
    if value.as_bytes().get(index) != Some(&b'/') || is_awk_escaped(value, index) {
        return false;
    }
    in_regex || awk_slash_starts_regex(value, index)
}

fn awk_unescape_string(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut cursor = 0usize;
    while cursor < chars.len() {
        if chars[cursor] != '\\' {
            output.push(chars[cursor]);
            cursor += 1;
            continue;
        }
        cursor += 1;
        let Some(ch) = chars.get(cursor).copied() else {
            output.push('\\');
            break;
        };
        match ch {
            'n' => output.push('\n'),
            't' => output.push('\t'),
            'f' => output.push('\x0c'),
            'r' => output.push('\r'),
            'b' => output.push('\x08'),
            'v' => output.push('\x0b'),
            'a' => output.push('\x07'),
            '\\' => output.push('\\'),
            '"' => output.push('"'),
            'x' => {
                let mut hex = String::new();
                for offset in 1..=2 {
                    if let Some(next) = chars.get(cursor + offset)
                        && next.is_ascii_hexdigit()
                    {
                        hex.push(*next);
                    } else {
                        break;
                    }
                }
                if let Ok(value) = u8::from_str_radix(&hex, 16) {
                    output.push(value as char);
                    cursor += hex.len();
                } else {
                    output.push('x');
                }
            }
            '0'..='7' => {
                let mut octal = ch.to_string();
                for offset in 1..=2 {
                    if let Some(next @ '0'..='7') = chars.get(cursor + offset) {
                        octal.push(*next);
                    } else {
                        break;
                    }
                }
                if let Ok(value) = u8::from_str_radix(&octal, 8) {
                    output.push(value as char);
                    cursor += octal.len() - 1;
                }
            }
            other => output.push(other),
        }
        cursor += 1;
    }
    output
}

impl AwkSeparator {
    fn as_value(&self) -> String {
        match self {
            Self::Whitespace => " ".to_string(),
            Self::Pattern(pattern) => pattern.clone(),
        }
    }
}

fn awk_fields(line: &str, separator: &AwkSeparator) -> Vec<String> {
    match separator {
        AwkSeparator::Whitespace => line.split_whitespace().map(ToString::to_string).collect(),
        AwkSeparator::Pattern(separator) => {
            if separator.is_empty() {
                return line.chars().map(|ch| ch.to_string()).collect();
            }
            Regex::new(separator).map_or_else(
                |_| line.split(separator).map(ToString::to_string).collect(),
                |regex| regex.split(line).map(ToString::to_string).collect(),
            )
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TailLines {
    Last(usize),
    From(usize),
}

fn parse_tail_lines(value: &str) -> Option<TailLines> {
    value
        .strip_prefix('+')
        .and_then(|line| line.parse().ok().map(TailLines::From))
        .or_else(|| value.parse().ok().map(TailLines::Last))
}

fn join_lines_with_newline(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut output = lines.join("\n");
    output.push('\n');
    output
}

fn command_sort(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let mut options = SortOptions::default();
    let mut paths = Vec::new();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        match arg.as_str() {
            "--help" => {
                return stdout_result(
                    "Usage: sort [OPTION]... [FILE]...\n  -f, --ignore-case\n  -n, --numeric-sort\n  -r, --reverse\n  -u, --unique\n",
                );
            }
            "--ignore-case" => options.ignore_case = true,
            "-k" => {
                if let Some(key) = args.get(index + 1) {
                    options.key_field = parse_sort_key_field(key);
                    index += 2;
                    continue;
                }
            }
            _ if arg.starts_with("-k") && arg.len() > 2 => {
                options.key_field = parse_sort_key_field(&arg[2..]);
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                for flag in arg[1..].chars() {
                    match flag {
                        'r' => options.reverse = true,
                        'n' => options.numeric = true,
                        'u' => options.unique = true,
                        'f' => options.ignore_case = true,
                        _ => {}
                    }
                }
            }
            _ => paths.push(arg.clone()),
        }
        index += 1;
    }
    let input = match collect_text_inputs(state, &paths, stdin) {
        Ok(input) => input,
        Err(error) => return stderr_result(1, format!("sort: {error}\n")),
    };
    let mut lines = input.lines().map(ToString::to_string).collect::<Vec<_>>();
    lines.sort_by(|left, right| compare_sort_lines(left, right, &options));
    if options.unique {
        lines.dedup_by(|left, right| {
            sort_unique_key(left, &options) == sort_unique_key(right, &options)
        });
    }
    stdout_result(join_lines_with_newline(&lines))
}

#[derive(Clone, Copy, Debug, Default)]
struct SortOptions {
    reverse: bool,
    numeric: bool,
    unique: bool,
    ignore_case: bool,
    key_field: Option<usize>,
}

fn parse_sort_key_field(value: &str) -> Option<usize> {
    value
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

fn sort_key<'a>(line: &'a str, options: &SortOptions) -> &'a str {
    options.key_field.map_or(line, |field| {
        line.split_whitespace()
            .nth(field.saturating_sub(1))
            .unwrap_or("")
    })
}

fn compare_sort_lines(left: &str, right: &str, options: &SortOptions) -> CmpOrdering {
    let left_key = sort_key(left, options);
    let right_key = sort_key(right, options);
    let ordering = if options.numeric {
        let left_number = left_key.parse::<f64>().unwrap_or(0.0);
        let right_number = right_key.parse::<f64>().unwrap_or(0.0);
        left_number
            .partial_cmp(&right_number)
            .unwrap_or(CmpOrdering::Equal)
    } else {
        compare_text_keys(left_key, right_key, options.ignore_case)
    };
    let ordering = ordering.then_with(|| compare_text_keys(left, right, options.ignore_case));
    if options.reverse {
        ordering.reverse()
    } else {
        ordering
    }
}

fn compare_text_keys(left: &str, right: &str, ignore_case: bool) -> CmpOrdering {
    let left_key = left.to_ascii_lowercase();
    let right_key = right.to_ascii_lowercase();
    let ordering = left_key.cmp(&right_key);
    if ordering != CmpOrdering::Equal {
        return ordering;
    }
    if ignore_case {
        return CmpOrdering::Equal;
    }
    match (starts_lowercase(left), starts_lowercase(right)) {
        (true, false) => CmpOrdering::Less,
        (false, true) => CmpOrdering::Greater,
        _ => left.cmp(right),
    }
}

fn starts_lowercase(value: &str) -> bool {
    value.chars().next().is_some_and(char::is_lowercase)
}

fn sort_unique_key(line: &str, options: &SortOptions) -> String {
    let key = sort_key(line, options);
    if options.ignore_case {
        key.to_ascii_lowercase()
    } else {
        key.to_string()
    }
}

fn command_uniq(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let mut count = false;
    let mut duplicates_only = false;
    let mut unique_only = false;
    let mut ignore_case = false;
    let mut paths = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-c" | "--count" => count = true,
            "-d" | "--repeated" => duplicates_only = true,
            "-u" | "--unique" => unique_only = true,
            "-i" | "--ignore-case" => ignore_case = true,
            _ if arg.starts_with('-') && arg.len() > 1 => {
                for flag in arg[1..].chars() {
                    match flag {
                        'c' => count = true,
                        'd' => duplicates_only = true,
                        'u' => unique_only = true,
                        'i' => ignore_case = true,
                        _ => {}
                    }
                }
            }
            _ => paths.push(arg.clone()),
        }
    }
    let input = match collect_text_inputs(state, &paths, stdin) {
        Ok(input) => input,
        Err(error) => return stderr_result(1, format!("uniq: {error}\n")),
    };
    let mut stdout = String::new();
    let mut previous_line: Option<String> = None;
    let mut previous_key: Option<String> = None;
    let mut group_count = 0;
    for line in input.lines().map(ToString::to_string) {
        let key = uniq_compare_key(&line, ignore_case);
        if previous_key
            .as_ref()
            .is_some_and(|previous| previous == &key)
        {
            group_count += 1;
        } else {
            emit_uniq_group(
                &mut stdout,
                previous_line.take(),
                group_count,
                count,
                duplicates_only,
                unique_only,
            );
            previous_line = Some(line);
            previous_key = Some(key);
            group_count = 1;
        }
    }
    emit_uniq_group(
        &mut stdout,
        previous_line,
        group_count,
        count,
        duplicates_only,
        unique_only,
    );
    stdout_result(stdout)
}

fn uniq_compare_key(line: &str, ignore_case: bool) -> String {
    if ignore_case {
        line.to_lowercase()
    } else {
        line.to_string()
    }
}

fn emit_uniq_group(
    stdout: &mut String,
    line: Option<String>,
    group_count: usize,
    count: bool,
    duplicates_only: bool,
    unique_only: bool,
) {
    let Some(line) = line else {
        return;
    };
    if duplicates_only && group_count <= 1 {
        return;
    }
    if unique_only && group_count != 1 {
        return;
    }
    if count {
        stdout.push_str(&format!("{group_count:4} {line}\n"));
    } else {
        stdout.push_str(&line);
        stdout.push('\n');
    }
}

fn command_cut(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let mut delimiter = "\t".to_string();
    let mut fields = None;
    let mut chars = None;
    let mut paths = Vec::new();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        match arg.as_str() {
            "-d" => {
                if let Some(value) = args.get(index + 1) {
                    delimiter.clone_from(value);
                    index += 2;
                    continue;
                }
            }
            "-f" => {
                fields = args.get(index + 1).map(|value| parse_cut_list(value));
                index += 2;
                continue;
            }
            "-c" => {
                chars = args.get(index + 1).map(|value| parse_cut_list(value));
                index += 2;
                continue;
            }
            _ if arg.starts_with("-d") && arg.len() > 2 => delimiter = arg[2..].to_string(),
            _ if arg.starts_with("-f") && arg.len() > 2 => fields = Some(parse_cut_list(&arg[2..])),
            _ if arg.starts_with("-c") && arg.len() > 2 => chars = Some(parse_cut_list(&arg[2..])),
            _ => paths.push(arg.clone()),
        }
        index += 1;
    }
    if fields.is_none() && chars.is_none() {
        return stderr_result(
            1,
            "cut: you must specify a list of bytes, characters, or fields\n",
        );
    }
    let input = match collect_text_inputs(state, &paths, stdin) {
        Ok(input) => input,
        Err(error) => return stderr_result(1, format!("cut: {error}\n")),
    };
    let mut stdout = String::new();
    for line in input.lines() {
        if let Some(ranges) = &fields {
            let parts = line.split(&delimiter).collect::<Vec<_>>();
            let selected = selected_indexes(parts.len(), ranges)
                .into_iter()
                .filter_map(|index| parts.get(index).copied())
                .collect::<Vec<_>>();
            stdout.push_str(&selected.join(&delimiter));
        } else if let Some(ranges) = &chars {
            let characters = line.chars().collect::<Vec<_>>();
            for index in selected_indexes(characters.len(), ranges) {
                if let Some(ch) = characters.get(index) {
                    stdout.push(*ch);
                }
            }
        }
        stdout.push('\n');
    }
    stdout_result(stdout)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CutRange {
    start: usize,
    end: Option<usize>,
}

fn parse_cut_list(value: &str) -> Vec<CutRange> {
    value
        .split(',')
        .filter_map(|entry| {
            if let Some((start, end)) = entry.split_once('-') {
                let start = start.parse().ok()?;
                let end = if end.is_empty() {
                    None
                } else {
                    Some(end.parse().ok()?)
                };
                Some(CutRange { start, end })
            } else {
                entry.parse().ok().map(|index| CutRange {
                    start: index,
                    end: Some(index),
                })
            }
        })
        .collect()
}

fn selected_indexes(len: usize, ranges: &[CutRange]) -> Vec<usize> {
    let mut indexes = Vec::new();
    for range in ranges {
        let start = range.start.saturating_sub(1);
        let end = range.end.unwrap_or(len).min(len);
        for index in start..end {
            if index < len && !indexes.contains(&index) {
                indexes.push(index);
            }
        }
    }
    indexes
}

fn command_tr(args: &[String], stdin: &str) -> CommandResult {
    if args.is_empty() {
        return stderr_result(1, "tr: missing operand\n");
    }
    let mut delete = false;
    let mut squeeze = false;
    let mut complement = false;
    let mut operands = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-d" | "--delete" => delete = true,
            "-s" | "--squeeze-repeats" => squeeze = true,
            "-c" | "-C" | "--complement" => complement = true,
            _ if arg.starts_with('-') && arg.len() > 1 => {
                for flag in arg[1..].chars() {
                    match flag {
                        'd' => delete = true,
                        's' => squeeze = true,
                        'c' | 'C' => complement = true,
                        _ => {}
                    }
                }
            }
            _ => operands.push(arg.clone()),
        }
    }
    let Some(set1) = operands.first() else {
        return stderr_result(1, "tr: missing operand\n");
    };
    let set1 = expand_tr_set(set1);
    if delete {
        let mut output = stdin
            .chars()
            .filter(|ch| !tr_set_matches(*ch, &set1, complement))
            .collect::<String>();
        if squeeze && let Some(set2) = operands.get(1) {
            let set2 = expand_tr_set(set2);
            output = squeeze_tr_chars(&output, &set2, false);
        }
        return stdout_result(output);
    }
    let Some(set2) = operands.get(1) else {
        if squeeze {
            return stdout_result(squeeze_tr_chars(stdin, &set1, complement));
        }
        return stderr_result(1, "tr: missing operand after SET1\n");
    };
    let set2 = expand_tr_set(set2);
    let mut output = String::new();
    let mut previous_translated = None;
    for ch in stdin.chars() {
        if tr_set_matches(ch, &set1, complement) {
            let replacement = if complement {
                set2.first().copied().unwrap_or(ch)
            } else {
                let index = set1
                    .iter()
                    .position(|candidate| *candidate == ch)
                    .unwrap_or_default();
                set2.get(index)
                    .or_else(|| set2.last())
                    .copied()
                    .unwrap_or(ch)
            };
            if squeeze && Some(replacement) == previous_translated {
                continue;
            }
            output.push(replacement);
            previous_translated = Some(replacement);
        } else {
            output.push(ch);
            previous_translated = None;
        }
    }
    stdout_result(output)
}

fn tr_set_matches(ch: char, set: &[char], complement: bool) -> bool {
    set.contains(&ch) ^ complement
}

fn squeeze_tr_chars(input: &str, set: &[char], complement: bool) -> String {
    let mut output = String::new();
    let mut previous = None;
    for ch in input.chars() {
        if Some(ch) == previous && tr_set_matches(ch, set, complement) {
            continue;
        }
        output.push(ch);
        previous = Some(ch);
    }
    output
}

fn expand_tr_set(value: &str) -> Vec<char> {
    let chars = unescape_tr_set(value).chars().collect::<Vec<_>>();
    let mut expanded = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if let Some((class, next_index)) = parse_tr_posix_class(&chars, index) {
            expanded.extend(class);
            index = next_index;
            continue;
        }
        if index + 2 < chars.len() && chars[index + 1] == '-' {
            let start = chars[index] as u32;
            let end = chars[index + 2] as u32;
            if start <= end && end - start <= 65_536 {
                for value in start..=end {
                    if let Some(ch) = char::from_u32(value) {
                        expanded.push(ch);
                    }
                }
                index += 3;
                continue;
            }
        }
        expanded.push(chars[index]);
        index += 1;
    }
    expanded
}

fn parse_tr_posix_class(chars: &[char], index: usize) -> Option<(Vec<char>, usize)> {
    if chars.get(index) != Some(&'[') || chars.get(index + 1) != Some(&':') {
        return None;
    }
    let mut end = index + 2;
    while end + 1 < chars.len() {
        if chars[end] == ':' && chars[end + 1] == ']' {
            let name = chars[index + 2..end].iter().collect::<String>();
            return tr_posix_class_chars(&name).map(|class| (class, end + 2));
        }
        end += 1;
    }
    None
}

fn tr_posix_class_chars(name: &str) -> Option<Vec<char>> {
    let ranges = match name {
        "alnum" => vec![('0', '9'), ('A', 'Z'), ('a', 'z')],
        "alpha" => vec![('A', 'Z'), ('a', 'z')],
        "digit" => vec![('0', '9')],
        "lower" => vec![('a', 'z')],
        "upper" => vec![('A', 'Z')],
        "xdigit" => vec![('0', '9'), ('A', 'F'), ('a', 'f')],
        _ => return tr_named_posix_class_chars(name),
    };
    Some(
        ranges
            .into_iter()
            .flat_map(|(start, end)| (start as u32..=end as u32).filter_map(char::from_u32))
            .collect(),
    )
}

fn tr_named_posix_class_chars(name: &str) -> Option<Vec<char>> {
    let chars = match name {
        "blank" => vec!['\t', ' '],
        "space" => vec!['\t', '\n', '\x0b', '\x0c', '\r', ' '],
        "punct" => (b'!'..=b'/')
            .chain(b':'..=b'@')
            .chain(b'['..=b'`')
            .chain(b'{'..=b'~')
            .map(char::from)
            .collect(),
        _ => return None,
    };
    Some(chars)
}

fn unescape_tr_set(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => output.push('\n'),
            Some('t') => output.push('\t'),
            Some('r') => output.push('\r'),
            Some(other) => output.push(other),
            None => output.push('\\'),
        }
    }
    output
}

fn command_read(state: &mut ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let Some(name) = args.first() else {
        return CommandResult::default();
    };
    let value = stdin.lines().next().unwrap_or_default().to_string();
    state.env.insert(name.clone(), value);
    CommandResult::default()
}

#[derive(Clone, Debug, Default)]
struct JqOptions {
    filter: String,
    paths: Vec<String>,
    raw_output: bool,
    compact: bool,
    null_input: bool,
    slurp: bool,
    exit_status: bool,
    join_output: bool,
    tab_indent: bool,
}

fn command_jq(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let options = match parse_jq_options(args) {
        Ok(options) => options,
        Err(result) => return result,
    };
    if options.filter == "__help__" {
        return stdout_result(
            "jq - commandline JSON processor\nUsage: jq [options] <filter> [file...]\n",
        );
    }
    let inputs = match collect_jq_inputs(state, &options, stdin) {
        Ok(inputs) => inputs,
        Err(result) => return result,
    };
    let mut stdout = String::new();
    let mut last_value = JsonValue::Null;
    let mut saw_value = false;
    for input in inputs {
        let selected = match eval_structured_filter(&input, &input, &options.filter, None) {
            Ok(selected) => selected,
            Err(error) => return stderr_result(3, format!("jq: {error}\n")),
        };
        for value in selected {
            saw_value = true;
            last_value = value.clone();
            stdout.push_str(&render_json_output(
                &value,
                StructuredOutput {
                    raw: options.raw_output,
                    compact: options.compact,
                    tab_indent: options.tab_indent,
                    indent: 2,
                    default_scalar_raw: false,
                },
            ));
            if !options.join_output {
                stdout.push('\n');
            }
        }
    }
    let mut result = stdout_result(stdout);
    if options.exit_status && (!saw_value || !is_truthy(&last_value)) {
        result.exit_code = 1;
    }
    result
}

fn parse_jq_options(args: &[String]) -> Result<JqOptions, CommandResult> {
    let mut options = JqOptions::default();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if !options.filter.is_empty() {
            options.paths.push(arg.clone());
            index += 1;
            continue;
        }
        if arg == "--help" {
            options.filter = "__help__".to_string();
            return Ok(options);
        }
        match arg.as_str() {
            "--raw-output" => options.raw_output = true,
            "--tab" => options.tab_indent = true,
            "-r" => options.raw_output = true,
            "-c" => options.compact = true,
            "-n" => options.null_input = true,
            "-s" => options.slurp = true,
            "-S" => {}
            "-e" => options.exit_status = true,
            "-j" => options.join_output = true,
            _ if arg.starts_with("--") => {
                return Err(stderr_result(
                    1,
                    format!("jq: unrecognized option '{arg}'\n"),
                ));
            }
            _ if arg.starts_with('-') && arg.len() > 2 => {
                for flag in arg[1..].chars() {
                    match flag {
                        'r' => options.raw_output = true,
                        'c' => options.compact = true,
                        'n' => options.null_input = true,
                        's' => options.slurp = true,
                        'S' => {}
                        'e' => options.exit_status = true,
                        'j' => options.join_output = true,
                        other => {
                            return Err(stderr_result(
                                1,
                                format!("jq: invalid option -- '{other}'\n"),
                            ));
                        }
                    }
                }
            }
            _ if arg.starts_with('-') && arg != "-" => {
                let flag = arg.trim_start_matches('-');
                return Err(stderr_result(
                    1,
                    format!("jq: invalid option -- '{flag}'\n"),
                ));
            }
            _ => options.filter = arg.clone(),
        }
        index += 1;
    }
    if options.filter.is_empty() {
        return Err(stderr_result(2, "jq: missing filter\n"));
    }
    Ok(options)
}

fn collect_jq_inputs(
    state: &ExecState<'_>,
    options: &JqOptions,
    stdin: &str,
) -> Result<Vec<JsonValue>, CommandResult> {
    if options.null_input {
        return if options.slurp {
            Ok(vec![JsonValue::Array(Vec::new())])
        } else {
            Ok(vec![JsonValue::Null])
        };
    }
    let mut values = Vec::new();
    if options.paths.is_empty() {
        values.extend(
            parse_json_stream(stdin)
                .map_err(|error| stderr_result(5, format!("jq: parse error: {error}\n")))?,
        );
    } else {
        let fs = state
            .session
            .inner
            .fs
            .lock()
            .map_err(|_| stderr_result(1, "jq: filesystem lock poisoned\n"))?;
        for raw_path in &options.paths {
            let text = if raw_path == "-" {
                stdin.to_string()
            } else {
                let path = resolve_path(&state.cwd, raw_path);
                fs.read_file(&path).map_err(|_| {
                    stderr_result(2, format!("jq: {path}: No such file or directory\n"))
                })?
            };
            if text.trim().is_empty() {
                continue;
            }
            values.extend(
                parse_json_stream(&text)
                    .map_err(|error| stderr_result(5, format!("jq: parse error: {error}\n")))?,
            );
        }
    }
    if options.slurp {
        Ok(vec![JsonValue::Array(values)])
    } else {
        Ok(values)
    }
}

fn parse_json_stream(input: &str) -> Result<Vec<JsonValue>, String> {
    let mut values = Vec::new();
    let mut rest = input.trim_start();
    while !rest.is_empty() {
        let mut stream = serde_json::Deserializer::from_str(rest).into_iter::<JsonValue>();
        let Some(next) = stream.next() else {
            break;
        };
        let value = next.map_err(|error| format!("{error}"))?;
        let offset = stream.byte_offset();
        if offset == 0 {
            return Err("invalid JSON stream".to_string());
        }
        values.push(value);
        rest = rest[offset..].trim_start();
    }
    Ok(values)
}

#[derive(Clone, Copy, Debug)]
struct StructuredOutput {
    raw: bool,
    compact: bool,
    tab_indent: bool,
    indent: usize,
    default_scalar_raw: bool,
}

fn render_json_output(value: &JsonValue, options: StructuredOutput) -> String {
    if options.raw || options.default_scalar_raw {
        match value {
            JsonValue::String(value) => return value.clone(),
            JsonValue::Null if options.raw => return "null".to_string(),
            JsonValue::Bool(_) | JsonValue::Number(_) if options.raw => return value.to_string(),
            _ if options.raw => {}
            _ => return value.to_string(),
        }
    }
    if options.compact {
        return serde_json::to_string(value).unwrap_or_else(|_| value.to_string());
    }
    match value {
        JsonValue::Array(_) | JsonValue::Object(_) => render_pretty_json_output(value, options),
        _ => value.to_string(),
    }
}

fn render_pretty_json_output(value: &JsonValue, options: StructuredOutput) -> String {
    let rendered = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    if options.tab_indent {
        return rendered.replace("  ", "\t");
    }
    if options.indent == 2 {
        return rendered;
    }
    rendered
        .lines()
        .map(|line| {
            let leading_spaces = line.chars().take_while(|ch| *ch == ' ').count();
            let level = leading_spaces / 2;
            format!(
                "{}{}",
                " ".repeat(level * options.indent),
                &line[leading_spaces..]
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn eval_structured_filter(
    value: &JsonValue,
    root: &JsonValue,
    filter: &str,
    env: Option<&BTreeMap<String, String>>,
) -> Result<Vec<JsonValue>, String> {
    let filter = filter.trim();
    if filter.is_empty() {
        return Ok(vec![value.clone()]);
    }
    let mut output = Vec::new();
    for branch in split_top_level(filter, ',') {
        let mut current = vec![value.clone()];
        for segment in split_top_level(branch, '|') {
            let mut next = Vec::new();
            for value in &current {
                next.extend(eval_structured_expr(value, root, segment.trim(), env)?);
            }
            current = next;
        }
        output.extend(current);
    }
    Ok(output)
}

fn eval_structured_expr(
    value: &JsonValue,
    root: &JsonValue,
    expr: &str,
    env: Option<&BTreeMap<String, String>>,
) -> Result<Vec<JsonValue>, String> {
    let expr = trim_outer_parens(expr.trim());
    if expr.is_empty() {
        return Ok(vec![value.clone()]);
    }
    if expr == "empty" {
        return Ok(Vec::new());
    }
    if expr == ".." {
        return Ok(json_recursive_values(value));
    }
    if expr == "numbers" {
        return if value.is_number() {
            Ok(vec![value.clone()])
        } else {
            Ok(Vec::new())
        };
    }
    if let Some(tail) = expr.strip_prefix("(.).") {
        let selector = format!(".{tail}");
        return eval_path_selector(value, &selector);
    }
    if let Some(inner) = function_arg(expr, "limit") {
        let args = split_top_level(inner, ';');
        if args.len() == 2 {
            let limit = args[0].trim().parse::<usize>().unwrap_or(0);
            return Ok(eval_structured_filter(value, root, args[1], env)?
                .into_iter()
                .take(limit)
                .collect());
        }
    }
    if let Some(formatter) = expr.strip_prefix('@') {
        return Ok(vec![format_yq_value(value, formatter)]);
    }
    if let Some(env_expr) = expr
        .strip_prefix("env.")
        .or_else(|| expr.strip_prefix("$ENV."))
    {
        return Ok(vec![
            env.and_then(|env| env.get(env_expr))
                .map(|value| JsonValue::String(value.clone()))
                .unwrap_or(JsonValue::Null),
        ]);
    }
    if expr == "env" || expr == "$ENV" {
        let object = env
            .map(|env| {
                env.iter()
                    .map(|(key, value)| (key.clone(), JsonValue::String(value.clone())))
                    .collect::<JsonMap<_, _>>()
            })
            .unwrap_or_default();
        return Ok(vec![JsonValue::Object(object)]);
    }
    if let Some(result) = eval_conditional_expr(value, root, expr, env)? {
        return Ok(vec![result]);
    }
    if let Some(inner) = function_arg(expr, "range") {
        return Ok(json_range_values(inner));
    }
    if let Some((left, op, right)) = split_binary_expr(expr, &["//", " or ", " and "]) {
        let left_value = eval_first(value, root, left, env)?;
        return Ok(vec![match op.trim() {
            "//" => {
                if matches!(left_value, JsonValue::Null | JsonValue::Bool(false)) {
                    eval_first(value, root, right, env)?
                } else {
                    left_value
                }
            }
            "or" => JsonValue::Bool(
                is_truthy(&left_value) || is_truthy(&eval_first(value, root, right, env)?),
            ),
            "and" => JsonValue::Bool(
                is_truthy(&left_value) && is_truthy(&eval_first(value, root, right, env)?),
            ),
            _ => unreachable!(),
        }]);
    }
    if let Some((left, op, right)) = split_binary_expr(expr, &["==", "!=", "<=", ">=", "<", ">"]) {
        let left_value = eval_first(value, root, left, env)?;
        let right_value = eval_first(value, root, right, env)?;
        return Ok(vec![JsonValue::Bool(compare_json(
            &left_value,
            &right_value,
            op,
        ))]);
    }
    if let Some((left, op, right)) = split_binary_expr(expr, &[" + ", " - "]) {
        let left_value = eval_first(value, root, left, env)?;
        let right_value = eval_first(value, root, right, env)?;
        return Ok(vec![apply_json_arithmetic(
            &left_value,
            &right_value,
            op.trim(),
        )?]);
    }
    if let Some((left, op, right)) = split_binary_expr(expr, &[" * ", " / ", " % "]) {
        let left_value = eval_first(value, root, left, env)?;
        let right_value = eval_first(value, root, right, env)?;
        return Ok(vec![apply_json_arithmetic(
            &left_value,
            &right_value,
            op.trim(),
        )?]);
    }
    if expr.starts_with('[') && expr.ends_with(']') {
        let inner = &expr[1..expr.len() - 1];
        if inner.trim().is_empty() {
            return Ok(vec![JsonValue::Array(Vec::new())]);
        }
        let mut values = Vec::new();
        for part in split_top_level(inner, ',') {
            values.extend(eval_structured_filter(value, root, part.trim(), env)?);
        }
        return Ok(vec![JsonValue::Array(values)]);
    }
    if expr.starts_with('{') && expr.ends_with('}') {
        return Ok(vec![eval_object_construction(value, root, expr, env)?]);
    }
    if let Ok(literal) = serde_json::from_str::<JsonValue>(expr) {
        return Ok(vec![literal]);
    }
    if let Some(inner) = function_arg(expr, "select") {
        return if is_truthy(&eval_first(value, root, inner, env)?) {
            Ok(vec![value.clone()])
        } else {
            Ok(Vec::new())
        };
    }
    if let Some(result) = eval_function(value, root, expr, env)? {
        return Ok(vec![result]);
    }
    if expr == "not" {
        return Ok(vec![JsonValue::Bool(!is_truthy(value))]);
    }
    if expr.starts_with('.') {
        return eval_path_selector(value, expr);
    }
    Err(format!("unsupported filter '{expr}'"))
}

fn eval_first(
    value: &JsonValue,
    root: &JsonValue,
    expr: &str,
    env: Option<&BTreeMap<String, String>>,
) -> Result<JsonValue, String> {
    Ok(eval_structured_filter(value, root, expr, env)?
        .into_iter()
        .next()
        .unwrap_or(JsonValue::Null))
}

fn eval_conditional_expr(
    value: &JsonValue,
    root: &JsonValue,
    expr: &str,
    env: Option<&BTreeMap<String, String>>,
) -> Result<Option<JsonValue>, String> {
    if !expr.starts_with("if ") || !expr.ends_with(" end") {
        return Ok(None);
    }
    let body = &expr[3..expr.len() - 4];
    let Some((condition, rest)) = body.split_once(" then ") else {
        return Err("invalid if expression".to_string());
    };
    if let Some((then_expr, else_expr)) = rest.split_once(" else ") {
        let selected = if is_truthy(&eval_first(value, root, condition, env)?) {
            then_expr
        } else {
            else_expr
        };
        return Ok(Some(eval_first(value, root, selected, env)?));
    }
    Ok(None)
}

fn eval_object_construction(
    value: &JsonValue,
    root: &JsonValue,
    expr: &str,
    env: Option<&BTreeMap<String, String>>,
) -> Result<JsonValue, String> {
    let mut object = JsonMap::new();
    let inner = &expr[1..expr.len() - 1];
    for entry in split_top_level(inner, ',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if let Some((raw_key, raw_value)) = split_object_entry(entry) {
            let key = object_key(value, root, raw_key.trim(), env)?;
            let value = eval_first(value, root, raw_value.trim(), env)?;
            insert_json_object_key(&mut object, key, value);
        } else {
            let key = entry.trim().trim_matches('"').to_string();
            let value = eval_first(value, root, &format!(".{key}"), env)?;
            insert_json_object_key(&mut object, key, value);
        }
    }
    Ok(JsonValue::Object(object))
}

fn split_object_entry(entry: &str) -> Option<(&str, &str)> {
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escape = false;
    for (index, ch) in entry.char_indices() {
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ':' if depth == 0 => return Some((&entry[..index], &entry[index + 1..])),
            _ => {}
        }
    }
    None
}

fn is_safe_json_object_key(key: &str) -> bool {
    !matches!(
        key,
        "__proto__"
            | "constructor"
            | "prototype"
            | "__defineGetter__"
            | "__defineSetter__"
            | "__lookupGetter__"
            | "__lookupSetter__"
            | "hasOwnProperty"
            | "isPrototypeOf"
            | "propertyIsEnumerable"
            | "toLocaleString"
            | "toString"
            | "valueOf"
    )
}

fn insert_json_object_key(object: &mut JsonMap<String, JsonValue>, key: String, value: JsonValue) {
    if is_safe_json_object_key(&key) {
        object.insert(key, value);
    }
}

fn object_key(
    value: &JsonValue,
    root: &JsonValue,
    raw_key: &str,
    env: Option<&BTreeMap<String, String>>,
) -> Result<String, String> {
    if raw_key.starts_with('(') && raw_key.ends_with(')') {
        return Ok(
            match eval_first(value, root, &raw_key[1..raw_key.len() - 1], env)? {
                JsonValue::String(value) => value,
                other => other.to_string(),
            },
        );
    }
    Ok(raw_key.trim_matches('"').to_string())
}

fn eval_function(
    value: &JsonValue,
    root: &JsonValue,
    expr: &str,
    env: Option<&BTreeMap<String, String>>,
) -> Result<Option<JsonValue>, String> {
    match expr {
        "type" => return Ok(Some(JsonValue::String(json_type(value).to_string()))),
        "length" => return Ok(Some(json_number(json_length(value) as f64))),
        "keys" => return Ok(Some(json_keys(value))),
        "first" => return Ok(Some(json_first(value))),
        "last" => return Ok(Some(json_last(value))),
        "reverse" => return Ok(Some(json_reverse(value))),
        "sort" => return Ok(Some(json_sort(value))),
        "unique" => return Ok(Some(json_unique(value))),
        "add" => return Ok(Some(json_add(value)?)),
        "min" => return Ok(Some(json_minmax(value, false))),
        "max" => return Ok(Some(json_minmax(value, true))),
        "floor" => return Ok(Some(json_number(value.as_f64().unwrap_or(0.0).floor()))),
        "ceil" => return Ok(Some(json_number(value.as_f64().unwrap_or(0.0).ceil()))),
        "round" => return Ok(Some(json_number(value.as_f64().unwrap_or(0.0).round()))),
        "sqrt" => return Ok(Some(json_number(value.as_f64().unwrap_or(0.0).sqrt()))),
        "abs" => return Ok(Some(json_number(value.as_f64().unwrap_or(0.0).abs()))),
        "tostring" => {
            return Ok(Some(JsonValue::String(match value {
                JsonValue::String(value) => value.clone(),
                other => other.to_string(),
            })));
        }
        "tonumber" => {
            return Ok(Some(match value {
                JsonValue::String(value) => value
                    .parse::<f64>()
                    .map(json_number)
                    .unwrap_or(JsonValue::Null),
                JsonValue::Number(_) => value.clone(),
                _ => JsonValue::Null,
            }));
        }
        "flatten" => return Ok(Some(json_flatten(value, usize::MAX))),
        "to_entries" => return Ok(Some(json_to_entries(value))),
        "from_entries" => return Ok(Some(json_from_entries(value))),
        "transpose" => return Ok(Some(json_transpose(value))),
        _ => {}
    }
    if let Some(inner) = function_arg(expr, "with_entries") {
        let JsonValue::Array(entries) = json_to_entries(value) else {
            return Ok(Some(JsonValue::Object(JsonMap::new())));
        };
        let mut mapped = Vec::new();
        for entry in entries {
            mapped.extend(eval_structured_filter(&entry, root, inner, env)?);
        }
        return Ok(Some(json_from_entries(&JsonValue::Array(mapped))));
    }
    if let Some(inner) = function_arg(expr, "getpath") {
        let path = parse_json_path_arg(inner);
        return Ok(Some(json_get_path(value, &path)));
    }
    if let Some(inner) = function_arg(expr, "setpath") {
        let args = split_top_level(inner, ';');
        if args.len() == 2 {
            let path = parse_json_path_arg(args[0]);
            let replacement = eval_first(value, root, args[1], env)?;
            return Ok(Some(json_set_path(value, &path, replacement)));
        }
    }
    if let Some(inner) = function_arg(expr, "pow") {
        let args = split_top_level(inner, ';');
        if args.len() == 2 {
            let base = eval_first(value, root, args[0], env)?;
            let exponent = eval_first(value, root, args[1], env)?;
            return Ok(Some(match (base.as_f64(), exponent.as_f64()) {
                (Some(base), Some(exponent)) => json_number(base.powf(exponent)),
                _ => JsonValue::Null,
            }));
        }
    }
    if let Some(inner) = function_arg(expr, "atan2") {
        let args = split_top_level(inner, ';');
        if args.len() == 2 {
            let y = eval_first(value, root, args[0], env)?;
            let x = eval_first(value, root, args[1], env)?;
            return Ok(Some(match (y.as_f64(), x.as_f64()) {
                (Some(y), Some(x)) => json_number(y.atan2(x)),
                _ => JsonValue::Null,
            }));
        }
    }
    if let Some(inner) = function_arg(expr, "map") {
        let JsonValue::Array(values) = value else {
            return Ok(Some(JsonValue::Array(Vec::new())));
        };
        let mut mapped = Vec::new();
        for item in values {
            mapped.extend(eval_structured_filter(item, root, inner, env)?);
        }
        return Ok(Some(JsonValue::Array(mapped)));
    }
    if let Some(inner) = function_arg(expr, "has") {
        let key = eval_first(value, root, inner, env)?;
        return Ok(Some(JsonValue::Bool(match (value, key) {
            (JsonValue::Object(map), JsonValue::String(key)) => map.contains_key(&key),
            (JsonValue::Array(values), JsonValue::Number(index)) => index
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .is_some_and(|index| index < values.len()),
            _ => false,
        })));
    }
    if let Some(inner) = function_arg(expr, "contains") {
        let needle = eval_first(value, root, inner, env)?;
        return Ok(Some(JsonValue::Bool(json_contains(value, &needle))));
    }
    if let Some(inner) = function_arg(expr, "any") {
        let JsonValue::Array(values) = value else {
            return Ok(Some(JsonValue::Bool(false)));
        };
        return Ok(Some(JsonValue::Bool(values.iter().any(|item| {
            eval_first(item, root, inner, env).is_ok_and(|value| is_truthy(&value))
        }))));
    }
    if let Some(inner) = function_arg(expr, "all") {
        let JsonValue::Array(values) = value else {
            return Ok(Some(JsonValue::Bool(false)));
        };
        return Ok(Some(JsonValue::Bool(values.iter().all(|item| {
            eval_first(item, root, inner, env).is_ok_and(|value| is_truthy(&value))
        }))));
    }
    if let Some(inner) = function_arg(expr, "sort_by") {
        return Ok(Some(json_sort_by(value, root, inner, env)));
    }
    if let Some(inner) = function_arg(expr, "min_by") {
        return Ok(Some(json_minmax_by(value, root, inner, env, false)));
    }
    if let Some(inner) = function_arg(expr, "max_by") {
        return Ok(Some(json_minmax_by(value, root, inner, env, true)));
    }
    if let Some(inner) = function_arg(expr, "unique_by") {
        return Ok(Some(json_unique_by(value, root, inner, env)));
    }
    if let Some(inner) = function_arg(expr, "group_by") {
        return Ok(Some(json_group_by(value, root, inner, env)));
    }
    if let Some(inner) = function_arg(expr, "flatten") {
        let depth = inner.trim().parse::<usize>().unwrap_or(usize::MAX);
        return Ok(Some(json_flatten(value, depth)));
    }
    if let Some(inner) = function_arg(expr, "split") {
        let separator = string_arg(inner);
        return Ok(Some(JsonValue::Array(
            value
                .as_str()
                .unwrap_or_default()
                .split(&separator)
                .map(|part| JsonValue::String(part.to_string()))
                .collect(),
        )));
    }
    if let Some(inner) = function_arg(expr, "join") {
        let separator = string_arg(inner);
        let JsonValue::Array(values) = value else {
            return Ok(Some(JsonValue::String(String::new())));
        };
        return Ok(Some(JsonValue::String(
            values
                .iter()
                .map(json_scalar_string)
                .collect::<Vec<_>>()
                .join(&separator),
        )));
    }
    if let Some(inner) = function_arg(expr, "test") {
        let pattern = string_arg(inner);
        let regex = Regex::new(&pattern).map_err(|error| error.to_string())?;
        return Ok(Some(JsonValue::Bool(
            value.as_str().is_some_and(|value| regex.is_match(value)),
        )));
    }
    if let Some(inner) = function_arg(expr, "startswith") {
        let prefix = string_arg(inner);
        return Ok(Some(JsonValue::Bool(
            value
                .as_str()
                .is_some_and(|value| value.starts_with(&prefix)),
        )));
    }
    if let Some(inner) = function_arg(expr, "endswith") {
        let suffix = string_arg(inner);
        return Ok(Some(JsonValue::Bool(
            value.as_str().is_some_and(|value| value.ends_with(&suffix)),
        )));
    }
    if let Some(inner) = function_arg(expr, "ltrimstr") {
        let prefix = string_arg(inner);
        return Ok(Some(JsonValue::String(
            value
                .as_str()
                .unwrap_or_default()
                .strip_prefix(&prefix)
                .unwrap_or_else(|| value.as_str().unwrap_or_default())
                .to_string(),
        )));
    }
    if let Some(inner) = function_arg(expr, "rtrimstr") {
        let suffix = string_arg(inner);
        return Ok(Some(JsonValue::String(
            value
                .as_str()
                .unwrap_or_default()
                .strip_suffix(&suffix)
                .unwrap_or_else(|| value.as_str().unwrap_or_default())
                .to_string(),
        )));
    }
    if expr == "ascii_downcase" {
        return Ok(Some(JsonValue::String(
            value.as_str().unwrap_or_default().to_ascii_lowercase(),
        )));
    }
    if expr == "ascii_upcase" {
        return Ok(Some(JsonValue::String(
            value.as_str().unwrap_or_default().to_ascii_uppercase(),
        )));
    }
    if let Some(inner) = function_arg(expr, "sub") {
        let args = split_top_level(inner, ';');
        if args.len() == 2 {
            let pattern = string_arg(args[0]);
            let replacement = string_arg(args[1]);
            return Ok(Some(JsonValue::String(
                value
                    .as_str()
                    .unwrap_or_default()
                    .replacen(&pattern, &replacement, 1),
            )));
        }
    }
    if let Some(inner) = function_arg(expr, "gsub") {
        let args = split_top_level(inner, ';');
        if args.len() == 2 {
            let pattern = string_arg(args[0]);
            let replacement = string_arg(args[1]);
            return Ok(Some(JsonValue::String(
                value
                    .as_str()
                    .unwrap_or_default()
                    .replace(&pattern, &replacement),
            )));
        }
    }
    if let Some(inner) = function_arg(expr, "index") {
        let needle = string_arg(inner);
        return Ok(Some(
            value
                .as_str()
                .and_then(|value| value.find(&needle))
                .map_or(JsonValue::Null, |index| json_number(index as f64)),
        ));
    }
    if let Some(inner) = function_arg(expr, "indices") {
        let needle = string_arg(inner);
        let mut indexes = Vec::new();
        if !needle.is_empty() {
            let mut rest = value.as_str().unwrap_or_default();
            let mut offset = 0;
            while let Some(index) = rest.find(&needle) {
                indexes.push(json_number((offset + index) as f64));
                offset += index + needle.len();
                rest = &rest[index + needle.len()..];
            }
        }
        return Ok(Some(JsonValue::Array(indexes)));
    }
    if let Some(inner) = function_arg(expr, "first") {
        return Ok(Some(
            eval_structured_filter(value, root, inner, env)?
                .into_iter()
                .next()
                .unwrap_or(JsonValue::Null),
        ));
    }
    Ok(None)
}

fn eval_path_selector(value: &JsonValue, selector: &str) -> Result<Vec<JsonValue>, String> {
    if selector == "." {
        return Ok(vec![value.clone()]);
    }
    if has_invalid_dot_whitespace(selector) {
        return Err("invalid field selector".to_string());
    }
    if selector == ".[]" {
        return Ok(json_iter_values(value));
    }
    let mut current = value.clone();
    let mut rest = selector
        .strip_prefix('.')
        .ok_or_else(|| "unsupported filter".to_string())?;
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest == "[]" {
            return Ok(json_iter_values(&current));
        }
        if let Some((inside, tail)) = rest.strip_prefix('[').and_then(|tail| tail.split_once(']')) {
            let inside = inside.trim();
            current = if inside.contains(':') {
                json_slice(&current, inside)
            } else {
                json_index_or_field(&current, inside.trim_matches('"')).unwrap_or(JsonValue::Null)
            };
            rest = tail.strip_prefix('.').unwrap_or(tail);
            continue;
        }
        let quoted = rest.starts_with('"');
        let (field, tail) = if quoted {
            let Some(end) = rest[1..].find('"') else {
                return Err("unterminated field string".to_string());
            };
            (&rest[1..1 + end], &rest[1 + end + 1..])
        } else {
            let split = rest.find(['.', '[']).unwrap_or(rest.len());
            (&rest[..split], &rest[split..])
        };
        let field = field.strip_suffix('?').unwrap_or(field);
        if let Some(field) = field.strip_suffix("[]") {
            current = current.get(field).cloned().unwrap_or(JsonValue::Null);
            return Ok(json_iter_values(&current));
        }
        if field.is_empty() {
            return Err("empty field selector".to_string());
        }
        current = current.get(field).cloned().unwrap_or(JsonValue::Null);
        rest = tail.strip_prefix('.').unwrap_or(tail);
    }
    Ok(vec![current])
}

fn has_invalid_dot_whitespace(selector: &str) -> bool {
    for (index, ch) in selector.char_indices() {
        if ch != '.' {
            continue;
        }
        let tail = &selector[index + ch.len_utf8()..];
        if !tail.chars().next().is_some_and(char::is_whitespace) {
            continue;
        }
        let next = tail.trim_start().chars().next();
        if next != Some('"') {
            return true;
        }
    }
    false
}

fn json_iter_values(value: &JsonValue) -> Vec<JsonValue> {
    match value {
        JsonValue::Array(values) => values.clone(),
        JsonValue::Object(values) => values.values().cloned().collect(),
        _ => Vec::new(),
    }
}

fn json_index_or_field(value: &JsonValue, index: &str) -> Option<JsonValue> {
    if let JsonValue::Array(values) = value
        && let Ok(index) = index.parse::<isize>()
    {
        let index = if index < 0 {
            values.len().checked_sub(index.unsigned_abs())?
        } else {
            index as usize
        };
        return values.get(index).cloned();
    }
    value.get(index).cloned()
}

fn json_slice(value: &JsonValue, spec: &str) -> JsonValue {
    let (start, end) = spec.split_once(':').unwrap_or((spec, ""));
    match value {
        JsonValue::Array(values) => {
            let start = slice_bound(start, values.len(), 0);
            let end = slice_bound(end, values.len(), values.len());
            JsonValue::Array(values[start.min(end)..end.min(values.len())].to_vec())
        }
        JsonValue::String(value) => {
            let chars = value.chars().collect::<Vec<_>>();
            let start = slice_bound(start, chars.len(), 0);
            let end = slice_bound(end, chars.len(), chars.len());
            JsonValue::String(chars[start.min(end)..end.min(chars.len())].iter().collect())
        }
        _ => JsonValue::Null,
    }
}

fn slice_bound(raw: &str, len: usize, default: usize) -> usize {
    let raw = raw.trim();
    if raw.is_empty() {
        return default;
    }
    let Ok(index) = raw.parse::<isize>() else {
        return default;
    };
    if index < 0 {
        len.saturating_sub(index.unsigned_abs())
    } else {
        (index as usize).min(len)
    }
}

fn split_top_level(input: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut in_single_string = false;
    let mut escape = false;
    for (index, ch) in input.char_indices() {
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        if in_single_string {
            if ch == '\'' {
                in_single_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '\'' => in_single_string = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if ch == separator && depth == 0 => {
                parts.push(input[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(input[start..].trim());
    parts
}

fn split_binary_expr<'a>(expr: &'a str, ops: &[&'a str]) -> Option<(&'a str, &'a str, &'a str)> {
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escape = false;
    let chars = expr.char_indices().collect::<Vec<_>>();
    for (index, ch) in chars {
        if in_string {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if depth == 0 => {
                for op in ops {
                    if expr[index..].starts_with(op) {
                        return Some((expr[..index].trim(), *op, expr[index + op.len()..].trim()));
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn trim_outer_parens(expr: &str) -> &str {
    let mut trimmed = expr.trim();
    loop {
        if !(trimmed.starts_with('(') && trimmed.ends_with(')')) {
            return trimmed;
        }
        let inner = &trimmed[1..trimmed.len() - 1];
        if split_top_level(inner, ',').len() == 1 {
            trimmed = inner.trim();
        } else {
            return trimmed;
        }
    }
}

fn function_arg<'a>(expr: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{name}(");
    expr.strip_prefix(&prefix)?.strip_suffix(')')
}

fn string_arg(raw: &str) -> String {
    let raw = raw.trim();
    serde_json::from_str::<String>(raw).unwrap_or_else(|_| raw.trim_matches('"').to_string())
}

fn json_number(value: f64) -> JsonValue {
    if value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value <= i64::MAX as f64
    {
        return json_integer(value as i64);
    }
    serde_json::Number::from_f64(value)
        .map(JsonValue::Number)
        .unwrap_or(JsonValue::Null)
}

fn json_integer(value: i64) -> JsonValue {
    JsonValue::Number(serde_json::Number::from(value))
}

fn json_type(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

fn json_length(value: &JsonValue) -> usize {
    match value {
        JsonValue::Array(values) => values.len(),
        JsonValue::Object(values) => values.len(),
        JsonValue::String(value) => value.chars().count(),
        JsonValue::Null => 0,
        _ => 1,
    }
}

fn json_keys(value: &JsonValue) -> JsonValue {
    let keys = match value {
        JsonValue::Object(map) => map.keys().cloned().collect::<Vec<_>>(),
        JsonValue::Array(values) => (0..values.len()).map(|index| index.to_string()).collect(),
        _ => Vec::new(),
    };
    JsonValue::Array(keys.into_iter().map(JsonValue::String).collect())
}

fn json_first(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Array(values) => values.first().cloned().unwrap_or(JsonValue::Null),
        JsonValue::String(value) => value
            .chars()
            .next()
            .map(|ch| JsonValue::String(ch.to_string()))
            .unwrap_or(JsonValue::Null),
        _ => JsonValue::Null,
    }
}

fn json_last(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Array(values) => values.last().cloned().unwrap_or(JsonValue::Null),
        JsonValue::String(value) => value
            .chars()
            .next_back()
            .map(|ch| JsonValue::String(ch.to_string()))
            .unwrap_or(JsonValue::Null),
        _ => JsonValue::Null,
    }
}

fn json_reverse(value: &JsonValue) -> JsonValue {
    match value {
        JsonValue::Array(values) => JsonValue::Array(values.iter().cloned().rev().collect()),
        JsonValue::String(value) => JsonValue::String(value.chars().rev().collect()),
        _ => JsonValue::Null,
    }
}

fn json_sort(value: &JsonValue) -> JsonValue {
    let JsonValue::Array(values) = value else {
        return JsonValue::Null;
    };
    let mut values = values.clone();
    values.sort_by_key(json_sort_key);
    JsonValue::Array(values)
}

fn json_unique(value: &JsonValue) -> JsonValue {
    let JsonValue::Array(values) = json_sort(value) else {
        return JsonValue::Null;
    };
    let mut output = Vec::new();
    for value in values {
        if !output.iter().any(|existing| existing == &value) {
            output.push(value);
        }
    }
    JsonValue::Array(output)
}

fn json_add(value: &JsonValue) -> Result<JsonValue, String> {
    let JsonValue::Array(values) = value else {
        return Ok(JsonValue::Null);
    };
    if values.iter().all(JsonValue::is_number) {
        return Ok(json_number(
            values.iter().filter_map(JsonValue::as_f64).sum(),
        ));
    }
    if values.iter().all(JsonValue::is_string) {
        return Ok(JsonValue::String(
            values
                .iter()
                .filter_map(JsonValue::as_str)
                .collect::<Vec<_>>()
                .join(""),
        ));
    }
    if values.iter().all(JsonValue::is_array) {
        let mut output = Vec::new();
        for value in values {
            if let JsonValue::Array(values) = value {
                output.extend(values.clone());
            }
        }
        return Ok(JsonValue::Array(output));
    }
    Err("unsupported add inputs".to_string())
}

fn json_minmax(value: &JsonValue, max: bool) -> JsonValue {
    let JsonValue::Array(values) = value else {
        return JsonValue::Null;
    };
    if max {
        values
            .iter()
            .cloned()
            .max_by_key(json_sort_key)
            .unwrap_or(JsonValue::Null)
    } else {
        values
            .iter()
            .cloned()
            .min_by_key(json_sort_key)
            .unwrap_or(JsonValue::Null)
    }
}

fn json_flatten(value: &JsonValue, depth: usize) -> JsonValue {
    let JsonValue::Array(values) = value else {
        return value.clone();
    };
    let mut output = Vec::new();
    for item in values {
        if depth > 0
            && let JsonValue::Array(_) = item
            && let JsonValue::Array(flat) = json_flatten(item, depth.saturating_sub(1))
        {
            output.extend(flat);
            continue;
        }
        output.push(item.clone());
    }
    JsonValue::Array(output)
}

fn json_to_entries(value: &JsonValue) -> JsonValue {
    let JsonValue::Object(map) = value else {
        return JsonValue::Array(Vec::new());
    };
    JsonValue::Array(
        map.iter()
            .map(|(key, value)| {
                JsonValue::Object(JsonMap::from_iter([
                    ("key".to_string(), JsonValue::String(key.clone())),
                    ("value".to_string(), value.clone()),
                ]))
            })
            .collect(),
    )
}

fn json_from_entries(value: &JsonValue) -> JsonValue {
    let JsonValue::Array(entries) = value else {
        return JsonValue::Object(JsonMap::new());
    };
    let mut map = JsonMap::new();
    for entry in entries {
        if let JsonValue::Object(entry) = entry
            && let Some((key, value)) = json_entry_key_value(entry)
        {
            insert_json_object_key(&mut map, key, value);
        }
    }
    JsonValue::Object(map)
}

fn json_transpose(value: &JsonValue) -> JsonValue {
    let JsonValue::Array(rows) = value else {
        return JsonValue::Array(Vec::new());
    };
    let width = rows
        .iter()
        .filter_map(|row| match row {
            JsonValue::Array(values) => Some(values.len()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let mut output = Vec::new();
    for column in 0..width {
        output.push(JsonValue::Array(
            rows.iter()
                .filter_map(|row| match row {
                    JsonValue::Array(values) => values.get(column).cloned(),
                    _ => None,
                })
                .collect(),
        ));
    }
    JsonValue::Array(output)
}

fn parse_json_path_arg(inner: &str) -> Vec<JsonValue> {
    serde_json::from_str::<Vec<JsonValue>>(inner.trim()).unwrap_or_default()
}

fn json_get_path(value: &JsonValue, path: &[JsonValue]) -> JsonValue {
    let mut current = value;
    for part in path {
        match part {
            JsonValue::String(key) => {
                let Some(next) = current.get(key) else {
                    return JsonValue::Null;
                };
                current = next;
            }
            JsonValue::Number(index) => {
                let Some(index) = index.as_u64().and_then(|index| usize::try_from(index).ok())
                else {
                    return JsonValue::Null;
                };
                let Some(next) = current.get(index) else {
                    return JsonValue::Null;
                };
                current = next;
            }
            _ => return JsonValue::Null,
        }
    }
    current.clone()
}

fn json_set_path(value: &JsonValue, path: &[JsonValue], replacement: JsonValue) -> JsonValue {
    let Some((head, tail)) = path.split_first() else {
        return replacement;
    };
    match head {
        JsonValue::String(key) => {
            let mut map = value.as_object().cloned().unwrap_or_default();
            let child = map.get(key).cloned().unwrap_or(JsonValue::Null);
            let next = json_set_path(&child, tail, replacement);
            insert_json_object_key(&mut map, key.clone(), next);
            JsonValue::Object(map)
        }
        JsonValue::Number(index) => {
            let Some(index) = index.as_u64().and_then(|index| usize::try_from(index).ok()) else {
                return value.clone();
            };
            let mut values = value.as_array().cloned().unwrap_or_default();
            if values.len() <= index {
                values.resize(index + 1, JsonValue::Null);
            }
            let next = json_set_path(&values[index], tail, replacement);
            values[index] = next;
            JsonValue::Array(values)
        }
        _ => value.clone(),
    }
}

fn json_entry_key_value(entry: &JsonMap<String, JsonValue>) -> Option<(String, JsonValue)> {
    let key = ["key", "Key", "name", "Name", "k"]
        .into_iter()
        .find_map(|name| match entry.get(name) {
            Some(JsonValue::String(value)) => Some(value.clone()),
            _ => None,
        })?;
    let value = ["value", "Value", "v"]
        .into_iter()
        .find_map(|name| entry.get(name).cloned())
        .unwrap_or(JsonValue::Null);
    Some((key, value))
}

fn json_sort_by(
    value: &JsonValue,
    root: &JsonValue,
    expr: &str,
    env: Option<&BTreeMap<String, String>>,
) -> JsonValue {
    let JsonValue::Array(values) = value else {
        return JsonValue::Null;
    };
    let mut values = values.clone();
    values.sort_by_key(|value| {
        json_sort_key(&eval_first(value, root, expr, env).unwrap_or(JsonValue::Null))
    });
    JsonValue::Array(values)
}

fn json_minmax_by(
    value: &JsonValue,
    root: &JsonValue,
    expr: &str,
    env: Option<&BTreeMap<String, String>>,
    max: bool,
) -> JsonValue {
    let JsonValue::Array(values) = value else {
        return JsonValue::Null;
    };
    if max {
        values
            .iter()
            .cloned()
            .max_by_key(|value| {
                json_sort_key(&eval_first(value, root, expr, env).unwrap_or(JsonValue::Null))
            })
            .unwrap_or(JsonValue::Null)
    } else {
        values
            .iter()
            .cloned()
            .min_by_key(|value| {
                json_sort_key(&eval_first(value, root, expr, env).unwrap_or(JsonValue::Null))
            })
            .unwrap_or(JsonValue::Null)
    }
}

fn json_unique_by(
    value: &JsonValue,
    root: &JsonValue,
    expr: &str,
    env: Option<&BTreeMap<String, String>>,
) -> JsonValue {
    let JsonValue::Array(values) = value else {
        return JsonValue::Null;
    };
    let mut output = Vec::new();
    let mut keys = Vec::new();
    for value in values {
        let key = json_sort_key(&eval_first(value, root, expr, env).unwrap_or(JsonValue::Null));
        if !keys.contains(&key) {
            keys.push(key);
            output.push(value.clone());
        }
    }
    JsonValue::Array(output)
}

fn json_group_by(
    value: &JsonValue,
    root: &JsonValue,
    expr: &str,
    env: Option<&BTreeMap<String, String>>,
) -> JsonValue {
    let JsonValue::Array(values) = json_sort_by(value, root, expr, env) else {
        return JsonValue::Null;
    };
    let mut groups: Vec<Vec<JsonValue>> = Vec::new();
    let mut previous = None;
    for value in values {
        let key = json_sort_key(&eval_first(&value, root, expr, env).unwrap_or(JsonValue::Null));
        if previous.as_ref() != Some(&key) {
            groups.push(Vec::new());
            previous = Some(key);
        }
        if let Some(group) = groups.last_mut() {
            group.push(value);
        }
    }
    JsonValue::Array(groups.into_iter().map(JsonValue::Array).collect())
}

fn json_contains(value: &JsonValue, needle: &JsonValue) -> bool {
    match (value, needle) {
        (JsonValue::Array(values), JsonValue::Array(needles)) => {
            needles.iter().all(|needle| values.contains(needle))
        }
        (JsonValue::Object(values), JsonValue::Object(needles)) => needles
            .iter()
            .all(|(key, needle)| values.get(key).is_some_and(|value| value == needle)),
        (JsonValue::String(value), JsonValue::String(needle)) => value.contains(needle),
        _ => value == needle,
    }
}

fn json_range_values(inner: &str) -> Vec<JsonValue> {
    let args = split_top_level(inner, ';');
    let (start, end) = if args.len() == 1 {
        (0, args[0].trim().parse::<i64>().unwrap_or(0))
    } else {
        (
            args[0].trim().parse::<i64>().unwrap_or(0),
            args[1].trim().parse::<i64>().unwrap_or(0),
        )
    };
    (start..end)
        .map(|value| json_number(value as f64))
        .collect()
}

fn compare_json(left: &JsonValue, right: &JsonValue, op: &str) -> bool {
    match op {
        "==" => left == right,
        "!=" => left != right,
        "<" => json_sort_key(left) < json_sort_key(right),
        ">" => json_sort_key(left) > json_sort_key(right),
        "<=" => json_sort_key(left) <= json_sort_key(right),
        ">=" => json_sort_key(left) >= json_sort_key(right),
        _ => false,
    }
}

fn apply_json_arithmetic(
    left: &JsonValue,
    right: &JsonValue,
    op: &str,
) -> Result<JsonValue, String> {
    if op == "+" {
        return match (left, right) {
            (JsonValue::String(left), JsonValue::String(right)) => {
                Ok(JsonValue::String(format!("{left}{right}")))
            }
            (JsonValue::Array(left), JsonValue::Array(right)) => {
                let mut output = left.clone();
                output.extend(right.clone());
                Ok(JsonValue::Array(output))
            }
            (JsonValue::Object(left), JsonValue::Object(right)) => {
                let mut output = left.clone();
                output.extend(right.clone());
                Ok(JsonValue::Object(output))
            }
            _ => Ok(json_number(
                left.as_f64().unwrap_or(0.0) + right.as_f64().unwrap_or(0.0),
            )),
        };
    }
    let left = left.as_f64().unwrap_or(0.0);
    let right = right.as_f64().unwrap_or(0.0);
    Ok(json_number(match op {
        "-" => left - right,
        "*" => left * right,
        "/" => left / right,
        "%" => left % right,
        _ => return Err(format!("unsupported operator {op}")),
    }))
}

fn is_truthy(value: &JsonValue) -> bool {
    !matches!(value, JsonValue::Null | JsonValue::Bool(false))
}

fn json_sort_key(value: &JsonValue) -> String {
    match value {
        JsonValue::Number(number) => format!("0:{:020.8}", number.as_f64().unwrap_or(0.0)),
        JsonValue::String(value) => format!("1:{value}"),
        JsonValue::Bool(value) => format!("2:{value}"),
        other => format!("3:{other}"),
    }
}

fn json_scalar_string(value: &JsonValue) -> String {
    match value {
        JsonValue::String(value) => value.clone(),
        JsonValue::Null => String::new(),
        other => other.to_string(),
    }
}

fn json_recursive_values(value: &JsonValue) -> Vec<JsonValue> {
    let mut values = vec![value.clone()];
    match value {
        JsonValue::Array(items) => {
            for item in items {
                values.extend(json_recursive_values(item));
            }
        }
        JsonValue::Object(map) => {
            for item in map.values() {
                values.extend(json_recursive_values(item));
            }
        }
        _ => {}
    }
    values
}

fn format_yq_value(value: &JsonValue, formatter: &str) -> JsonValue {
    match formatter {
        "base64" => value
            .as_str()
            .map(|value| JsonValue::String(base64_encode(value.as_bytes())))
            .unwrap_or(JsonValue::Null),
        "base64d" => value
            .as_str()
            .and_then(base64_decode)
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .map(JsonValue::String)
            .unwrap_or(JsonValue::Null),
        "uri" => value
            .as_str()
            .map(|value| JsonValue::String(percent_encode(value)))
            .unwrap_or(JsonValue::Null),
        "csv" => match value {
            JsonValue::Array(values) => JsonValue::String(format_csv_record(values, ",")),
            _ => JsonValue::Null,
        },
        "tsv" => match value {
            JsonValue::Array(values) => JsonValue::String(format_csv_record(values, "\t")),
            _ => JsonValue::Null,
        },
        "json" => JsonValue::String(
            serde_json::to_string(value).unwrap_or_else(|_| json_scalar_string(value)),
        ),
        "html" => value
            .as_str()
            .map(|value| JsonValue::String(escape_html(value)))
            .unwrap_or(JsonValue::Null),
        "sh" => value
            .as_str()
            .map(|value| JsonValue::String(shell_quote(value)))
            .unwrap_or(JsonValue::Null),
        "text" => JsonValue::String(json_scalar_string(value)),
        _ => JsonValue::Null,
    }
}

fn format_csv_record(values: &[JsonValue], separator: &str) -> String {
    values
        .iter()
        .map(|value| csv_escape_field(&json_scalar_string(value), separator))
        .collect::<Vec<_>>()
        .join(separator)
}

fn csv_escape_field(value: &str, separator: &str) -> String {
    if value.contains(separator) || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn decode_html_entities(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        output.push(TABLE[(b0 >> 2) as usize] as char);
        output.push(TABLE[(((b0 & 0b0000_0011) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            output.push(TABLE[(((b1 & 0b0000_1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(TABLE[(b2 & 0b0011_1111) as usize] as char);
        } else {
            output.push('=');
        }
    }
    output
}

fn base64_decode(value: &str) -> Option<Vec<u8>> {
    fn val(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            b'=' => Some(64),
            _ => None,
        }
    }

    let bytes = value.as_bytes();
    if bytes.len() % 4 != 0 {
        return None;
    }
    let mut output = Vec::new();
    for chunk in bytes.chunks(4) {
        let a = val(chunk[0])?;
        let b = val(chunk[1])?;
        let c = val(chunk[2])?;
        let d = val(chunk[3])?;
        if a == 64 || b == 64 {
            return None;
        }
        output.push((a << 2) | (b >> 4));
        if c != 64 {
            output.push(((b & 0b0000_1111) << 4) | (c >> 2));
        }
        if d != 64 {
            output.push(((c & 0b0000_0011) << 6) | d);
        }
    }
    Some(output)
}

#[derive(Clone, Debug)]
struct YqOptions {
    filter: String,
    paths: Vec<String>,
    input_format: Option<String>,
    output_format: String,
    raw_output: bool,
    compact: bool,
    null_input: bool,
    exit_status: bool,
    join_output: bool,
    indent: usize,
}

impl Default for YqOptions {
    fn default() -> Self {
        Self {
            filter: String::new(),
            paths: Vec::new(),
            input_format: None,
            output_format: "yaml".to_string(),
            raw_output: false,
            compact: false,
            null_input: false,
            exit_status: false,
            join_output: false,
            indent: 2,
        }
    }
}

fn command_yq(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let options = match parse_yq_options(args) {
        Ok(options) => options,
        Err(result) => return result,
    };
    if options.filter == "__help__" {
        return stdout_result("yq - YAML/JSON processor\nUsage: yq [options] <filter> [file]\n");
    }
    let inputs = match collect_yq_inputs(state, &options, stdin) {
        Ok(inputs) => inputs,
        Err(result) => return result,
    };
    let mut stdout = String::new();
    let mut saw_value = false;
    let mut last_value = JsonValue::Null;
    for input in inputs {
        let selected =
            match eval_structured_filter(&input, &input, &options.filter, Some(&state.env)) {
                Ok(selected) => selected,
                Err(error) => return stderr_result(1, format!("yq: {error}\n")),
            };
        for value in selected {
            saw_value = true;
            last_value = value.clone();
            stdout.push_str(&render_yq_output(&value, &options));
            if !options.join_output {
                stdout.push('\n');
            }
        }
    }
    let mut result = stdout_result(stdout);
    if options.exit_status && (!saw_value || !is_truthy(&last_value)) {
        result.exit_code = 1;
    }
    result
}

fn parse_yq_options(args: &[String]) -> Result<YqOptions, CommandResult> {
    let mut options = YqOptions::default();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "--help" {
            options.filter = "__help__".to_string();
            return Ok(options);
        }
        if let Some(format) = arg.strip_prefix("--input-format=") {
            options.input_format = Some(validate_yq_format(format, "input")?);
            index += 1;
            continue;
        }
        if let Some(format) = arg.strip_prefix("--output-format=") {
            options.output_format = validate_yq_format(format, "output")?;
            index += 1;
            continue;
        }
        match arg.as_str() {
            "-p" | "--input-format" => {
                let Some(format) = args.get(index + 1) else {
                    return Err(stderr_result(1, "yq: missing argument to -p\n"));
                };
                options.input_format = Some(validate_yq_format(format, "input")?);
                index += 2;
                continue;
            }
            "-o" | "--output-format" => {
                let Some(format) = args.get(index + 1) else {
                    return Err(stderr_result(1, "yq: missing argument to -o\n"));
                };
                options.output_format = validate_yq_format(format, "output")?;
                index += 2;
                continue;
            }
            "-I" => {
                if let Some(indent) = args.get(index + 1).and_then(|value| value.parse().ok()) {
                    options.indent = indent;
                    index += 2;
                    continue;
                }
            }
            "-r" => options.raw_output = true,
            "-c" => options.compact = true,
            "-n" => options.null_input = true,
            "-e" => options.exit_status = true,
            "-j" => options.join_output = true,
            _ if arg.starts_with('-') && arg.len() > 2 => {
                for flag in arg[1..].chars() {
                    match flag {
                        'r' => options.raw_output = true,
                        'c' => options.compact = true,
                        'n' => options.null_input = true,
                        'e' => options.exit_status = true,
                        'j' => options.join_output = true,
                        _ => {
                            return Err(stderr_result(1, format!("yq: unknown option: -{flag}\n")));
                        }
                    }
                }
            }
            _ if arg.starts_with('-') && arg != "-" => {
                return Err(stderr_result(1, format!("yq: unknown option: {arg}\n")));
            }
            _ if options.filter.is_empty() => options.filter = arg.clone(),
            _ => options.paths.push(arg.clone()),
        }
        index += 1;
    }
    if options.filter.is_empty() {
        return Err(stderr_result(1, "yq: missing filter\n"));
    }
    Ok(options)
}

fn validate_yq_format(format: &str, kind: &str) -> Result<String, CommandResult> {
    let normalized = format.to_ascii_lowercase();
    match normalized.as_str() {
        "yaml" | "yml" | "json" | "xml" | "csv" | "ini" | "toml" | "tsv" => Ok(normalized),
        _ => Err(stderr_result(
            1,
            format!("yq: invalid {kind} format: {format}\n"),
        )),
    }
}

fn collect_yq_inputs(
    state: &ExecState<'_>,
    options: &YqOptions,
    stdin: &str,
) -> Result<Vec<JsonValue>, CommandResult> {
    if options.null_input {
        return Ok(vec![JsonValue::Null]);
    }
    let raw_path = options.paths.first().map(String::as_str);
    let input = if let Some(path) = raw_path {
        if path == "-" {
            stdin.to_string()
        } else {
            let resolved = resolve_path(&state.cwd, path);
            state
                .session
                .inner
                .fs
                .lock()
                .map_err(|_| stderr_result(1, "yq: filesystem lock poisoned\n"))?
                .read_file(&resolved)
                .map_err(|_| {
                    stderr_result(1, format!("yq: {resolved}: No such file or directory\n"))
                })?
        }
    } else {
        stdin.to_string()
    };
    let format = options
        .input_format
        .clone()
        .or_else(|| raw_path.and_then(input_format_from_path))
        .unwrap_or_else(|| {
            let trimmed = input.trim_start();
            if trimmed.starts_with(['{', '[', '"'])
                || matches!(trimmed.trim(), "null" | "true" | "false")
                || trimmed
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_digit() || ch == '-')
            {
                "json".to_string()
            } else {
                "yaml".to_string()
            }
        });
    match format.as_str() {
        "json" => parse_json_stream(&input)
            .map_err(|error| stderr_result(1, format!("yq: parse error: {error}\n"))),
        "yaml" | "yml" => parse_simple_yaml(&input)
            .map(|value| vec![value])
            .map_err(|error| stderr_result(1, format!("yq: {error}\n"))),
        _ => Err(stderr_result(
            1,
            format!("yq: input format {format} is not implemented in the Rust backend\n"),
        )),
    }
}

fn input_format_from_path(path: &str) -> Option<String> {
    path.rsplit_once('.').map(|(_, extension)| match extension {
        "json" => "json".to_string(),
        "yaml" | "yml" => "yaml".to_string(),
        other => other.to_string(),
    })
}

fn render_yq_output(value: &JsonValue, options: &YqOptions) -> String {
    if options.output_format == "json" {
        return render_json_output(
            value,
            StructuredOutput {
                raw: options.raw_output,
                compact: options.compact,
                tab_indent: false,
                indent: options.indent,
                default_scalar_raw: false,
            },
        );
    }
    if options.raw_output {
        return json_scalar_string(value);
    }
    match value {
        JsonValue::Array(_) | JsonValue::Object(_) => render_yaml_value(value, 0, options.indent),
        _ => json_scalar_string(value),
    }
}

fn parse_simple_yaml(input: &str) -> Result<JsonValue, String> {
    let lines = input
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            (
                line.chars().take_while(|ch| *ch == ' ').count(),
                line.trim().to_string(),
            )
        })
        .collect::<Vec<_>>();
    let mut index = 0;
    if lines.is_empty() {
        return Ok(JsonValue::Null);
    }
    parse_yaml_block(&lines, &mut index, lines[0].0)
}

fn parse_yaml_block(
    lines: &[(usize, String)],
    index: &mut usize,
    indent: usize,
) -> Result<JsonValue, String> {
    if lines
        .get(*index)
        .is_some_and(|(line_indent, line)| *line_indent == indent && line.starts_with("- "))
    {
        let mut values = Vec::new();
        while let Some((line_indent, line)) = lines.get(*index) {
            if *line_indent != indent || !line.starts_with("- ") {
                break;
            }
            let item = line[2..].trim();
            *index += 1;
            let mut value = if item.is_empty() {
                parse_yaml_block(lines, index, indent + 2)?
            } else if let Some((key, raw_value)) = item.split_once(':') {
                let mut object = JsonMap::new();
                object.insert(key.trim().to_string(), parse_yaml_scalar(raw_value.trim()));
                JsonValue::Object(object)
            } else {
                parse_yaml_scalar(item)
            };
            if lines
                .get(*index)
                .is_some_and(|(line_indent, line)| *line_indent > indent && !line.starts_with("- "))
                && let JsonValue::Object(object) = &mut value
                && let JsonValue::Object(extra) = parse_yaml_block(lines, index, indent + 2)?
            {
                object.extend(extra);
            }
            values.push(value);
        }
        return Ok(JsonValue::Array(values));
    }
    let mut object = JsonMap::new();
    while let Some((line_indent, line)) = lines.get(*index) {
        if *line_indent != indent || line.starts_with("- ") {
            break;
        }
        let Some((key, raw_value)) = line.split_once(':') else {
            return Err(format!("invalid YAML line: {line}"));
        };
        *index += 1;
        let value = if raw_value.trim().is_empty() {
            parse_yaml_block(lines, index, indent + 2)?
        } else {
            parse_yaml_scalar(raw_value.trim())
        };
        object.insert(key.trim().to_string(), value);
    }
    Ok(JsonValue::Object(object))
}

fn parse_yaml_scalar(raw: &str) -> JsonValue {
    let raw = raw.trim().trim_matches('"').trim_matches('\'');
    match raw {
        "" => JsonValue::String(String::new()),
        "true" => JsonValue::Bool(true),
        "false" => JsonValue::Bool(false),
        "null" | "~" => JsonValue::Null,
        _ => raw
            .parse::<i64>()
            .map(json_integer)
            .or_else(|_| raw.parse::<f64>().map(json_number))
            .unwrap_or_else(|_| JsonValue::String(raw.to_string())),
    }
}

fn render_yaml_value(value: &JsonValue, indent: usize, step: usize) -> String {
    let spaces = " ".repeat(indent);
    match value {
        JsonValue::Object(map) => {
            let mut output = String::new();
            for (key, value) in map {
                match value {
                    JsonValue::Array(_) | JsonValue::Object(_) => {
                        output.push_str(&format!("{spaces}{key}:\n"));
                        output.push_str(&render_yaml_value(value, indent + step, step));
                    }
                    _ => {
                        output.push_str(&format!("{spaces}{key}: {}\n", json_scalar_string(value)))
                    }
                }
            }
            output.trim_end_matches('\n').to_string()
        }
        JsonValue::Array(values) => {
            let mut output = String::new();
            for value in values {
                match value {
                    JsonValue::Array(_) | JsonValue::Object(_) => {
                        output.push_str(&format!("{spaces}-\n"));
                        output.push_str(&render_yaml_value(value, indent + step, step));
                        output.push('\n');
                    }
                    _ => output.push_str(&format!("{spaces}- {}\n", json_scalar_string(value))),
                }
            }
            output.trim_end_matches('\n').to_string()
        }
        _ => json_scalar_string(value),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CsvData {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

fn command_xan(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return stdout_result("xan - CSV command toolkit\n");
    };
    if subcommand == "--help" || subcommand == "-h" {
        return stdout_result("xan - CSV command toolkit\n");
    }
    match subcommand {
        "count" => xan_count(state, &args[1..], stdin),
        "headers" => xan_headers(state, &args[1..], stdin),
        "head" => xan_head_tail(state, &args[1..], stdin, true),
        "tail" => xan_head_tail(state, &args[1..], stdin, false),
        "slice" => xan_slice(state, &args[1..], stdin),
        "reverse" => xan_reverse(state, &args[1..], stdin),
        "enum" => xan_enum(state, &args[1..], stdin),
        "behead" => xan_behead(state, &args[1..], stdin),
        "select" => xan_select_drop(state, &args[1..], stdin, false),
        "drop" => xan_select_drop(state, &args[1..], stdin, true),
        "rename" => xan_rename(state, &args[1..], stdin),
        "to" => xan_to(state, &args[1..], stdin),
        "from" => xan_from(state, &args[1..], stdin),
        "filter" => xan_filter(state, &args[1..], stdin),
        "sort" => xan_sort_cmd(state, &args[1..], stdin),
        "dedup" => xan_dedup(state, &args[1..], stdin),
        "search" => xan_search(state, &args[1..], stdin),
        "parallel" => stderr_result(1, "xan parallel: not yet implemented\n"),
        other => stderr_result(1, format!("xan: unknown command: {other}\n")),
    }
}

fn xan_count(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    match read_csv_arg(state, args.last().map(String::as_str), stdin, "xan count") {
        Ok(csv) => stdout_result(format!("{}\n", csv.rows.len())),
        Err(result) => result,
    }
}

fn xan_headers(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let just_names = args.iter().any(|arg| arg == "-j");
    match read_csv_arg(
        state,
        args.iter()
            .find(|arg| !arg.starts_with('-'))
            .map(String::as_str),
        stdin,
        "xan headers",
    ) {
        Ok(csv) => {
            let output = if just_names {
                csv.headers.join("\n")
            } else {
                csv.headers
                    .iter()
                    .enumerate()
                    .map(|(index, header)| format!("{index}   {header}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            stdout_result(format!("{output}\n"))
        }
        Err(result) => result,
    }
}

fn xan_head_tail(state: &ExecState<'_>, args: &[String], stdin: &str, head: bool) -> CommandResult {
    let limit = option_value(args, "-l")
        .and_then(|value| value.parse().ok())
        .unwrap_or(10);
    match read_csv_arg(
        state,
        positional_arg(args),
        stdin,
        if head { "xan head" } else { "xan tail" },
    ) {
        Ok(mut csv) => {
            csv.rows = if head {
                csv.rows.into_iter().take(limit).collect()
            } else {
                csv.rows
                    .into_iter()
                    .rev()
                    .take(limit)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect()
            };
            stdout_result(render_csv(&csv))
        }
        Err(result) => result,
    }
}

fn xan_slice(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let start = option_value(args, "-s")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let end: Option<usize> = option_value(args, "-e").and_then(|value| value.parse().ok());
    let len: Option<usize> = option_value(args, "-l").and_then(|value| value.parse().ok());
    match read_csv_arg(state, positional_arg(args), stdin, "xan slice") {
        Ok(mut csv) => {
            let end = end
                .or_else(|| len.map(|len| start + len))
                .unwrap_or(csv.rows.len());
            csv.rows = csv
                .rows
                .into_iter()
                .skip(start)
                .take(end.saturating_sub(start))
                .collect();
            stdout_result(render_csv(&csv))
        }
        Err(result) => result,
    }
}

fn xan_reverse(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    match read_csv_arg(state, positional_arg(args), stdin, "xan reverse") {
        Ok(mut csv) => {
            csv.rows.reverse();
            stdout_result(render_csv(&csv))
        }
        Err(result) => result,
    }
}

fn xan_enum(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let name = option_value(args, "-c").unwrap_or("index");
    match read_csv_arg(state, positional_arg(args), stdin, "xan enum") {
        Ok(mut csv) => {
            csv.headers.insert(0, name.to_string());
            for (index, row) in csv.rows.iter_mut().enumerate() {
                row.insert(0, index.to_string());
            }
            stdout_result(render_csv(&csv))
        }
        Err(result) => result,
    }
}

fn xan_behead(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    match read_csv_arg(state, positional_arg(args), stdin, "xan behead") {
        Ok(csv) => stdout_result(
            csv.rows
                .iter()
                .map(|row| row.join(","))
                .collect::<Vec<_>>()
                .join("\n")
                + if csv.rows.is_empty() { "" } else { "\n" },
        ),
        Err(result) => result,
    }
}

fn xan_select_drop(
    state: &ExecState<'_>,
    args: &[String],
    stdin: &str,
    drop: bool,
) -> CommandResult {
    let Some(spec) = args.first() else {
        return stderr_result(1, "xan select: missing column selector\n");
    };
    match read_csv_arg(state, args.get(1).map(String::as_str), stdin, "xan select") {
        Ok(csv) => {
            let selected = csv_column_indexes(&csv.headers, spec);
            let indexes = if drop {
                (0..csv.headers.len())
                    .filter(|index| !selected.contains(index))
                    .collect::<Vec<_>>()
            } else {
                selected
            };
            stdout_result(render_csv(&project_csv(&csv, &indexes)))
        }
        Err(result) => result,
    }
}

fn xan_rename(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let Some(new_names) = args.first() else {
        return stderr_result(1, "xan rename: missing column names\n");
    };
    let selector = option_value(args, "-s");
    match read_csv_arg(state, positional_arg(args), stdin, "xan rename") {
        Ok(mut csv) => {
            if let Some(selector) = selector {
                for index in csv_column_indexes(&csv.headers, selector) {
                    if let Some(header) = csv.headers.get_mut(index) {
                        *header = new_names.to_string();
                    }
                }
            } else {
                csv.headers = new_names.split(',').map(ToString::to_string).collect();
            }
            stdout_result(render_csv(&csv))
        }
        Err(result) => result,
    }
}

fn xan_to(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    if args.first().map(String::as_str) != Some("json") {
        return stderr_result(1, "xan to: usage: xan to <format> [FILE]\n");
    }
    match read_csv_arg(state, args.get(1).map(String::as_str), stdin, "xan to") {
        Ok(csv) => {
            let values = csv
                .rows
                .iter()
                .map(|row| {
                    JsonValue::Object(
                        csv.headers
                            .iter()
                            .zip(row)
                            .map(|(key, value)| (key.clone(), csv_json_value(value)))
                            .collect(),
                    )
                })
                .collect::<Vec<_>>();
            stdout_result(format!(
                "{}\n",
                serde_json::to_string_pretty(&JsonValue::Array(values)).unwrap()
            ))
        }
        Err(result) => result,
    }
}

fn xan_from(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    if option_value(args, "-f") != Some("json") {
        return stderr_result(1, "xan from: usage: xan from -f <format> [FILE]\n");
    }
    let Some(path) = positional_arg(args) else {
        return stderr_result(1, "xan from: usage: xan from -f <format> [FILE]\n");
    };
    let text = match read_text_arg(state, Some(path), stdin, "xan from") {
        Ok(text) => text,
        Err(result) => return result,
    };
    let Ok(value) = serde_json::from_str::<JsonValue>(&text) else {
        return stderr_result(1, "xan from: invalid JSON input\n");
    };
    match json_to_csv(&value) {
        Some(csv) => stdout_result(render_csv(&csv)),
        None => stderr_result(1, "xan from: invalid JSON input\n"),
    }
}

fn xan_filter(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let invert = args.iter().any(|arg| arg == "-v");
    let limit = option_value(args, "-l").and_then(|value| value.parse().ok());
    let expression = args
        .iter()
        .find(|arg| !arg.starts_with('-') && option_predecessor(args, arg) != Some("-l"));
    let Some(expression) = expression.map(String::as_str) else {
        return stderr_result(1, "xan filter: missing expression\n");
    };
    match read_csv_arg(
        state,
        args.last()
            .filter(|arg| *arg != expression)
            .map(String::as_str),
        stdin,
        "xan filter",
    ) {
        Ok(mut csv) => {
            let mut rows = Vec::new();
            for row in &csv.rows {
                let matched = eval_csv_predicate(&csv.headers, row, expression);
                if matched != invert {
                    rows.push(row.clone());
                }
                if limit.is_some_and(|limit| rows.len() >= limit) {
                    break;
                }
            }
            csv.rows = rows;
            stdout_result(render_csv(&csv))
        }
        Err(result) => result,
    }
}

fn xan_sort_cmd(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let Some(column) = option_value(args, "-s") else {
        return stderr_result(1, "xan sort: missing -s column\n");
    };
    let numeric = args.iter().any(|arg| arg == "-N");
    let reverse = args.iter().any(|arg| arg == "-R");
    match read_csv_arg(state, positional_arg(args), stdin, "xan sort") {
        Ok(mut csv) => {
            let index = csv
                .headers
                .iter()
                .position(|header| header == column)
                .unwrap_or(0);
            csv.rows
                .sort_by(|left, right| compare_csv_cell(&left[index], &right[index], numeric));
            if reverse {
                csv.rows.reverse();
            }
            stdout_result(render_csv(&csv))
        }
        Err(result) => result,
    }
}

fn xan_dedup(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let Some(column) = option_value(args, "-s") else {
        return stderr_result(1, "xan dedup: missing -s column\n");
    };
    match read_csv_arg(state, positional_arg(args), stdin, "xan dedup") {
        Ok(mut csv) => {
            let index = csv
                .headers
                .iter()
                .position(|header| header == column)
                .unwrap_or(0);
            let mut seen = Vec::new();
            csv.rows.retain(|row| {
                let key = row.get(index).cloned().unwrap_or_default();
                if seen.contains(&key) {
                    false
                } else {
                    seen.push(key);
                    true
                }
            });
            stdout_result(render_csv(&csv))
        }
        Err(result) => result,
    }
}

fn xan_search(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let invert = args.iter().any(|arg| arg == "-v");
    let ignore_case = args.iter().any(|arg| arg == "-i");
    let column = option_value(args, "-s");
    let pattern = option_value(args, "-r").or_else(|| {
        args.iter()
            .find(|arg| !arg.starts_with('-'))
            .map(String::as_str)
    });
    let Some(pattern) = pattern else {
        return stderr_result(1, "xan search: missing pattern\n");
    };
    let regex = match RegexBuilder::new(pattern)
        .case_insensitive(ignore_case)
        .build()
    {
        Ok(regex) => regex,
        Err(error) => return stderr_result(1, format!("xan search: invalid regex: {error}\n")),
    };
    match read_csv_arg(state, positional_arg(args), stdin, "xan search") {
        Ok(mut csv) => {
            let indexes = column.map_or_else(
                || (0..csv.headers.len()).collect::<Vec<_>>(),
                |column| csv_column_indexes(&csv.headers, column),
            );
            csv.rows.retain(|row| {
                let matched = indexes
                    .iter()
                    .filter_map(|index| row.get(*index))
                    .any(|cell| regex.is_match(cell));
                matched != invert
            });
            stdout_result(render_csv(&csv))
        }
        Err(result) => result,
    }
}

fn read_csv_arg(
    state: &ExecState<'_>,
    path: Option<&str>,
    stdin: &str,
    command: &str,
) -> Result<CsvData, CommandResult> {
    read_text_arg(state, path, stdin, command).map(|text| parse_csv(&text))
}

fn read_text_arg(
    state: &ExecState<'_>,
    path: Option<&str>,
    stdin: &str,
    command: &str,
) -> Result<String, CommandResult> {
    let Some(path) = path else {
        return Ok(stdin.to_string());
    };
    let path = resolve_path(&state.cwd, path);
    state
        .session
        .inner
        .fs
        .lock()
        .map_err(|_| stderr_result(1, format!("{command}: filesystem lock poisoned\n")))?
        .read_file(&path)
        .map_err(|_| stderr_result(1, format!("{command}: {path}: No such file\n")))
}

fn parse_csv(text: &str) -> CsvData {
    let mut lines = text.lines();
    let headers = lines.next().map(parse_csv_line).unwrap_or_default();
    let rows = lines.map(parse_csv_line).collect();
    CsvData { headers, rows }
}

fn parse_csv_line(line: &str) -> Vec<String> {
    line.split(',')
        .map(|cell| cell.trim_matches('"').to_string())
        .collect()
}

fn render_csv(csv: &CsvData) -> String {
    let mut output = String::new();
    output.push_str(&csv.headers.join(","));
    output.push('\n');
    for row in &csv.rows {
        output.push_str(&row.join(","));
        output.push('\n');
    }
    output
}

fn option_value<'a>(args: &'a [String], option: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == option)
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
}

fn option_predecessor<'a>(args: &'a [String], value: &str) -> Option<&'a str> {
    args.iter()
        .position(|arg| arg == value)
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| args.get(index))
        .map(String::as_str)
}

fn positional_arg(args: &[String]) -> Option<&str> {
    let mut skip_next = false;
    let mut positional = None;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if matches!(arg.as_str(), "-l" | "-s" | "-e" | "-c" | "-f" | "-r") {
            skip_next = true;
            continue;
        }
        if !arg.starts_with('-') {
            positional = Some(arg.as_str());
        }
    }
    positional
}

fn csv_column_indexes(headers: &[String], spec: &str) -> Vec<usize> {
    let mut indexes = Vec::new();
    for part in spec.split(',') {
        if let Some((start, end)) = part.split_once('-') {
            if let (Ok(start), Ok(end)) = (start.parse::<usize>(), end.parse::<usize>()) {
                indexes.extend(start..=end.min(headers.len().saturating_sub(1)));
            }
        } else if let Ok(index) = part.parse::<usize>() {
            indexes.push(index);
        } else if let Some(index) = headers.iter().position(|header| header == part) {
            indexes.push(index);
        }
    }
    indexes
}

fn project_csv(csv: &CsvData, indexes: &[usize]) -> CsvData {
    CsvData {
        headers: indexes
            .iter()
            .filter_map(|index| csv.headers.get(*index).cloned())
            .collect(),
        rows: csv
            .rows
            .iter()
            .map(|row| {
                indexes
                    .iter()
                    .filter_map(|index| row.get(*index).cloned())
                    .collect()
            })
            .collect(),
    }
}

fn csv_json_value(value: &str) -> JsonValue {
    match value {
        "true" => JsonValue::Bool(true),
        "false" => JsonValue::Bool(false),
        "" => JsonValue::String(String::new()),
        _ => value
            .parse::<i64>()
            .map(json_integer)
            .or_else(|_| value.parse::<f64>().map(json_number))
            .unwrap_or_else(|_| JsonValue::String(value.to_string())),
    }
}

fn json_to_csv(value: &JsonValue) -> Option<CsvData> {
    let JsonValue::Array(rows) = value else {
        return None;
    };
    if rows.first().is_some_and(JsonValue::is_array) {
        let JsonValue::Array(header_row) = &rows[0] else {
            return None;
        };
        let headers = header_row
            .iter()
            .map(json_scalar_string)
            .collect::<Vec<_>>();
        let rows = rows
            .iter()
            .skip(1)
            .filter_map(|row| match row {
                JsonValue::Array(row) => Some(row.iter().map(json_scalar_string).collect()),
                _ => None,
            })
            .collect();
        return Some(CsvData { headers, rows });
    }
    let mut headers = rows
        .iter()
        .filter_map(|row| match row {
            JsonValue::Object(map) => Some(map.keys().cloned().collect::<Vec<_>>()),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    headers.sort();
    headers.dedup();
    let rows = rows
        .iter()
        .filter_map(|row| match row {
            JsonValue::Object(map) => Some(
                headers
                    .iter()
                    .map(|header| map.get(header).map(json_scalar_string).unwrap_or_default())
                    .collect(),
            ),
            _ => None,
        })
        .collect();
    Some(CsvData { headers, rows })
}

fn eval_csv_predicate(headers: &[String], row: &[String], expression: &str) -> bool {
    let parts = expression.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 3 {
        return false;
    }
    let left = headers
        .iter()
        .position(|header| header == parts[0])
        .and_then(|index| row.get(index))
        .map(String::as_str)
        .unwrap_or_default();
    let right = parts[2].trim_matches('"');
    match parts[1] {
        "eq" => left == right,
        ">" => left.parse::<f64>().unwrap_or(0.0) > right.parse::<f64>().unwrap_or(0.0),
        "<" => left.parse::<f64>().unwrap_or(0.0) < right.parse::<f64>().unwrap_or(0.0),
        ">=" => left.parse::<f64>().unwrap_or(0.0) >= right.parse::<f64>().unwrap_or(0.0),
        "<=" => left.parse::<f64>().unwrap_or(0.0) <= right.parse::<f64>().unwrap_or(0.0),
        _ => false,
    }
}

fn compare_csv_cell(left: &str, right: &str, numeric: bool) -> CmpOrdering {
    if numeric {
        left.parse::<f64>()
            .unwrap_or(0.0)
            .partial_cmp(&right.parse::<f64>().unwrap_or(0.0))
            .unwrap_or(CmpOrdering::Equal)
    } else {
        left.cmp(right)
    }
}

fn command_sqlite3(_state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    sqlite3_minimal(args, stdin)
}

#[derive(Clone, Debug)]
struct SqliteOptions {
    mode: SqliteMode,
    header: bool,
    separator: String,
    newline: String,
    null_value: String,
    echo: bool,
    bail: bool,
    pre_commands: Vec<String>,
}

impl Default for SqliteOptions {
    fn default() -> Self {
        Self {
            mode: SqliteMode::List,
            header: false,
            separator: "|".to_string(),
            newline: "\n".to_string(),
            null_value: String::new(),
            echo: false,
            bail: false,
            pre_commands: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SqliteMode {
    List,
    Csv,
    Json,
    Line,
    Column,
    Table,
    Markdown,
    Html,
    Box,
    Ascii,
    Tabs,
    Quote,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SqlValue {
    raw: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SqlResultSet {
    columns: Vec<String>,
    rows: Vec<Vec<SqlValue>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct MiniSqlDb {
    tables: BTreeMap<String, SqlResultSet>,
}

fn sqlite3_minimal(args: &[String], stdin: &str) -> CommandResult {
    let (options, database, sql) = match parse_sqlite_args(args, stdin) {
        Ok(parsed) => parsed,
        Err(result) => return result,
    };
    if database == "__version__" {
        return stdout_result("3.45.0 0000000000000000000000000000000000000000\n");
    }
    if database == "__help__" {
        return stdout_result("Usage: sqlite3 [OPTIONS] DATABASE [SQL]\n");
    }
    let mut db = MiniSqlDb::default();
    let full_sql = options
        .pre_commands
        .iter()
        .chain(std::iter::once(&sql))
        .cloned()
        .collect::<Vec<_>>()
        .join(";");
    let statements = split_sql_statements(&full_sql);
    let mut stdout = String::new();
    if options.echo && !sql.trim().is_empty() {
        stdout.push_str(sql.trim());
        stdout.push('\n');
    }
    for statement in statements {
        match execute_sql_statement(&mut db, &statement) {
            Ok(Some(result_set)) => stdout.push_str(&format_sql_result(&result_set, &options)),
            Ok(None) => {}
            Err(error) => {
                if options.bail {
                    return CommandResult {
                        stdout,
                        stderr: format!("Error: {error}\n"),
                        exit_code: 1,
                        exit_requested: false,
                    };
                }
                stdout.push_str(&format!("Error: {error}\n"));
            }
        }
    }
    stdout_result(stdout)
}

fn parse_sqlite_args(
    args: &[String],
    stdin: &str,
) -> Result<(SqliteOptions, String, String), CommandResult> {
    let mut options = SqliteOptions::default();
    let mut positionals = Vec::new();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "--" {
            positionals.extend(args[index + 1..].iter().cloned());
            break;
        }
        match arg.as_str() {
            "-version" | "--version" => {
                return Ok((options, "__version__".to_string(), String::new()));
            }
            "-help" | "--help" => return Ok((options, "__help__".to_string(), String::new())),
            "-list" => options.mode = SqliteMode::List,
            "-csv" => options.mode = SqliteMode::Csv,
            "-json" => options.mode = SqliteMode::Json,
            "-line" => options.mode = SqliteMode::Line,
            "-column" => options.mode = SqliteMode::Column,
            "-table" => options.mode = SqliteMode::Table,
            "-markdown" => options.mode = SqliteMode::Markdown,
            "-html" => options.mode = SqliteMode::Html,
            "-box" => options.mode = SqliteMode::Box,
            "-ascii" => {
                options.mode = SqliteMode::Ascii;
                options.separator = "\x1f".to_string();
                options.newline = "\x1e".to_string();
            }
            "-tabs" => {
                options.mode = SqliteMode::Tabs;
                options.separator = "\t".to_string();
            }
            "-quote" => options.mode = SqliteMode::Quote,
            "-header" => options.header = true,
            "-noheader" => options.header = false,
            "-echo" => options.echo = true,
            "-bail" => options.bail = true,
            "-separator" | "-newline" | "-nullvalue" | "-cmd" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(stderr_result(
                        1,
                        format!("sqlite3: Error: missing argument to {arg}\n"),
                    ));
                };
                match arg.as_str() {
                    "-separator" => options.separator = value.clone(),
                    "-newline" => options.newline = value.clone(),
                    "-nullvalue" => options.null_value = value.clone(),
                    "-cmd" => options.pre_commands.push(value.clone()),
                    _ => {}
                }
                index += 2;
                continue;
            }
            _ if arg.starts_with("--") => {
                let normalized = arg.trim_start_matches('-');
                return Err(stderr_result(
                    1,
                    format!(
                        "sqlite3: Error: unknown option: -{normalized}\nUse -help for a list of options.\n"
                    ),
                ));
            }
            _ if arg.starts_with('-') => {
                return Err(stderr_result(
                    1,
                    format!(
                        "sqlite3: Error: unknown option: {arg}\nUse -help for a list of options.\n"
                    ),
                ));
            }
            _ => positionals.push(arg.clone()),
        }
        index += 1;
    }
    let Some(database) = positionals.first().cloned() else {
        return Err(stderr_result(1, "sqlite3: missing database argument\n"));
    };
    let sql = if positionals.len() > 1 {
        positionals[1..].join(" ")
    } else {
        stdin.to_string()
    };
    if sql.trim().is_empty() {
        return Err(stderr_result(1, "sqlite3: no SQL provided\n"));
    }
    Ok((options, database, sql))
}

fn split_sql_statements(sql: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut start = 0;
    let mut in_single = false;
    let mut in_double = false;
    let chars = sql.char_indices().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        let (byte_index, ch) = chars[index];
        match ch {
            '\'' if !in_double => {
                if in_single && chars.get(index + 1).is_some_and(|(_, next)| *next == '\'') {
                    index += 1;
                } else {
                    in_single = !in_single;
                }
            }
            '"' if !in_single => in_double = !in_double,
            ';' if !in_single && !in_double => {
                let statement = sql[start..byte_index].trim();
                if !statement.is_empty() {
                    statements.push(statement.to_string());
                }
                start = byte_index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    let statement = sql[start..].trim();
    if !statement.is_empty() {
        statements.push(statement.to_string());
    }
    statements
}

fn execute_sql_statement(
    db: &mut MiniSqlDb,
    statement: &str,
) -> Result<Option<SqlResultSet>, String> {
    let upper = statement.to_ascii_uppercase();
    if upper.contains("LOAD_EXTENSION") {
        return Err("not authorized".to_string());
    }
    if upper.starts_with("CREATE TABLE ") {
        create_sql_table(db, statement)?;
        return Ok(None);
    }
    if upper.starts_with("INSERT INTO ") {
        insert_sql_rows(db, statement)?;
        return Ok(None);
    }
    if upper.starts_with("SELECT ") {
        return select_sql(db, statement).map(Some);
    }
    Err(format!(
        "near \"{}\": syntax error",
        statement.split_whitespace().next().unwrap_or(statement)
    ))
}

fn create_sql_table(db: &mut MiniSqlDb, statement: &str) -> Result<(), String> {
    let rest = statement["CREATE TABLE ".len()..].trim();
    let Some((name, columns)) = rest.split_once('(') else {
        return Err("invalid CREATE TABLE".to_string());
    };
    let columns = columns
        .trim_end_matches(')')
        .split(',')
        .filter_map(|column| column.split_whitespace().next())
        .map(|column| column.trim_matches('"').to_string())
        .collect::<Vec<_>>();
    db.tables.insert(
        name.trim().to_string(),
        SqlResultSet {
            columns,
            rows: Vec::new(),
        },
    );
    Ok(())
}

fn insert_sql_rows(db: &mut MiniSqlDb, statement: &str) -> Result<(), String> {
    let rest = statement["INSERT INTO ".len()..].trim();
    let Some((table, values)) = rest.split_once("VALUES") else {
        return Err("invalid INSERT".to_string());
    };
    let table = table.split_whitespace().next().unwrap_or(table).trim();
    let Some(result_set) = db.tables.get_mut(table) else {
        return Err(format!("no such table: {table}"));
    };
    for row in parse_sql_value_groups(values) {
        result_set.rows.push(row);
    }
    Ok(())
}

fn parse_sql_value_groups(values: &str) -> Vec<Vec<SqlValue>> {
    let mut rows = Vec::new();
    let mut depth = 0_i32;
    let mut start = 0;
    let mut in_single = false;
    let chars = values.char_indices().collect::<Vec<_>>();
    for (index, ch) in chars {
        match ch {
            '\'' => in_single = !in_single,
            '(' if !in_single => {
                if depth == 0 {
                    start = index + 1;
                }
                depth += 1;
            }
            ')' if !in_single => {
                depth -= 1;
                if depth == 0 {
                    rows.push(parse_sql_values(&values[start..index]));
                }
            }
            _ => {}
        }
    }
    rows
}

fn parse_sql_values(values: &str) -> Vec<SqlValue> {
    split_top_level(values, ',')
        .into_iter()
        .map(parse_sql_value)
        .collect()
}

fn parse_sql_value(value: &str) -> SqlValue {
    let value = value.trim();
    if value.eq_ignore_ascii_case("NULL") {
        return SqlValue { raw: None };
    }
    if value.len() >= 3
        && value.starts_with("X'")
        && value.ends_with('\'')
        && let Some(decoded) = decode_sql_blob(&value[2..value.len() - 1])
    {
        return SqlValue { raw: Some(decoded) };
    }
    if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
        return SqlValue {
            raw: Some(value[1..value.len() - 1].replace("''", "'")),
        };
    }
    SqlValue {
        raw: Some(value.to_string()),
    }
}

fn select_sql(db: &MiniSqlDb, statement: &str) -> Result<SqlResultSet, String> {
    let body = statement["SELECT ".len()..].trim();
    if let Some((projection, table)) = body.split_once(" FROM ") {
        let table = table.trim();
        let Some(source) = db.tables.get(table) else {
            return Err(format!("no such table: {table}"));
        };
        if projection.trim() == "*" {
            return Ok(source.clone());
        }
        let columns = projection
            .split(',')
            .map(|column| column.trim().to_string())
            .collect::<Vec<_>>();
        let indexes = columns
            .iter()
            .map(|column| {
                source
                    .columns
                    .iter()
                    .position(|source| source == column)
                    .unwrap_or(0)
            })
            .collect::<Vec<_>>();
        return Ok(SqlResultSet {
            columns,
            rows: source
                .rows
                .iter()
                .map(|row| {
                    indexes
                        .iter()
                        .filter_map(|index| row.get(*index).cloned())
                        .collect()
                })
                .collect(),
        });
    }
    let values = split_top_level(body, ',');
    let mut columns = Vec::new();
    let mut row = Vec::new();
    for (index, value) in values.iter().enumerate() {
        let (raw_value, alias) = split_sql_alias(value);
        columns.push(alias.unwrap_or_else(|| (index + 1).to_string()));
        row.push(parse_sql_value(raw_value));
    }
    Ok(SqlResultSet {
        columns,
        rows: vec![row],
    })
}

fn split_sql_alias(value: &str) -> (&str, Option<String>) {
    let lower = value.to_ascii_lowercase();
    if let Some(index) = lower.rfind(" as ") {
        return (&value[..index], Some(value[index + 4..].trim().to_string()));
    }
    (value, None)
}

fn format_sql_result(result_set: &SqlResultSet, options: &SqliteOptions) -> String {
    match options.mode {
        SqliteMode::Json => format_sql_json(result_set),
        SqliteMode::Line => format_sql_line(result_set, options),
        SqliteMode::Column => format_sql_column(result_set, options),
        SqliteMode::Table => format_sql_table(result_set, options, false),
        SqliteMode::Markdown => format_sql_markdown(result_set, options),
        SqliteMode::Html => format_sql_html(result_set, options),
        SqliteMode::Box => format_sql_table(result_set, options, true),
        SqliteMode::Ascii => format_sql_delimited(result_set, options, &options.separator),
        SqliteMode::Csv => format_sql_delimited(result_set, options, ","),
        SqliteMode::Tabs => format_sql_delimited(result_set, options, "\t"),
        SqliteMode::Quote => format_sql_quote(result_set),
        SqliteMode::List => format_sql_delimited(result_set, options, &options.separator),
    }
}

fn format_sql_delimited(
    result_set: &SqlResultSet,
    options: &SqliteOptions,
    separator: &str,
) -> String {
    let mut output = String::new();
    if options.header {
        output.push_str(&result_set.columns.join(separator));
        output.push_str(&options.newline);
    }
    for row in &result_set.rows {
        output.push_str(
            &row.iter()
                .map(|value| sql_value_delimited(value, &options.null_value, separator))
                .collect::<Vec<_>>()
                .join(separator),
        );
        output.push_str(&options.newline);
    }
    output
}

fn format_sql_column(result_set: &SqlResultSet, options: &SqliteOptions) -> String {
    let widths = sql_column_widths(result_set, options);
    let mut output = String::new();
    if options.header {
        output.push_str(&format_sql_padded_row(&result_set.columns, &widths));
        output.push('\n');
        output.push_str(
            &widths
                .iter()
                .map(|width| "-".repeat(*width))
                .collect::<Vec<_>>()
                .join("  "),
        );
        output.push('\n');
    }
    for row in &result_set.rows {
        let cells = row
            .iter()
            .map(|value| sql_value_text(value, &options.null_value))
            .collect::<Vec<_>>();
        output.push_str(&format_sql_padded_row(&cells, &widths));
        output.push('\n');
    }
    output
}

fn format_sql_padded_row(cells: &[String], widths: &[usize]) -> String {
    cells
        .iter()
        .zip(widths)
        .map(|(cell, width)| format!("{cell:<width$}"))
        .collect::<Vec<_>>()
        .join("  ")
}

fn sql_column_widths(result_set: &SqlResultSet, options: &SqliteOptions) -> Vec<usize> {
    let mut widths = result_set
        .columns
        .iter()
        .map(|column| column.len())
        .collect::<Vec<_>>();
    for row in &result_set.rows {
        for (index, value) in row.iter().enumerate() {
            let width = sql_value_text(value, &options.null_value).len();
            if let Some(existing) = widths.get_mut(index) {
                *existing = (*existing).max(width);
            }
        }
    }
    widths
}

fn format_sql_table(result_set: &SqlResultSet, options: &SqliteOptions, unicode: bool) -> String {
    let widths = sql_column_widths(result_set, options);
    let (tl, tr, bl, br, h, v, cross) = if unicode {
        ("┌", "┐", "└", "┘", "─", "│", "┼")
    } else {
        ("+", "+", "+", "+", "-", "|", "+")
    };
    let border = |left: &str, right: &str| {
        format!(
            "{left}{}{right}\n",
            widths
                .iter()
                .map(|width| h.repeat(width + 2))
                .collect::<Vec<_>>()
                .join(cross)
        )
    };
    let row = |cells: Vec<String>| {
        format!(
            "{v}{}{v}\n",
            cells
                .iter()
                .zip(&widths)
                .map(|(cell, width)| format!(" {cell:<width$} "))
                .collect::<Vec<_>>()
                .join(v)
        )
    };
    let mut output = String::new();
    output.push_str(&border(tl, tr));
    if options.header || unicode {
        output.push_str(&row(result_set.columns.clone()));
        output.push_str(&border(cross, cross));
    }
    for row_values in &result_set.rows {
        output.push_str(&row(row_values
            .iter()
            .map(|value| sql_value_text(value, &options.null_value))
            .collect()));
    }
    output.push_str(&border(bl, br));
    output
}

fn format_sql_markdown(result_set: &SqlResultSet, options: &SqliteOptions) -> String {
    let mut output = String::new();
    if options.header {
        output.push_str(&format!("| {} |\n", result_set.columns.join(" | ")));
        output.push_str(&format!(
            "|{}|\n",
            result_set
                .columns
                .iter()
                .map(|_| "---")
                .collect::<Vec<_>>()
                .join("|")
        ));
    }
    for row in &result_set.rows {
        output.push_str(&format!(
            "| {} |\n",
            row.iter()
                .map(|value| sql_value_text(value, &options.null_value))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    output
}

fn format_sql_html(result_set: &SqlResultSet, options: &SqliteOptions) -> String {
    let mut output = String::new();
    if options.header {
        output.push_str("<TR>");
        for column in &result_set.columns {
            output.push_str(&format!("<TH>{}</TH>", escape_html(column)));
        }
        output.push_str("</TR>\n");
    }
    for row in &result_set.rows {
        output.push_str("<TR>");
        for value in row {
            output.push_str(&format!(
                "<TD>{}</TD>",
                escape_html(&sql_value_text(value, &options.null_value))
            ));
        }
        output.push_str("</TR>\n");
    }
    output
}

fn format_sql_json(result_set: &SqlResultSet) -> String {
    if result_set.rows.is_empty() {
        return String::new();
    }
    let rows = result_set
        .rows
        .iter()
        .map(|row| {
            let fields = result_set
                .columns
                .iter()
                .zip(row)
                .map(|(column, value)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(column).unwrap(),
                        value
                            .raw
                            .as_ref()
                            .map(|value| serde_json::to_string(&csv_json_value(value)).unwrap())
                            .unwrap_or_else(|| "null".to_string())
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{fields}}}")
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!("[{rows}]\n")
}

fn format_sql_line(result_set: &SqlResultSet, options: &SqliteOptions) -> String {
    let mut output = String::new();
    for row in &result_set.rows {
        for (column, value) in result_set.columns.iter().zip(row) {
            output.push_str(&format!(
                "{column:>5} = {}\n",
                sql_value_text(value, &options.null_value)
            ));
        }
    }
    output
}

fn format_sql_quote(result_set: &SqlResultSet) -> String {
    let mut output = String::new();
    for row in &result_set.rows {
        output.push_str(
            &row.iter()
                .map(sql_value_quote)
                .collect::<Vec<_>>()
                .join(","),
        );
        output.push('\n');
    }
    output
}

fn sql_value_text(value: &SqlValue, null_value: &str) -> String {
    value.raw.clone().unwrap_or_else(|| null_value.to_string())
}

fn sql_value_delimited(value: &SqlValue, null_value: &str, separator: &str) -> String {
    let value = sql_value_text(value, null_value);
    if separator == "," {
        csv_escape_field(&value, separator)
    } else {
        value
    }
}

fn sql_value_quote(value: &SqlValue) -> String {
    match &value.raw {
        None => "NULL".to_string(),
        Some(value) if value.parse::<i64>().is_ok() => value.clone(),
        Some(value) if value.parse::<f64>().is_ok() => {
            let float = value.parse::<f64>().unwrap_or_default();
            if value.contains('.') {
                format!("{float:.16}")
            } else {
                value.clone()
            }
        }
        Some(value) => format!("'{}'", value.replace('\'', "''")),
    }
}

fn decode_sql_blob(hex: &str) -> Option<String> {
    if hex.len() % 2 != 0 {
        return None;
    }
    let mut bytes = Vec::new();
    for index in (0..hex.len()).step_by(2) {
        let byte = u8::from_str_radix(&hex[index..index + 2], 16).ok()?;
        bytes.push(byte);
    }
    String::from_utf8(bytes).ok()
}

fn command_which(state: &ExecState<'_>, args: &[String]) -> CommandResult {
    let mut stdout = String::new();
    let mut exit_code = 0;
    for arg in args {
        if state.session.inner.commands.contains(arg) {
            stdout.push_str("/usr/bin/");
            stdout.push_str(arg);
            stdout.push('\n');
        } else {
            exit_code = 1;
        }
    }
    CommandResult {
        stdout,
        exit_code,
        ..CommandResult::default()
    }
}

fn command_basename_utility(args: &[String]) -> CommandResult {
    if args.iter().any(|arg| arg == "--help") {
        return stdout_result(
            "Usage: basename NAME [SUFFIX]\nbasename OPTION... NAME...\nstrip directory and suffix from filenames\n",
        );
    }

    let mut multiple = false;
    let mut suffix = String::new();
    let mut names = Vec::new();
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "-a" || arg == "--multiple" {
            multiple = true;
        } else if arg == "-s" {
            if let Some(value) = args.get(index + 1) {
                suffix = value.clone();
                index += 1;
            }
            multiple = true;
        } else if let Some(value) = arg.strip_prefix("--suffix=") {
            suffix = value.to_string();
            multiple = true;
        } else if !arg.starts_with('-') {
            names.push(arg.as_str());
        }
        index += 1;
    }

    if names.is_empty() {
        return stderr_result(1, "basename: missing operand\n");
    }

    if !multiple && names.len() >= 2 {
        suffix = names.pop().unwrap_or_default().to_string();
    }

    let stdout = names
        .into_iter()
        .map(|name| {
            let clean_name = name.trim_end_matches('/');
            let mut base = clean_name
                .rsplit('/')
                .next()
                .unwrap_or(clean_name)
                .to_string();
            if !suffix.is_empty() && base.ends_with(&suffix) {
                base.truncate(base.len() - suffix.len());
            }
            base
        })
        .collect::<Vec<_>>()
        .join("\n");
    stdout_result(format!("{stdout}\n"))
}

fn command_dirname_utility(args: &[String]) -> CommandResult {
    if args.iter().any(|arg| arg == "--help") {
        return stdout_result(
            "Usage: dirname [OPTION] NAME...\nstrip last component from file name\n",
        );
    }

    let names = args
        .iter()
        .filter(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if names.is_empty() {
        return stderr_result(1, "dirname: missing operand\n");
    }

    let stdout = names
        .into_iter()
        .map(|name| {
            let clean_name = name.trim_end_matches('/');
            match clean_name.rfind('/') {
                None => ".".to_string(),
                Some(0) => "/".to_string(),
                Some(index) => clean_name[..index].to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    stdout_result(format!("{stdout}\n"))
}

fn collect_named_text_inputs(
    state: &ExecState<'_>,
    paths: &[String],
    stdin: &str,
    _command: &str,
) -> Result<Vec<NamedTextInput>, String> {
    if paths.is_empty() {
        return Ok(vec![NamedTextInput {
            label: String::new(),
            text: stdin.to_string(),
        }]);
    }
    let fs = state
        .session
        .inner
        .fs
        .lock()
        .map_err(|_| "filesystem lock poisoned".to_string())?;
    let mut inputs = Vec::new();
    for path in paths {
        let path = resolve_path(&state.cwd, path);
        let stat = fs
            .stat(&path)
            .map_err(|_| format!("{path}: No such file or directory"))?;
        if stat.is_directory {
            return Err(format!("{path}: Is a directory"));
        }
        let text = fs
            .read_file(&path)
            .map_err(|_| format!("{path}: No such file or directory"))?;
        inputs.push(NamedTextInput { label: path, text });
    }
    Ok(inputs)
}

fn collect_text_inputs(
    state: &ExecState<'_>,
    paths: &[String],
    stdin: &str,
) -> Result<String, String> {
    if paths.is_empty() {
        return Ok(stdin.to_string());
    }
    let fs = state
        .session
        .inner
        .fs
        .lock()
        .map_err(|_| "filesystem lock poisoned".to_string())?;
    let mut input = String::new();
    for path in paths {
        let path = resolve_path(&state.cwd, path);
        input.push_str(
            &fs.read_file(&path)
                .map_err(|_| format!("{path}: No such file or directory"))?,
        );
    }
    Ok(input)
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let mut regex = String::from("^");
    for ch in pattern.chars() {
        match ch {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            other => regex.push_str(&regex::escape(&other.to_string())),
        }
    }
    regex.push('$');
    Regex::new(&regex).is_ok_and(|regex| regex.is_match(text))
}

fn is_assignment(value: &str) -> bool {
    value
        .split_once('=')
        .is_some_and(|(name, _)| is_valid_var_name(name))
}

fn is_valid_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn command_sleep(state: &ExecState<'_>, args: &[String]) -> CommandResult {
    let ms = args
        .iter()
        .filter_map(|arg| parse_duration_ms(arg).ok())
        .sum::<u64>();
    let deadline = Instant::now() + Duration::from_millis(ms);
    while Instant::now() < deadline {
        if let Some(cancelled) = cancelled_result(state) {
            return cancelled;
        }
        thread::sleep(Duration::from_millis(5));
    }
    CommandResult::default()
}

fn command_timeout(state: &mut ExecState<'_>, args: &[String], stdin: String) -> CommandResult {
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        match arg.as_str() {
            "--help" => {
                return stdout_result(
                    "Usage: timeout [OPTION] DURATION COMMAND [ARG]...\nRun COMMAND with a time limit.\n",
                );
            }
            "--foreground" => index += 1,
            "-k" | "-s" => {
                if index + 1 >= args.len() {
                    return stderr_result(
                        1,
                        format!("timeout: option '{}' requires an argument\n", arg),
                    );
                }
                index += 2;
            }
            _ if arg.starts_with('-') => {
                return stderr_result(1, format!("timeout: unrecognized option '{}'\n", arg));
            }
            _ => break,
        }
    }

    let Some(duration) = args.get(index) else {
        return stderr_result(1, "timeout: missing operand\n");
    };
    let timeout_ms = match parse_duration_ms(duration) {
        Ok(ms) => ms.max(1),
        Err(()) => {
            return stderr_result(
                1,
                format!("timeout: invalid time interval '{}'\n", duration),
            );
        }
    };
    let command = &args[index + 1..];
    if command.is_empty() {
        return stderr_result(1, "timeout: missing operand after duration\n");
    }

    let old_started_at = state.started_at;
    let old_timeout_ms = state.timeout_ms;
    state.started_at = Instant::now();
    state.timeout_ms = timeout_ms;
    let mut result = execute_tokens(state, command, stdin);
    state.started_at = old_started_at;
    state.timeout_ms = old_timeout_ms;
    if result.exit_code == JUST_BASH_TIMEOUT_EXIT_CODE
        && result.stderr.starts_with("Command timed out after")
    {
        result.stderr.clear();
    }
    result
}

fn parse_duration_ms(raw: &str) -> Result<u64, ()> {
    let (number, multiplier) = match raw.chars().last() {
        Some('s') => (&raw[..raw.len() - 1], 1_000.0),
        Some('m') => (&raw[..raw.len() - 1], 60_000.0),
        Some('h') => (&raw[..raw.len() - 1], 3_600_000.0),
        Some('d') => (&raw[..raw.len() - 1], 86_400_000.0),
        _ => (raw, 1_000.0),
    };
    let seconds = number.parse::<f64>().map_err(|_| ())?;
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(());
    }
    Ok((seconds * multiplier) as u64)
}

fn command_bash(
    shell_name: &str,
    state: &mut ExecState<'_>,
    args: &[String],
    stdin: String,
) -> CommandResult {
    if args.is_empty() {
        return CommandResult::default();
    }
    if args.first().is_some_and(|arg| arg == "--help") {
        return stdout_result(format!(
            "Usage: {shell_name} [-c command] [script] [args...]\n"
        ));
    }
    if args.first().is_some_and(|arg| arg == "-c") {
        if let Some(script) = args.get(1) {
            let old_stdin = std::mem::replace(&mut state.stdin, stdin);
            let positional = args.iter().skip(3).cloned().collect::<Vec<_>>();
            let old_positionals = set_positional_args(state, positional);
            let result = execute_control_script(state, script);
            restore_positional_args(state, old_positionals);
            state.stdin = old_stdin;
            return result;
        }
    }
    let script_path = resolve_path(&state.cwd, &args[0]);
    let script = match state.session.inner.fs.lock() {
        Ok(fs) => match fs.read_file(&script_path) {
            Ok(script) => script,
            Err(_) => {
                return stderr_result(
                    127,
                    format!("{shell_name}: {}: No such file or directory\n", args[0]),
                );
            }
        },
        Err(_) => return stderr_result(1, "bash: filesystem lock poisoned\n"),
    };
    let script = strip_shebang(&script);
    let old_stdin = std::mem::replace(&mut state.stdin, stdin);
    let old_positionals = set_positional_args(state, args[1..].to_vec());
    let result = execute_control_script(state, script);
    restore_positional_args(state, old_positionals);
    state.stdin = old_stdin;
    result
}

fn execute_custom_command(
    state: &ExecState<'_>,
    command: &str,
    args: &[String],
    stdin: &str,
) -> Option<CommandResult> {
    let custom = state.session.inner.custom_commands.get(command)?.clone();
    let context = JustBashCustomCommandContext {
        args: args.to_vec(),
        cwd: state.cwd.clone(),
        env: state.env.clone(),
        stdin: stdin.to_string(),
        session: state.session.clone(),
    };
    let result = custom.execute(context);
    Some(CommandResult {
        stdout: result.stdout,
        stderr: result.stderr,
        exit_code: result.exit_code,
        exit_requested: false,
    })
}

fn execute_executor_command(
    state: &ExecState<'_>,
    namespace: &str,
    args: &[String],
    stdin: &str,
) -> Option<CommandResult> {
    let executor = state.session.inner.executor.as_ref()?;
    let subcommands = executor.subcommands(namespace);
    if subcommands.is_empty() {
        return None;
    }
    if args.is_empty() || (args.len() == 1 && args[0] == "--help") {
        return Some(stdout_result(format_namespace_help(
            namespace,
            &subcommands,
        )));
    }
    let sub_name = &args[0];
    let Some(sub) = subcommands
        .iter()
        .find(|sub| &sub.name == sub_name || sub.aliases.iter().any(|alias| alias == sub_name))
    else {
        return Some(stderr_result(
            1,
            format!(
                "{namespace}: unknown command \"{sub_name}\"\nRun '{namespace} --help' for usage.\n"
            ),
        ));
    };
    if args.get(1).is_some_and(|arg| arg == "--help") {
        return Some(stdout_result(format_subcommand_help(namespace, sub)));
    }
    let parsed = match parse_tool_cli_args(&args[1..], stdin) {
        Ok(parsed) => parsed,
        Err(error) => return Some(stderr_result(1, format!("{}: {error}\n", sub.name))),
    };
    Some(match executor.invoke(&sub.original_path, parsed) {
        Ok(value) => stdout_result(format!("{value}\n")),
        Err(error) => stderr_result(1, format!("{}: {error}\n", sub.name)),
    })
}

fn parse_tool_cli_args(args: &[String], stdin: &str) -> Result<JsonValue, String> {
    let mut result = JsonMap::new();
    if let Ok(JsonValue::Object(object)) = serde_json::from_str::<JsonValue>(stdin.trim()) {
        result.extend(object);
    }
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        if arg == "--json" {
            let Some(raw) = args.get(index + 1) else {
                return Err("Invalid --json value: missing value".to_string());
            };
            merge_json_object(&mut result, raw)?;
            index += 2;
        } else if let Some(raw) = arg.strip_prefix("--json=") {
            merge_json_object(&mut result, raw)?;
            index += 1;
        } else if let Some(raw) = arg.strip_prefix("--") {
            if let Some((key, value)) = raw.split_once('=') {
                result.insert(key.to_string(), coerce_json_value(value));
            } else if args
                .get(index + 1)
                .is_some_and(|next| !next.starts_with("--"))
            {
                index += 1;
                result.insert(raw.to_string(), coerce_json_value(&args[index]));
            } else {
                result.insert(raw.to_string(), JsonValue::Bool(true));
            }
            index += 1;
        } else if let Some((key, value)) = arg.split_once('=') {
            result.insert(key.to_string(), coerce_json_value(value));
            index += 1;
        } else if args.len() == 1 && arg.starts_with('{') {
            match serde_json::from_str::<JsonValue>(arg) {
                Ok(JsonValue::Object(object)) => {
                    result.extend(object);
                }
                Ok(_) => return Err("positional JSON must be a JSON object".to_string()),
                Err(error) => return Err(format!("Invalid positional JSON: {error}")),
            }
            index += 1;
        } else {
            index += 1;
        }
    }
    Ok(JsonValue::Object(result))
}

fn merge_json_object(target: &mut JsonMap<String, JsonValue>, raw: &str) -> Result<(), String> {
    match serde_json::from_str::<JsonValue>(raw) {
        Ok(JsonValue::Object(object)) => {
            target.extend(object);
            Ok(())
        }
        Ok(_) => Err("--json must be a JSON object".to_string()),
        Err(error) => Err(format!("Invalid --json value: {error}")),
    }
}

fn coerce_json_value(raw: &str) -> JsonValue {
    serde_json::from_str(raw).unwrap_or_else(|_| JsonValue::String(raw.to_string()))
}

fn cancelled_result(state: &ExecState<'_>) -> Option<CommandResult> {
    if state
        .cancel_token
        .as_ref()
        .is_some_and(JustBashCancelToken::is_cancelled)
    {
        return Some(CommandResult {
            exit_code: JUST_BASH_TIMEOUT_EXIT_CODE,
            ..CommandResult::default()
        });
    }
    if state.started_at.elapsed() >= Duration::from_millis(state.timeout_ms) {
        return Some(stderr_result(
            JUST_BASH_TIMEOUT_EXIT_CODE,
            format!("Command timed out after {}ms", state.timeout_ms),
        ));
    }
    None
}

fn cap_output(result: &mut CommandResult, max: usize) -> bool {
    let mut truncated = false;
    if result.stdout.len() > max {
        result.stdout.truncate(max);
        truncated = true;
    }
    if result.stderr.len() > max {
        result.stderr.truncate(max);
        truncated = true;
    }
    truncated
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControlOp {
    Always,
    And,
    Or,
}

fn split_control(script: &str) -> Vec<(ControlOp, String)> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut op = ControlOp::Always;
    let mut quote = None::<char>;
    let mut chars = script.chars().peekable();
    while let Some(ch) = chars.next() {
        match quote {
            Some(q) if ch == q => {
                quote = None;
                current.push(ch);
            }
            Some(_) => current.push(ch),
            None if ch == '\'' || ch == '"' => {
                quote = Some(ch);
                current.push(ch);
            }
            None if ch == '&' && matches!(chars.peek(), Some('&')) => {
                chars.next();
                parts.push((op, current.trim().to_string()));
                current.clear();
                op = ControlOp::And;
            }
            None if ch == '|' && matches!(chars.peek(), Some('|')) => {
                chars.next();
                parts.push((op, current.trim().to_string()));
                current.clear();
                op = ControlOp::Or;
            }
            None if ch == ';' || ch == '\n' => {
                parts.push((op, current.trim().to_string()));
                current.clear();
                op = ControlOp::Always;
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        parts.push((op, current.trim().to_string()));
    }
    parts
}

fn split_pipeline(command: &str) -> Vec<String> {
    split_unquoted(command, '|')
}

fn split_unquoted(input: &str, separator: char) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote = None::<char>;
    let mut substitution_depth = 0usize;
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        match quote {
            Some(q) if ch == q => {
                quote = None;
                current.push(ch);
            }
            Some(_) => current.push(ch),
            None if ch == '\'' || ch == '"' => {
                quote = Some(ch);
                current.push(ch);
            }
            None if ch == '$' && matches!(chars.peek(), Some('(')) => {
                substitution_depth += 1;
                current.push(ch);
                current.push(chars.next().expect("peeked command substitution open"));
            }
            None if ch == ')' && substitution_depth > 0 => {
                substitution_depth -= 1;
                current.push(ch);
            }
            None if ch == separator && substitution_depth == 0 => {
                parts.push(current.trim().to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

fn split_stdout_redirection(command: &str) -> (&str, Option<(String, bool)>) {
    let mut quote = None::<char>;
    let chars = command.char_indices().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        let (byte, ch) = chars[index];
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch == '>' => {
                let previous = index.checked_sub(1).map(|idx| chars[idx].1);
                let next = chars.get(index + 1).map(|(_, ch)| *ch);
                if previous == Some('2') || next == Some('&') {
                    index += 1;
                    continue;
                }
                let append = next == Some('>');
                let target = command[byte + if append { 2 } else { 1 }..]
                    .trim()
                    .to_string();
                return (command[..byte].trim(), Some((target, append)));
            }
            _ => {}
        }
        index += 1;
    }
    (command, None)
}

fn split_stdin_redirection(command: &str) -> (&str, Option<String>) {
    let mut quote = None::<char>;
    let chars = command.char_indices().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        let (byte, ch) = chars[index];
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => {}
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch == '<' => {
                let target = command[byte + 1..].trim().to_string();
                return (command[..byte].trim(), Some(target));
            }
            _ => {}
        }
        index += 1;
    }
    (command, None)
}

fn tokenize(input: &str, env: &BTreeMap<String, String>) -> Result<Vec<String>, String> {
    let chars = input.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote = None::<char>;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        match quote {
            None if ch.is_whitespace() => {
                if started {
                    tokens.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            None if ch == '\'' || ch == '"' => {
                quote = Some(ch);
                started = true;
            }
            None | Some('"') if ch == '$' => {
                started = true;
                current.push_str(&read_variable(&chars, &mut index, env));
            }
            Some(q) if ch == q => quote = None,
            _ => {
                started = true;
                current.push(ch);
            }
        }
        index += 1;
    }
    if quote.is_some() {
        return Err("unterminated quoted string".to_string());
    }
    if started {
        tokens.push(current);
    }
    Ok(tokens)
}

fn read_variable(chars: &[char], index: &mut usize, env: &BTreeMap<String, String>) -> String {
    let start = *index;
    let Some(next) = chars.get(start + 1) else {
        return "$".to_string();
    };
    if *next == '{' {
        let mut end = start + 2;
        while end < chars.len() && chars[end] != '}' {
            end += 1;
        }
        if end >= chars.len() {
            return "$".to_string();
        }
        let expression = chars[start + 2..end].iter().collect::<String>();
        *index = end;
        if let Some((name, default)) = expression.split_once(":-") {
            env.get(name)
                .filter(|value| !value.is_empty())
                .cloned()
                .unwrap_or_else(|| default.to_string())
        } else {
            env.get(&expression).cloned().unwrap_or_default()
        }
    } else if next.is_ascii_alphabetic() || *next == '_' {
        let mut end = start + 1;
        while end < chars.len() && (chars[end].is_ascii_alphanumeric() || chars[end] == '_') {
            end += 1;
        }
        let name = chars[start + 1..end].iter().collect::<String>();
        *index = end - 1;
        env.get(&name).cloned().unwrap_or_default()
    } else if next.is_ascii_digit() || matches!(*next, '#' | '@' | '*') {
        *index = start + 1;
        env.get(&next.to_string()).cloned().unwrap_or_default()
    } else {
        "$".to_string()
    }
}

fn execute_assignment_substitution(
    state: &mut ExecState<'_>,
    command: &str,
    stdin: &str,
) -> Option<CommandResult> {
    let (name, rest) = command.split_once("=$(")?;
    if !is_valid_var_name(name.trim()) || !rest.ends_with(')') {
        return None;
    }
    let inner = &rest[..rest.len() - 1];
    let old_stdin = std::mem::replace(&mut state.stdin, stdin.to_string());
    let result = execute_control_script(state, inner);
    state.stdin = old_stdin;
    if result.exit_code == 0 {
        state.env.insert(
            name.trim().to_string(),
            result.stdout.trim_end_matches('\n').to_string(),
        );
    }
    Some(CommandResult {
        stdout: String::new(),
        stderr: result.stderr,
        exit_code: result.exit_code,
        exit_requested: result.exit_requested,
    })
}

fn set_positional_args(
    state: &mut ExecState<'_>,
    args: Vec<String>,
) -> Vec<(String, Option<String>)> {
    let mut keys = vec!["#".to_string(), "@".to_string(), "*".to_string()];
    keys.extend((0..=args.len()).map(|index| index.to_string()));
    let old_values = keys
        .iter()
        .map(|key| (key.clone(), state.env.get(key).cloned()))
        .collect::<Vec<_>>();
    state.env.insert("#".to_string(), args.len().to_string());
    let joined = args.join(" ");
    state.env.insert("@".to_string(), joined.clone());
    state.env.insert("*".to_string(), joined);
    for (index, value) in args.into_iter().enumerate() {
        state.env.insert((index + 1).to_string(), value);
    }
    old_values
}

fn restore_positional_args(state: &mut ExecState<'_>, old_values: Vec<(String, Option<String>)>) {
    for (key, value) in old_values {
        if let Some(value) = value {
            state.env.insert(key, value);
        } else {
            state.env.remove(&key);
        }
    }
}

fn strip_shebang(script: &str) -> &str {
    if let Some(rest) = script.strip_prefix("#!")
        && let Some(index) = rest.find('\n')
    {
        return &rest[index + 1..];
    }
    script
}

fn extract_function_definitions(script: &mut String, functions: &mut BTreeMap<String, String>) {
    while let Some(marker) = script.find("()") {
        let prefix = &script[..marker];
        let name_start = prefix
            .rfind(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
            .map_or(0, |index| index + 1);
        let name = script[name_start..marker].trim().to_string();
        let Some(open_offset) = script[marker + 2..].find('{') else {
            break;
        };
        let open = marker + 2 + open_offset;
        let Some(close_offset) = script[open + 1..].find('}') else {
            break;
        };
        let close = open + 1 + close_offset;
        functions.insert(name, script[open + 1..close].trim().to_string());
        let mut end = close + 1;
        while end < script.len()
            && script[end..]
                .chars()
                .next()
                .is_some_and(|ch| ch == ';' || ch.is_whitespace())
        {
            end += script[end..].chars().next().map_or(0, char::len_utf8);
        }
        script.replace_range(name_start..end, "");
    }
}

fn command_basename(command: &str) -> &str {
    command
        .strip_prefix("/bin/")
        .or_else(|| command.strip_prefix("/usr/bin/"))
        .unwrap_or(command)
}

fn default_env(cwd: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("HOME".to_string(), "/home/user".to_string()),
        ("PATH".to_string(), "/usr/bin:/bin".to_string()),
        ("IFS".to_string(), " \t\n".to_string()),
        ("OSTYPE".to_string(), "linux-gnu".to_string()),
        ("HOSTTYPE".to_string(), "x86_64".to_string()),
        ("HOSTNAME".to_string(), "localhost".to_string()),
        ("PWD".to_string(), cwd.to_string()),
        ("OLDPWD".to_string(), cwd.to_string()),
    ])
}

fn lock_poisoned_error() -> JustBashError {
    JustBashError::new(
        JustBashErrorKind::InvalidInput,
        "lock",
        "<filesystem>",
        "filesystem lock poisoned",
    )
}

fn stdout_result(stdout: impl Into<String>) -> CommandResult {
    CommandResult {
        stdout: stdout.into(),
        ..CommandResult::default()
    }
}

fn stderr_result(exit_code: i32, stderr: impl Into<String>) -> CommandResult {
    CommandResult {
        stderr: stderr.into(),
        exit_code,
        ..CommandResult::default()
    }
}

fn camel_to_kebab(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();
    let mut output = String::new();
    for (index, ch) in chars.iter().copied().enumerate() {
        if ch.is_ascii_uppercase() {
            let previous = index.checked_sub(1).and_then(|index| chars.get(index));
            let next = chars.get(index + 1);
            let starts_new_word = previous.is_some_and(|previous| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || (previous.is_ascii_uppercase() && next.is_some_and(char::is_ascii_lowercase))
            });
            if starts_new_word && !output.ends_with('-') {
                output.push('-');
            }
            output.push(ch.to_ascii_lowercase());
        } else {
            output.push(ch);
        }
    }
    output
}

fn format_namespace_help(namespace: &str, subcommands: &[ToolSubcommand]) -> String {
    let max_len = subcommands
        .iter()
        .map(|subcommand| subcommand.name.len())
        .max()
        .unwrap_or(0);
    let mut lines = vec![
        format!("Executor tools: {namespace}"),
        String::new(),
        "USAGE".to_string(),
        format!("  {namespace} <command> [flags]"),
        String::new(),
        "COMMANDS".to_string(),
    ];
    for subcommand in subcommands {
        let padding = " ".repeat(max_len.saturating_sub(subcommand.name.len()) + 4);
        lines.push(format!(
            "  {}{}{}",
            subcommand.name,
            padding,
            subcommand.description.as_deref().unwrap_or("")
        ));
    }
    lines.push(String::new());
    lines.push("EXAMPLES".to_string());
    if let Some(first) = subcommands.first() {
        lines.push(format!("  {namespace} {} key=value", first.name));
    }
    if let Some(second) = subcommands.get(1) {
        lines.push(format!("  {namespace} {} --key value", second.name));
    }
    lines.push(String::new());
    lines.push("LEARN MORE".to_string());
    lines.push(format!("  {namespace} <command> --help"));
    lines.push(String::new());
    lines.join("\n")
}

fn format_subcommand_help(namespace: &str, subcommand: &ToolSubcommand) -> String {
    let full = format!("{namespace} {}", subcommand.name);
    [
        subcommand.description.clone().unwrap_or_default(),
        String::new(),
        "USAGE".to_string(),
        format!("  {full} [key=value ...]"),
        format!("  {full} [--key value ...]"),
        format!("  {full} --json '{{...}}'"),
        format!("  <stdin> | {full}"),
        String::new(),
        "FLAGS".to_string(),
        "  --json string    Pass all arguments as a JSON object".to_string(),
        "  --help           Show this help".to_string(),
        String::new(),
        "EXAMPLES".to_string(),
        format!("  {full} key=value"),
        format!("  {full} --key value"),
        format!("  {full} --json '{{\"key\":\"value\"}}'"),
        format!("  echo '{{\"key\":\"value\"}}' | {full}"),
        format!("  {full} key=value | jq -r .field"),
        String::new(),
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn math_executor() -> JustBashExecutor {
        JustBashExecutor::new()
            .with_tool(
                "math.add",
                JustBashExecutorTool::new(Some("Add two numbers"), |args| {
                    let a = args.get("a").and_then(JsonValue::as_i64).unwrap_or(0);
                    let b = args.get("b").and_then(JsonValue::as_i64).unwrap_or(0);
                    Ok(json!({ "sum": a + b }))
                }),
            )
            .with_tool(
                "math.multiply",
                JustBashExecutorTool::new(Some("Multiply two numbers"), |args| {
                    let a = args.get("a").and_then(JsonValue::as_i64).unwrap_or(0);
                    let b = args.get("b").and_then(JsonValue::as_i64).unwrap_or(0);
                    Ok(json!({ "product": a * b }))
                }),
            )
            .with_tool(
                "util.echo",
                JustBashExecutorTool::new(Some("Echo arguments back"), Ok),
            )
    }

    fn fail_executor() -> JustBashExecutor {
        JustBashExecutor::new().with_tool(
            "fail.now",
            JustBashExecutorTool::new(None::<String>, |_args| Err("something broke".to_string())),
        )
    }

    fn api_executor() -> JustBashExecutor {
        JustBashExecutor::new().with_tool(
            "api.listUsers",
            JustBashExecutorTool::new(Some("List all users"), |_args| {
                Ok(json!([{ "name": "Alice" }]))
            }),
        )
    }

    fn hidden_executor() -> JustBashExecutor {
        JustBashExecutor::new()
            .with_tool(
                "calc.add",
                JustBashExecutorTool::new(None::<String>, |args| {
                    let a = args.get("a").and_then(JsonValue::as_i64).unwrap_or(0);
                    let b = args.get("b").and_then(JsonValue::as_i64).unwrap_or(0);
                    Ok(json!({ "sum": a + b }))
                }),
            )
            .with_expose_tools_as_commands(false)
    }

    fn countries_executor() -> JustBashExecutor {
        JustBashExecutor::new()
            .with_tool(
                "countries.country",
                JustBashExecutorTool::new(Some("Get a country by code"), |args| {
                    let code = args.get("code").and_then(JsonValue::as_str).unwrap_or("");
                    Ok(match code {
                        "JP" => json!({
                            "name": "Japan",
                            "capital": "Tokyo",
                            "continent": "Asia",
                        }),
                        "US" => json!({
                            "name": "United States",
                            "capital": "Washington D.C.",
                            "continent": "North America",
                        }),
                        "BR" => json!({
                            "name": "Brazil",
                            "capital": "Brasilia",
                            "continent": "South America",
                        }),
                        "AR" => json!({
                            "name": "Argentina",
                            "capital": "Buenos Aires",
                            "continent": "South America",
                        }),
                        _ => JsonValue::Null,
                    })
                }),
            )
            .with_tool(
                "countries.list",
                JustBashExecutorTool::new(Some("List all countries"), |args| {
                    let continent = args.get("continent").and_then(JsonValue::as_str);
                    let mut countries = vec![
                        json!({"code": "JP", "name": "Japan", "continent": "Asia"}),
                        json!({"code": "US", "name": "United States", "continent": "North America"}),
                        json!({"code": "BR", "name": "Brazil", "continent": "South America"}),
                        json!({"code": "AR", "name": "Argentina", "continent": "South America"}),
                    ];
                    if let Some(continent) = continent {
                        countries.retain(|country| {
                            country
                                .get("continent")
                                .and_then(JsonValue::as_str)
                                .is_some_and(|value| value == continent)
                        });
                    }
                    Ok(JsonValue::Array(countries))
                }),
            )
    }

    fn parsed_tool_args(args: &[&str], stdin: &str) -> JsonValue {
        parse_tool_cli_args(
            &args
                .iter()
                .map(|arg| (*arg).to_string())
                .collect::<Vec<_>>(),
            stdin,
        )
        .expect("tool CLI args should parse")
    }

    #[test]
    fn just_bash_executor_cli_helpers_match_upstream_tool_command_rows() {
        assert_eq!(camel_to_kebab("listPets"), "list-pets");
        assert_eq!(camel_to_kebab("getPetById"), "get-pet-by-id");
        assert_eq!(camel_to_kebab("createUser"), "create-user");
        assert_eq!(camel_to_kebab("add"), "add");
        assert_eq!(camel_to_kebab("list"), "list");
        assert_eq!(camel_to_kebab("parseXMLDocument"), "parse-xml-document");
        assert_eq!(camel_to_kebab("getHTTPResponse"), "get-http-response");
        assert_eq!(camel_to_kebab("already-kebab"), "already-kebab");

        assert_eq!(
            parsed_tool_args(&["a=1", "b=2"], ""),
            json!({"a": 1, "b": 2})
        );
        assert_eq!(
            parsed_tool_args(&["--a", "1", "--b", "2"], ""),
            json!({"a": 1, "b": 2})
        );
        assert_eq!(
            parsed_tool_args(&["--a=1", "--b=2"], ""),
            json!({"a": 1, "b": 2})
        );
        assert_eq!(
            parsed_tool_args(&["--json", r#"{"a":1,"b":2}"#], ""),
            json!({"a": 1, "b": 2})
        );
        assert_eq!(
            parsed_tool_args(&[r#"--json={"a":1}"#], ""),
            json!({"a": 1})
        );
        assert_eq!(parsed_tool_args(&[], ""), json!({}));
        let numeric_expected =
            serde_json::from_str::<JsonValue>(r#"{"a":42,"b":3.14,"c":-5}"#).unwrap();
        assert_eq!(
            parsed_tool_args(&["a=42", "b=3.14", "c=-5"], ""),
            numeric_expected
        );
        assert_eq!(
            parsed_tool_args(&["a=true", "b=false"], ""),
            json!({"a": true, "b": false})
        );
        assert_eq!(parsed_tool_args(&["a=null"], ""), json!({"a": null}));
        assert_eq!(
            parsed_tool_args(&["a=[1,2,3]"], ""),
            json!({"a": [1, 2, 3]})
        );
        assert_eq!(
            parsed_tool_args(&["name=hello", "path=/tmp/file"], ""),
            json!({"name": "hello", "path": "/tmp/file"})
        );
        assert_eq!(parsed_tool_args(&["a="], ""), json!({"a": ""}));
        assert_eq!(
            parsed_tool_args(&[r#"{"a":1,"b":2}"#], ""),
            json!({"a": 1, "b": 2})
        );
        assert_eq!(
            parsed_tool_args(&[], r#"{"a":1,"b":2}"#),
            json!({"a": 1, "b": 2})
        );
        assert_eq!(
            parsed_tool_args(&["a=99"], r#"{"a":1,"b":2}"#),
            json!({"a": 99, "b": 2})
        );
        assert_eq!(
            parsed_tool_args(&["--json", r#"{"a":99}"#], r#"{"a":1,"b":2}"#),
            json!({"a": 99, "b": 2})
        );
        assert_eq!(
            parsed_tool_args(&["--json", r#"{"a":1,"b":2}"#, "a=99"], ""),
            json!({"a": 99, "b": 2})
        );
        assert_eq!(
            parsed_tool_args(&["--verbose", "--debug"], ""),
            json!({"verbose": true, "debug": true})
        );
        assert_eq!(
            parsed_tool_args(&["a=1"], "not json at all"),
            json!({"a": 1})
        );

        let malformed_json =
            parse_tool_cli_args(&["--json".to_string(), r#"{"a":"#.to_string()], "");
        assert!(
            malformed_json
                .expect_err("malformed JSON should fail")
                .contains("Invalid --json value")
        );
        let array_json = parse_tool_cli_args(&[r#"--json=[1,2,3]"#.to_string()], "");
        assert!(
            array_json
                .expect_err("array JSON should fail")
                .contains("--json must be a JSON object")
        );
        let malformed_positional = parse_tool_cli_args(&[r#"{"a":"#.to_string()], "");
        assert!(
            malformed_positional
                .expect_err("malformed positional JSON should fail")
                .contains("Invalid positional JSON")
        );
    }

    #[test]
    fn structured_data_query_engine_safe_key_rows() {
        assert!(!is_safe_json_object_key("__proto__"));
        assert!(!is_safe_json_object_key("constructor"));
        assert!(!is_safe_json_object_key("prototype"));
        assert!(is_safe_json_object_key("name"));
        assert!(is_safe_json_object_key("__Proto__"));
        assert!(is_safe_json_object_key("CONSTRUCTOR"));

        let mut object = JsonMap::new();
        insert_json_object_key(&mut object, "a".to_string(), json!(1));
        insert_json_object_key(&mut object, "__proto__".to_string(), json!("polluted"));
        insert_json_object_key(&mut object, "b".to_string(), json!(2));
        insert_json_object_key(&mut object, "constructor".to_string(), json!("polluted"));
        assert_eq!(JsonValue::Object(object), json!({"a": 1, "b": 2}));

        let mut dangerous_only = JsonMap::new();
        insert_json_object_key(
            &mut dangerous_only,
            "__proto__".to_string(),
            json!("polluted"),
        );
        insert_json_object_key(
            &mut dangerous_only,
            "constructor".to_string(),
            json!("polluted"),
        );
        insert_json_object_key(
            &mut dangerous_only,
            "prototype".to_string(),
            json!("polluted"),
        );
        assert_eq!(JsonValue::Object(dangerous_only), json!({}));

        let safe_entries = json!([
            {"key": "a", "value": 1},
            {"key": "b", "value": 2}
        ]);
        assert_eq!(json_from_entries(&safe_entries), json!({"a": 1, "b": 2}));

        let entries = json!([
            {"key": "a", "value": "safe"},
            {"key": "__proto__", "value": "polluted"},
            {"key": "b", "value": "safe"},
            {"key": "constructor", "value": "polluted"}
        ]);
        assert_eq!(
            json_from_entries(&entries),
            json!({"a": "safe", "b": "safe"})
        );

        assert_eq!(json_from_entries(&json!([])), json!({}));
    }

    #[test]
    fn just_bash_core_stdout_stderr_status_redirection_and_chaining() {
        let bash = JustBashSession::with_options(
            JustBashSessionOptions::new().with_file("/test.txt", "hello\nworld\nhello\n"),
        );

        let piped = bash.exec("cat /test.txt | grep hello", JustBashExecOptions::new());
        assert_eq!(piped.exit_code, 0);
        assert_eq!(piped.stdout, "hello\nhello\n");

        let stderr = bash.exec("cat /missing", JustBashExecOptions::new());
        assert_ne!(stderr.exit_code, 0);
        assert!(stderr.stderr.contains("No such file or directory"));

        let redirected_stderr = bash.exec("echo err >&2", JustBashExecOptions::new());
        assert_eq!(redirected_stderr.stdout, "");
        assert_eq!(redirected_stderr.stderr, "err\n");

        let unknown = bash.exec("unknowncommand", JustBashExecOptions::new());
        assert_eq!(unknown.exit_code, 127);
        assert!(unknown.stderr.contains("command not found"));

        assert_eq!(
            bash.exec("false && echo no || echo yes", JustBashExecOptions::new())
                .stdout,
            "yes\n"
        );

        bash.exec("echo hello > /output.txt", JustBashExecOptions::new());
        bash.exec("echo again >> /output.txt", JustBashExecOptions::new());
        assert_eq!(bash.read_file("/output.txt").unwrap(), "hello\nagain\n");
    }

    #[test]
    fn just_bash_bash_general_export_does_not_persist_functions_reset_and_filesystem_persists() {
        let bash = JustBashSession::new();

        let first = bash.exec(
            "export FOO=bar; helper() { echo function-ok; }; helper; echo file > /tmp/file.txt",
            JustBashExecOptions::new(),
        );
        assert_eq!(first.exit_code, 0);
        assert_eq!(first.stdout, "function-ok\n");
        assert_eq!(first.env.get("FOO").map(String::as_str), Some("bar"));

        let second = bash.exec("echo $FOO; helper", JustBashExecOptions::new());
        assert_eq!(second.stdout, "\n");
        assert_eq!(second.exit_code, 127);
        assert!(second.stderr.contains("helper: command not found"));

        let third = bash.exec("cat /tmp/file.txt", JustBashExecOptions::new());
        assert_eq!(third.exit_code, 0);
        assert_eq!(third.stdout, "file\n");
    }

    #[test]
    fn just_bash_bash_general_env_result_export_unset_and_empty_command() {
        let bash = JustBashSession::with_options(
            JustBashSessionOptions::new()
                .with_env("INITIAL", "value")
                .with_env("FOO", "original")
                .with_env("TO_REMOVE", "value"),
        );

        let exported = bash.exec("export NEW_VAR=hello", JustBashExecOptions::new());
        assert_eq!(
            exported.env.get("INITIAL").map(String::as_str),
            Some("value")
        );
        assert_eq!(
            exported.env.get("NEW_VAR").map(String::as_str),
            Some("hello")
        );

        let modified = bash.exec("export FOO=modified", JustBashExecOptions::new());
        assert_eq!(
            modified.env.get("FOO").map(String::as_str),
            Some("modified")
        );

        let unset = bash.exec("unset TO_REMOVE", JustBashExecOptions::new());
        assert!(!unset.env.contains_key("TO_REMOVE"));

        let empty = bash.exec("", JustBashExecOptions::new());
        assert_eq!(empty.env.get("INITIAL").map(String::as_str), Some("value"));
    }

    #[test]
    fn just_bash_exec_options_env_replace_env_cwd_and_stdin_are_isolated() {
        let bash = JustBashSession::with_options(
            JustBashSessionOptions::new()
                .with_env("FOO", "base")
                .with_file("/work/input.txt", "content"),
        );

        let with_options = bash.exec(
            "pwd; echo \"$FOO:$BAR\"; cat input.txt; cat",
            JustBashExecOptions::new()
                .with_cwd("/work")
                .with_env("BAR", "exec")
                .with_stdin("stdin"),
        );
        assert_eq!(with_options.exit_code, 0);
        assert_eq!(with_options.stdout, "/work\nbase:exec\ncontentstdin");

        let restored = bash.exec("pwd; echo \"$BAR\"", JustBashExecOptions::new());
        assert_eq!(restored.stdout, "/home/user\n\n");

        let temporary_env = JustBashSession::new();
        let with_env = temporary_env.exec(
            "echo $FOO",
            JustBashExecOptions::new().with_env("FOO", "bar"),
        );
        assert_eq!(with_env.stdout, "bar\n");
        let without_env = temporary_env.exec("echo $FOO", JustBashExecOptions::new());
        assert_eq!(without_env.stdout, "\n");

        let override_env = JustBashSession::with_options(
            JustBashSessionOptions::new().with_env("VAR", "original"),
        );
        let overridden = override_env.exec(
            "echo $VAR",
            JustBashExecOptions::new().with_env("VAR", "override"),
        );
        assert_eq!(overridden.stdout, "override\n");
        assert_eq!(
            override_env
                .exec("echo $VAR", JustBashExecOptions::new())
                .stdout,
            "original\n"
        );

        let replaced_missing = bash.exec(
            "printenv FOO",
            JustBashExecOptions::new()
                .with_replace_env(true)
                .with_env("ONLY", "value"),
        );
        assert_eq!(replaced_missing.stdout, "");
        assert_eq!(replaced_missing.exit_code, 1);

        let replaced_present = bash.exec(
            "printenv ONLY",
            JustBashExecOptions::new()
                .with_replace_env(true)
                .with_env("ONLY", "value"),
        );
        assert_eq!(replaced_present.stdout, "value\n");
        assert_eq!(replaced_present.exit_code, 0);

        let replace_false = bash.exec(
            "printenv FOO; printenv EXTRA",
            JustBashExecOptions::new()
                .with_replace_env(false)
                .with_env("EXTRA", "added"),
        );
        assert_eq!(replace_false.stdout, "base\nadded\n");
    }

    #[test]
    fn jbc20_exec_scope_restores_env_cwd_after_errors_and_concurrent_runs() {
        let bash = JustBashSession::with_options(
            JustBashSessionOptions::new()
                .with_env("SHARED", "original")
                .with_env("VAR", "base")
                .with_file("/work/input.txt", "content"),
        );

        let multi_env = bash.exec(
            "echo \"$A $B $C\"; echo \"$MSG\"",
            JustBashExecOptions::new()
                .with_env("A", "1")
                .with_env("B", "2")
                .with_env("C", "3")
                .with_env("MSG", "hello world"),
        );
        assert_eq!(multi_env.stdout, "1 2 3\nhello world\n");
        assert!(!bash.get_env().contains_key("A"));
        assert!(!bash.get_env().contains_key("MSG"));

        let command_error = bash.exec(
            "missing_command",
            JustBashExecOptions::new()
                .with_env("VAR", "temporary")
                .with_env("TEMP_VAR", "temporary"),
        );
        assert_eq!(command_error.exit_code, 127);
        assert_eq!(
            bash.exec("echo \"$VAR:$TEMP_VAR\"", JustBashExecOptions::new())
                .stdout,
            "base:\n"
        );

        let parse_error = bash.exec(
            "echo \"unterminated",
            JustBashExecOptions::new()
                .with_cwd("/work")
                .with_env("VAR", "parse-temp"),
        );
        assert_eq!(parse_error.exit_code, 2);
        assert!(parse_error.stderr.contains("unterminated quoted string"));
        assert_eq!(
            bash.exec("pwd; echo $VAR", JustBashExecOptions::new())
                .stdout,
            "/home/user\nbase\n"
        );
        assert_eq!(bash.get_cwd(), "/home/user");

        let left = bash.clone();
        let right = bash.clone();
        let first = std::thread::spawn(move || {
            left.exec(
                "sleep 0.01; export VAR=left; echo \"$VAR $SHARED $OTHER\"",
                JustBashExecOptions::new().with_env("OTHER", "A"),
            )
        });
        let second = std::thread::spawn(move || {
            right.exec(
                "sleep 0.01; export VAR=right; echo \"$VAR $SHARED $OTHER\"",
                JustBashExecOptions::new().with_env("OTHER", "B"),
            )
        });
        let first = first.join().expect("left exec thread should complete");
        let second = second.join().expect("right exec thread should complete");
        assert_eq!(first.stdout, "left original A\n");
        assert_eq!(second.stdout, "right original B\n");
        assert_eq!(
            bash.exec("echo \"$VAR:$OTHER\"", JustBashExecOptions::new())
                .stdout,
            "base:\n"
        );
        assert_eq!(bash.get_env().get("VAR").map(String::as_str), Some("base"));
        assert!(!bash.get_env().contains_key("OTHER"));

        let command_set_var = bash.exec(
            "export NEW_VAR=created; export VAR=modified",
            JustBashExecOptions::new().with_env("TEMP", "temp"),
        );
        assert_eq!(
            command_set_var.env.get("NEW_VAR").map(String::as_str),
            Some("created")
        );
        assert_eq!(
            command_set_var.env.get("VAR").map(String::as_str),
            Some("modified")
        );
        assert!(!bash.get_env().contains_key("NEW_VAR"));
        assert!(!bash.get_env().contains_key("TEMP"));
        assert_eq!(bash.get_env().get("VAR").map(String::as_str), Some("base"));

        assert_eq!(
            bash.exec("sleep 0.001m; echo minute", JustBashExecOptions::new())
                .stdout,
            "minute\n"
        );
        assert_eq!(
            bash.exec("sleep 0.005 0.005; echo summed", JustBashExecOptions::new())
                .stdout,
            "summed\n"
        );
    }

    #[test]
    fn just_bash_exec_args_forwarding_appends_literal_args_once_to_first_command() {
        let bash = JustBashSession::new();

        let result = bash.exec(
            "printf '%s\\n'; echo second",
            JustBashExecOptions::new().with_args(["a b", "*.ts", "$HOME", "semi;colon"]),
        );

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "a b\n*.ts\n$HOME\nsemi;colon\nsecond\n");
    }

    #[test]
    fn just_bash_cancellation_timeout_returns_124_without_host_shell() {
        let bash = JustBashSession::new();

        let timeout = bash.exec(
            "sleep 1; echo never",
            JustBashExecOptions::new().with_timeout_ms(25),
        );
        assert_eq!(timeout.exit_code, JUST_BASH_TIMEOUT_EXIT_CODE);
        assert_eq!(timeout.stdout, "");
        assert!(timeout.stderr.contains("Command timed out after 25ms"));

        let cancel_token = JustBashCancelToken::new();
        cancel_token.cancel();
        let cancelled = bash.exec(
            "echo never",
            JustBashExecOptions::new().with_cancel_token(cancel_token),
        );
        assert_eq!(cancelled.exit_code, JUST_BASH_TIMEOUT_EXIT_CODE);

        let no_host_shell = bash.exec("sh -c 'printf portable'", JustBashExecOptions::new());
        assert_eq!(no_host_shell.exit_code, 0);
        assert_eq!(no_host_shell.stdout, "portable");
        assert_eq!(no_host_shell.stderr, "");
    }

    #[test]
    fn jbc20_timeout_command_rows_use_cooperative_in_process_cancellation() {
        let bash = JustBashSession::new();

        assert_eq!(
            bash.exec("timeout 10 echo hello", JustBashExecOptions::new())
                .stdout,
            "hello\n"
        );
        assert_eq!(
            bash.exec("timeout 10 echo one two three", JustBashExecOptions::new())
                .stdout,
            "one two three\n"
        );
        assert_eq!(
            bash.exec("timeout 5s echo seconds", JustBashExecOptions::new())
                .stdout,
            "seconds\n"
        );
        assert_eq!(
            bash.exec("timeout 1m echo minutes", JustBashExecOptions::new())
                .stdout,
            "minutes\n"
        );
        assert_eq!(
            bash.exec("timeout 0.5 echo decimal", JustBashExecOptions::new())
                .stdout,
            "decimal\n"
        );
        assert_eq!(
            bash.exec(
                "timeout --foreground -k 5 -s KILL 10 echo opts",
                JustBashExecOptions::new()
            )
            .stdout,
            "opts\n"
        );

        let missing_duration = bash.exec("timeout", JustBashExecOptions::new());
        assert_eq!(missing_duration.exit_code, 1);
        assert!(missing_duration.stderr.contains("missing operand"));
        let missing_command = bash.exec("timeout 5", JustBashExecOptions::new());
        assert_eq!(missing_command.exit_code, 1);
        assert!(missing_command.stderr.contains("missing operand"));
        let invalid = bash.exec("timeout abc echo test", JustBashExecOptions::new());
        assert_eq!(invalid.exit_code, 1);
        assert!(invalid.stderr.contains("invalid time interval"));
        let unknown = bash.exec("timeout --unknown 5 echo test", JustBashExecOptions::new());
        assert_eq!(unknown.exit_code, 1);
        assert!(unknown.stderr.contains("unrecognized option"));

        let timed_out = bash.exec("timeout 0.01 sleep 0.05", JustBashExecOptions::new());
        assert_eq!(timed_out.exit_code, JUST_BASH_TIMEOUT_EXIT_CODE);
        assert_eq!(timed_out.stdout, "");
        assert_eq!(timed_out.stderr, "");

        let no_side_effect = bash.exec(
            "timeout 0.01 bash -c 'sleep 0.05; echo LEAKED > /tmp/cancel-test'",
            JustBashExecOptions::new(),
        );
        assert_eq!(no_side_effect.exit_code, JUST_BASH_TIMEOUT_EXIT_CODE);
        assert!(!bash.file_exists("/tmp/cancel-test"));

        let multi_statement = bash.exec(
            "timeout 0.01 bash -c 'sleep 0.05; echo A > /tmp/a; echo B > /tmp/b; echo C > /tmp/c'",
            JustBashExecOptions::new(),
        );
        assert_eq!(multi_statement.exit_code, JUST_BASH_TIMEOUT_EXIT_CODE);
        assert!(!bash.file_exists("/tmp/a"));
        assert!(!bash.file_exists("/tmp/b"));
        assert!(!bash.file_exists("/tmp/c"));

        let help = bash.exec("timeout --help", JustBashExecOptions::new());
        assert_eq!(help.exit_code, 0);
        assert!(help.stdout.contains("timeout"));
        assert!(help.stdout.contains("DURATION"));
    }

    #[test]
    fn just_bash_optional_js_python_commands_fail_closed_without_host_runtime() {
        let bash = JustBashSession::new();
        let optional_runtime_commands = [
            "js-exec -c \"console.log('host-js')\"",
            "node -e \"console.log('host-node')\"",
            "python3 -c \"print('host-python3')\"",
            "python -c \"print('host-python')\"",
            "/usr/bin/node -e \"console.log('host-node')\"",
            "/usr/bin/python3 -c \"print('host-python3')\"",
            "/usr/bin/env node -e \"console.log('host-node')\"",
        ];

        assert!(!CommandRegistry::default_portable().contains("js-exec"));
        assert!(!CommandRegistry::default_portable().contains("node"));
        assert!(!CommandRegistry::default_portable().contains("python3"));
        assert!(!CommandRegistry::default_portable().contains("python"));

        for command in optional_runtime_commands {
            let result = bash.exec(command, JustBashExecOptions::new());
            assert_eq!(result.exit_code, 127, "{command}");
            assert_eq!(result.stdout, "", "{command}");
            assert!(result.stderr.contains("command not found"), "{command}");
            assert!(!result.stderr.contains("host-js"), "{command}");
            assert!(!result.stderr.contains("host-node"), "{command}");
            assert!(!result.stderr.contains("host-python"), "{command}");
            assert!(!result.metadata.external_sandbox, "{command}");
        }
    }

    #[test]
    fn just_bash_default_metadata_reports_in_process_backend() {
        let bash = JustBashSession::new();
        let result = bash.exec("echo ok", JustBashExecOptions::new());

        assert_eq!(result.stdout, "ok\n");
        assert_eq!(result.metadata.backend, JUST_BASH_BACKEND);
        assert!(!result.metadata.external_sandbox);
        assert_eq!(result.metadata.cwd, "/home/user");
        assert_eq!(result.metadata.command_count, 1);
        assert!(!result.metadata.truncated);
    }

    #[test]
    fn jbc20_pipeline_stderr_exit_status_and_metadata_rows_are_stable() {
        let bash = JustBashSession::with_options(
            JustBashSessionOptions::new().with_file("/data/file.txt", "hello\n"),
        );

        let first_error = bash.exec("ls /no_such_path_xyz | cat", JustBashExecOptions::new());
        assert_eq!(first_error.stdout, "");
        assert!(first_error.stderr.contains("No such file or directory"));
        let first_error_three_stage = bash.exec(
            "ls /no_such_path_xyz | cat | cat",
            JustBashExecOptions::new(),
        );
        assert!(
            first_error_three_stage
                .stderr
                .contains("No such file or directory")
        );
        let middle_error = bash.exec(
            "echo hello | ls /no_such_path_xyz | cat",
            JustBashExecOptions::new(),
        );
        assert!(middle_error.stderr.contains("No such file or directory"));
        let last_error = bash.exec(
            "echo hello | ls /no_such_path_xyz",
            JustBashExecOptions::new(),
        );
        assert!(last_error.stderr.contains("No such file or directory"));
        let multiple_errors = bash.exec(
            "ls /no_such_a | ls /no_such_b | cat",
            JustBashExecOptions::new(),
        );
        assert!(multiple_errors.stderr.contains("no_such_a"));
        assert!(multiple_errors.stderr.contains("no_such_b"));
        let mixed = bash.exec(
            "ls /data/file.txt /no_such_xyz | cat",
            JustBashExecOptions::new(),
        );
        assert!(mixed.stdout.contains("/data/file.txt"));
        assert!(mixed.stderr.contains("No such file or directory"));
        assert_eq!(
            bash.exec("echo hello | grep nomatch", JustBashExecOptions::new())
                .exit_code,
            1
        );

        let result = bash.exec(
            "echo ok",
            JustBashExecOptions::new()
                .with_cwd("/data")
                .with_timeout_ms(1234),
        );
        assert_eq!(result.metadata.backend, JUST_BASH_BACKEND);
        assert!(!result.metadata.external_sandbox);
        assert_eq!(result.metadata.cwd, "/data");
        assert_eq!(result.metadata.timeout_ms, 1234);
        assert_eq!(result.metadata.command_count, 1);
    }

    #[test]
    fn just_bash_executor_tool_command_parses_flags_json_stdin_and_errors() {
        let bash = JustBashSession::with_options(
            JustBashSessionOptions::new().with_executor(math_executor()),
        );

        let key_value = bash.exec("math add a=1 b=2", JustBashExecOptions::new());
        assert_eq!(key_value.stdout, "{\"sum\":3}\n");

        let flags = bash.exec("math add --a 1 --b 2", JustBashExecOptions::new());
        assert_eq!(flags.stdout, "{\"sum\":3}\n");

        let equals_flags = bash.exec("math add --a=1 --b=2", JustBashExecOptions::new());
        assert_eq!(equals_flags.stdout, "{\"sum\":3}\n");

        let json = bash.exec(
            "math add --json '{\"a\":10,\"b\":20}'",
            JustBashExecOptions::new(),
        );
        assert_eq!(json.stdout, "{\"sum\":30}\n");

        let stdin = bash.exec(
            "echo '{\"a\":5,\"b\":3}' | math add",
            JustBashExecOptions::new(),
        );
        assert_eq!(stdin.stdout, "{\"sum\":8}\n");

        let malformed = bash.exec("math add --json '{\"a\":'", JustBashExecOptions::new());
        assert_eq!(malformed.exit_code, 1);
        assert!(malformed.stderr.contains("Invalid --json value"));

        let piped = bash.exec("math add a=1 b=2 | jq -r .sum", JustBashExecOptions::new());
        assert_eq!(piped.exit_code, 0);
        assert_eq!(piped.stdout, "3\n");

        let help = bash.exec("math --help", JustBashExecOptions::new());
        assert_eq!(help.exit_code, 0);
        assert!(help.stdout.contains("Executor tools: math"));
        assert!(help.stdout.contains("COMMANDS"));
        assert!(help.stdout.contains("add"));
        assert!(help.stdout.contains("multiply"));
        assert!(help.stdout.contains("Add two numbers"));

        let namespace_help = bash.exec("math", JustBashExecOptions::new());
        assert_eq!(namespace_help.exit_code, 0);
        assert!(namespace_help.stdout.contains("COMMANDS"));
        assert!(namespace_help.stdout.contains("LEARN MORE"));

        let subcommand_help = bash.exec("math add --help", JustBashExecOptions::new());
        assert_eq!(subcommand_help.exit_code, 0);
        assert!(subcommand_help.stdout.contains("Add two numbers"));
        assert!(subcommand_help.stdout.contains("USAGE"));
        assert!(subcommand_help.stdout.contains("EXAMPLES"));
        assert!(subcommand_help.stdout.contains("--json"));
        assert!(subcommand_help.stdout.contains("math add"));

        let unknown = bash.exec("math nonexistent", JustBashExecOptions::new());
        assert_eq!(unknown.exit_code, 1);
        assert!(unknown.stderr.contains("unknown command \"nonexistent\""));
        assert!(unknown.stderr.contains("--help"));

        let util = bash.exec(
            "util echo --json '{\"hello\":\"world\"}'",
            JustBashExecOptions::new(),
        );
        assert_eq!(util.exit_code, 0);
        assert_eq!(util.stdout, "{\"hello\":\"world\"}\n");

        let multiply = bash.exec("math multiply a=3 b=4", JustBashExecOptions::new());
        assert_eq!(multiply.exit_code, 0);
        assert_eq!(multiply.stdout, "{\"product\":12}\n");

        let chained = bash.exec(
            "math add a=10 b=20 | jq -r .sum",
            JustBashExecOptions::new(),
        );
        assert_eq!(chained.exit_code, 0);
        assert_eq!(chained.stdout, "30\n");

        let failing = JustBashSession::with_options(
            JustBashSessionOptions::new().with_executor(fail_executor()),
        );
        let failure = failing.exec("fail now", JustBashExecOptions::new());
        assert_eq!(failure.exit_code, 1);
        assert!(failure.stderr.contains("something broke"));

        let hidden = JustBashSession::with_options(
            JustBashSessionOptions::new().with_executor(hidden_executor()),
        );
        let hidden_result = hidden.exec("calc add a=1 b=2", JustBashExecOptions::new());
        assert_eq!(hidden_result.exit_code, 127);
        assert!(hidden_result.stderr.contains("command not found"));

        let aliases = JustBashSession::with_options(
            JustBashSessionOptions::new().with_executor(api_executor()),
        );
        let kebab = aliases.exec("api list-users", JustBashExecOptions::new());
        assert_eq!(kebab.exit_code, 0);
        assert!(kebab.stdout.contains("Alice"));
        let camel = aliases.exec("api listUsers", JustBashExecOptions::new());
        assert_eq!(camel.exit_code, 0);
        assert!(camel.stdout.contains("Alice"));
    }

    #[test]
    fn just_bash_executor_custom_source_example_rows_use_virtual_session_state() {
        let bash = JustBashSession::with_options(
            JustBashSessionOptions::new().with_executor(countries_executor()),
        );

        let country = bash.exec(
            "countries country code=JP | jq -r .name",
            JustBashExecOptions::new(),
        );
        assert_eq!(country.exit_code, 0);
        assert_eq!(country.stdout, "Japan\n");

        let all = bash.exec(
            "countries list | jq -r 'length'",
            JustBashExecOptions::new(),
        );
        assert_eq!(all.exit_code, 0);
        assert_eq!(all.stdout, "4\n");

        let filtered = bash.exec(
            "countries list continent='South America' | jq -r 'length'",
            JustBashExecOptions::new(),
        );
        assert_eq!(filtered.exit_code, 0);
        assert_eq!(filtered.stdout, "2\n");

        let detail = bash.exec(
            "countries country code=US | jq -r .capital",
            JustBashExecOptions::new(),
        );
        assert_eq!(detail.exit_code, 0);
        assert_eq!(detail.stdout, "Washington D.C.\n");

        let write = bash.exec(
            "countries list continent='South America' > /tmp/countries.json",
            JustBashExecOptions::new(),
        );
        assert_eq!(write.exit_code, 0);

        let read = bash.exec(
            "cat /tmp/countries.json | jq -r 'length'",
            JustBashExecOptions::new(),
        );
        assert_eq!(read.exit_code, 0);
        assert_eq!(read.stdout, "2\n");

        let stored = bash.read_file("/tmp/countries.json").unwrap();
        assert!(stored.contains("Brazil"));
        assert!(stored.contains("Argentina"));
    }
}
