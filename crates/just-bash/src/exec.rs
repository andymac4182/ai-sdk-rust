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
use crate::encoding::OutputPayload;
use crate::error::{JustBashError, JustBashErrorKind, JustBashResult};
use crate::fs::{CpOptions, DirentEntry, FileStat, MkdirOptions, RmOptions, VirtualFileSystem};
use crate::path::resolve_path;

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
#[derive(Clone, Debug, Default)]
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
}

impl JustBashSession {
    /// Creates a memory-backed session with the upstream-style default layout.
    pub fn new() -> Self {
        Self::with_options(JustBashSessionOptions::new())
    }

    /// Creates a memory-backed session.
    pub fn with_options(options: JustBashSessionOptions) -> Self {
        let cwd = options.cwd.unwrap_or_else(|| "/home/user".to_string());
        let mut fs = VirtualFileSystem::new();
        for dir in ["/tmp", "/home", "/home/user", &cwd] {
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
                commands: options
                    .commands
                    .as_deref()
                    .map(CommandRegistry::filtered)
                    .unwrap_or_default(),
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
        self.inner.commands.names()
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
        "ls" => command_ls(state, &tokens[1..]),
        "mkdir" => command_mkdir(state, &tokens[1..]),
        "touch" => command_touch(state, &tokens[1..]),
        "rm" => command_rm(state, &tokens[1..]),
        "cp" => command_cp(state, &tokens[1..]),
        "mv" => command_mv(state, &tokens[1..]),
        "find" => command_find(state, &tokens[1..]),
        "read" => command_read(state, &tokens[1..], &stdin),
        "jq" => command_jq(state, &tokens[1..], &stdin),
        "yq" => command_yq(state, &tokens[1..], &stdin),
        "xan" => command_xan(state, &tokens[1..], &stdin),
        "sqlite3" => command_sqlite3(state, &tokens[1..], &stdin),
        "which" => command_which(state, &tokens[1..]),
        "whoami" => stdout_result("user\n"),
        "sleep" => command_sleep(state, &tokens[1..]),
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
        output = output
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\r", "\r");
    }
    if newline {
        output.push('\n');
    }
    stdout_result(output)
}

fn command_printf(args: &[String]) -> CommandResult {
    let Some(format) = args.first() else {
        return CommandResult::default();
    };
    let mut output = String::new();
    let mut arg_index = 1;
    let placeholder_count = count_printf_conversions(format);
    if placeholder_count == 0 {
        output.push_str(&render_printf_format(format, &[], &mut arg_index));
        return stdout_result(output);
    }
    while arg_index < args.len() {
        output.push_str(&render_printf_format(format, args, &mut arg_index));
    }
    stdout_result(output)
}

fn render_printf_format(format: &str, args: &[String], arg_index: &mut usize) -> String {
    let mut output = String::new();
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '%' if matches!(chars.peek(), Some('%')) => {
                chars.next();
                output.push('%');
            }
            '%' => {
                let specifier = chars.next().unwrap_or('%');
                let value = args.get(*arg_index).map(String::as_str).unwrap_or("");
                if matches!(specifier, 's' | 'd' | 'i' | 'f' | 'x' | 'X' | 'o') {
                    *arg_index += usize::from(*arg_index < args.len());
                }
                match specifier {
                    's' => output.push_str(value),
                    'd' | 'i' => output.push_str(&value.parse::<i64>().unwrap_or(0).to_string()),
                    'f' => output.push_str(&format!("{:.6}", value.parse::<f64>().unwrap_or(0.0))),
                    'x' => output.push_str(&format!("{:x}", value.parse::<i64>().unwrap_or(0))),
                    'X' => output.push_str(&format!("{:X}", value.parse::<i64>().unwrap_or(0))),
                    'o' => output.push_str(&format!("{:o}", value.parse::<i64>().unwrap_or(0))),
                    other => {
                        output.push('%');
                        output.push(other);
                    }
                }
            }
            '\\' => match chars.next() {
                Some('n') => output.push('\n'),
                Some('t') => output.push('\t'),
                Some('r') => output.push('\r'),
                Some('e') | Some('E') => output.push('\u{001b}'),
                Some('x') => {
                    let mut hex = String::new();
                    for _ in 0..2 {
                        if let Some(next) = chars.peek().copied()
                            && next.is_ascii_hexdigit()
                        {
                            hex.push(next);
                            chars.next();
                        }
                    }
                    if let Ok(value) = u8::from_str_radix(&hex, 16) {
                        output.push(value as char);
                    }
                }
                Some(first @ '0'..='7') => {
                    let mut octal = String::from(first);
                    for _ in 0..2 {
                        if let Some(next) = chars.peek().copied()
                            && matches!(next, '0'..='7')
                        {
                            octal.push(next);
                            chars.next();
                        }
                    }
                    if let Ok(value) = u8::from_str_radix(&octal, 8) {
                        output.push(value as char);
                    }
                }
                Some(other) => {
                    output.push('\\');
                    output.push(other);
                }
                None => output.push('\\'),
            },
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
            Some('s' | 'd' | 'i' | 'f' | 'x' | 'X' | 'o') => count += 1,
            Some(_) => {}
        }
    }
    count
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
            "-e" => {
                pattern = args.get(index + 1).cloned();
                index += 2;
                break;
            }
            _ if arg.starts_with('-') && arg.len() > 1 => {
                for flag in arg[1..].chars() {
                    match flag {
                        'i' => ignore_case = true,
                        'v' => invert = true,
                        'n' => line_number = true,
                        'c' => count_only = true,
                        'l' => files_with_matches = true,
                        'r' | 'R' => recursive = true,
                        'w' => word_regexp = true,
                        'E' | 'F' => {}
                        _ => {}
                    }
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
    let matcher = match LineMatcher::new(&pattern, ignore_case, word_regexp, mode) {
        Ok(matcher) => matcher,
        Err(error) => return stderr_result(2, format!("grep: {error}\n")),
    };
    let mut stdout = String::new();
    let mut matches = 0;
    let show_filename = paths.len() > 1 || recursive;
    for input in inputs {
        let mut file_matched = false;
        let mut file_matches = 0;
        for (line_index, line) in input.text.lines().enumerate() {
            let matched = matcher.is_match(line);
            if matched ^ invert {
                matches += 1;
                file_matches += 1;
                file_matched = true;
                if files_with_matches {
                    continue;
                }
                if count_only {
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
        }
    }
    CommandResult {
        exit_code: if matches == 0 { 1 } else { 0 },
        stdout,
        ..CommandResult::default()
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
    fn new(
        pattern: &str,
        ignore_case: bool,
        word_regexp: bool,
        mode: GrepMode,
    ) -> Result<Self, String> {
        Self::new_with_line_regexp(pattern, ignore_case, word_regexp, false, mode)
    }

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

fn command_find(state: &ExecState<'_>, args: &[String]) -> CommandResult {
    let root_arg = args
        .iter()
        .find(|arg| !arg.starts_with('-') && arg.as_str() != "f")
        .map(String::as_str)
        .unwrap_or(".");
    let root = resolve_path(&state.cwd, root_arg);
    let name_pattern = args
        .windows(2)
        .find_map(|window| (window[0] == "-name").then_some(window[1].as_str()));
    let type_filter = args
        .windows(2)
        .find_map(|window| (window[0] == "-type").then_some(window[1].as_str()));
    let fs = match state.session.inner.fs.lock() {
        Ok(fs) => fs,
        Err(_) => return stderr_result(1, "find: filesystem lock poisoned\n"),
    };
    let mut paths = fs
        .get_all_paths()
        .into_iter()
        .filter(|path| path == &root || path.starts_with(&format!("{root}/")))
        .filter(|path| {
            let Ok(stat) = fs.stat(path) else {
                return false;
            };
            match type_filter {
                Some("f") => stat.is_file,
                Some("d") => stat.is_directory,
                _ => true,
            }
        })
        .filter(|path| {
            name_pattern.is_none_or(|pattern| {
                wildcard_match(pattern, path.rsplit('/').next().unwrap_or(path))
            })
        })
        .collect::<Vec<_>>();
    paths.sort();
    stdout_result(
        paths
            .into_iter()
            .map(|path| format!("{path}\n"))
            .collect::<String>(),
    )
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
                    let replacement = replacement.replace('&', "$0");
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
    };
    let mut nr = 0usize;
    let mut last_filename = String::new();

    for rule in program
        .rules
        .iter()
        .filter(|rule| matches!(rule.pattern, AwkPattern::Begin))
    {
        let context = AwkRecordContext::empty(nr, &last_filename);
        if let Err(error) =
            execute_awk_actions(rule.actions.as_slice(), &context, &mut runtime, &mut stdout)
        {
            return stderr_result(1, format!("awk: {error}\n"));
        }
    }

    for input in &inputs {
        last_filename.clone_from(&input.label);
        for (line_index, line) in input.text.lines().enumerate() {
            nr += 1;
            let fnr = line_index + 1;
            let fields = awk_fields(line, &runtime.separator);
            let context = AwkRecordContext {
                line,
                fields,
                nr,
                fnr,
                filename: &input.label,
            };
            for rule in program.rules.iter().filter(|rule| rule.pattern.is_record()) {
                if rule.pattern.matches(&context, &runtime) {
                    if let Err(error) = execute_awk_actions(
                        rule.actions.as_slice(),
                        &context,
                        &mut runtime,
                        &mut stdout,
                    ) {
                        return stderr_result(1, format!("awk: {error}\n"));
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
        let context = AwkRecordContext::empty(nr, &last_filename);
        if let Err(error) =
            execute_awk_actions(rule.actions.as_slice(), &context, &mut runtime, &mut stdout)
        {
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
}

#[derive(Clone, Debug)]
struct AwkRecordContext<'a> {
    line: &'a str,
    fields: Vec<String>,
    nr: usize,
    fnr: usize,
    filename: &'a str,
}

#[derive(Clone, Debug)]
enum AwkPattern {
    Begin,
    End,
    Always,
    Regex(Regex),
    Condition(AwkCondition),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AwkCondition {
    Comparison {
        left: AwkExpr,
        op: AwkCompareOp,
        right: AwkExpr,
    },
    And(Box<AwkCondition>, Box<AwkCondition>),
    Or(Box<AwkCondition>, Box<AwkCondition>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AwkCompareOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AwkAction {
    Assign { name: String, value: AwkExpr },
    Print(Vec<AwkExpr>),
    Printf { format: AwkExpr, args: Vec<AwkExpr> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AwkExpr {
    Concat(Vec<AwkAtom>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum AwkAtom {
    WholeLine,
    Field(usize),
    LastField,
    Identifier(String),
    Literal(String),
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

impl<'a> AwkRecordContext<'a> {
    fn empty(nr: usize, filename: &'a str) -> Self {
        Self {
            line: "",
            fields: Vec::new(),
            nr,
            fnr: 0,
            filename,
        }
    }
}

impl AwkPattern {
    fn is_record(&self) -> bool {
        !matches!(self, Self::Begin | Self::End)
    }

    fn matches(&self, context: &AwkRecordContext<'_>, runtime: &AwkRuntime) -> bool {
        match self {
            Self::Begin | Self::End => false,
            Self::Always => true,
            Self::Regex(regex) => regex.is_match(context.line),
            Self::Condition(condition) => condition.matches(context, runtime),
        }
    }
}

impl AwkCondition {
    fn matches(&self, context: &AwkRecordContext<'_>, runtime: &AwkRuntime) -> bool {
        match self {
            Self::Comparison { left, op, right } => {
                let left = eval_awk_expr(left, context, runtime);
                let right = eval_awk_expr(right, context, runtime);
                compare_awk_values(&left, *op, &right)
            }
            Self::And(left, right) => {
                left.matches(context, runtime) && right.matches(context, runtime)
            }
            Self::Or(left, right) => {
                left.matches(context, runtime) || right.matches(context, runtime)
            }
        }
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
        } else if source.as_bytes().get(cursor) == Some(&b'/') {
            let (pattern, next_cursor) = parse_awk_regex_pattern(source, cursor)?;
            cursor = skip_awk_whitespace(source, next_cursor);
            if source.as_bytes().get(cursor) == Some(&b'{') {
                (pattern, cursor)
            } else {
                rules.push(AwkRule {
                    pattern,
                    actions: vec![AwkAction::Print(vec![AwkExpr::whole_line()])],
                });
                cursor = next_cursor;
                continue;
            }
        } else if let Some(block_start) = find_next_awk_block(source, cursor) {
            let condition = source[cursor..block_start].trim();
            let pattern = AwkPattern::Condition(parse_awk_condition(condition)?);
            (pattern, block_start)
        } else {
            let condition = source[cursor..].trim();
            rules.push(AwkRule {
                pattern: AwkPattern::Condition(parse_awk_condition(condition)?),
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
        Self::Concat(vec![AwkAtom::WholeLine])
    }
}

fn parse_awk_actions(body: &str) -> Result<Vec<AwkAction>, String> {
    let mut actions = Vec::new();
    for statement in split_awk_top_level(body, ';') {
        let statement = statement.trim();
        if statement.is_empty() {
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
        if let Some(index) = find_awk_assignment(statement) {
            let name = statement[..index].trim();
            if !is_awk_identifier(name) {
                return Err("unsupported program".to_string());
            }
            let value = parse_awk_concat_expr(statement[index + 1..].trim())?;
            actions.push(AwkAction::Assign {
                name: name.to_string(),
                value,
            });
            continue;
        }
        return Err("unsupported program".to_string());
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
        format: parse_awk_concat_expr(format.trim())?,
        args: parts
            .iter()
            .skip(1)
            .map(|part| parse_awk_concat_expr(part.trim()))
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
        .map(|entry| parse_awk_concat_expr(entry.trim()))
        .collect()
}

fn parse_awk_concat_expr(value: &str) -> Result<AwkExpr, String> {
    let mut atoms = Vec::new();
    let mut cursor = 0;
    let value = value.trim();
    while cursor < value.len() {
        cursor = skip_awk_whitespace(value, cursor);
        if cursor >= value.len() {
            break;
        }
        let rest = &value[cursor..];
        if rest.starts_with('"') {
            let end = find_awk_string_end(value, cursor)
                .ok_or_else(|| "unterminated string literal".to_string())?;
            atoms.push(AwkAtom::Literal(awk_unescape_string(
                &value[cursor + 1..end],
            )));
            cursor = end + 1;
            continue;
        }
        if rest == "$0" || rest.starts_with("$0 ") {
            atoms.push(AwkAtom::WholeLine);
            cursor += 2;
            continue;
        }
        if rest == "$NF" || rest.starts_with("$NF ") {
            atoms.push(AwkAtom::LastField);
            cursor += 3;
            continue;
        }
        if let Some(field_digits) = rest.strip_prefix('$').and_then(take_awk_digits) {
            let field = field_digits
                .parse::<usize>()
                .map_err(|_| "unsupported program".to_string())?;
            atoms.push(AwkAtom::Field(field));
            cursor += 1 + field_digits.len();
            continue;
        }
        if let Some(identifier) = take_awk_identifier(rest) {
            atoms.push(AwkAtom::Identifier(identifier.to_string()));
            cursor += identifier.len();
            continue;
        }
        if let Some(number) = take_awk_number(rest) {
            atoms.push(AwkAtom::Literal(number.to_string()));
            cursor += number.len();
            continue;
        }
        return Err("unsupported program".to_string());
    }

    if atoms.is_empty() {
        return Err("unsupported program".to_string());
    }
    Ok(AwkExpr::Concat(atoms))
}

fn parse_awk_condition(value: &str) -> Result<AwkCondition, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("unsupported program".to_string());
    }
    if let Some(index) = find_awk_operator(value, "||") {
        return Ok(AwkCondition::Or(
            Box::new(parse_awk_condition(&value[..index])?),
            Box::new(parse_awk_condition(&value[index + 2..])?),
        ));
    }
    if let Some(index) = find_awk_operator(value, "&&") {
        return Ok(AwkCondition::And(
            Box::new(parse_awk_condition(&value[..index])?),
            Box::new(parse_awk_condition(&value[index + 2..])?),
        ));
    }
    for (token, op) in [
        (">=", AwkCompareOp::Ge),
        ("<=", AwkCompareOp::Le),
        ("==", AwkCompareOp::Eq),
        ("!=", AwkCompareOp::Ne),
        (">", AwkCompareOp::Gt),
        ("<", AwkCompareOp::Lt),
    ] {
        if let Some(index) = find_awk_operator(value, token) {
            return Ok(AwkCondition::Comparison {
                left: parse_awk_concat_expr(&value[..index])?,
                op,
                right: parse_awk_concat_expr(&value[index + token.len()..])?,
            });
        }
    }
    Err("unsupported program".to_string())
}

fn execute_awk_actions(
    actions: &[AwkAction],
    context: &AwkRecordContext<'_>,
    runtime: &mut AwkRuntime,
    stdout: &mut String,
) -> Result<(), String> {
    for action in actions {
        match action {
            AwkAction::Assign { name, value } => {
                let value = eval_awk_expr(value, context, runtime);
                match name.as_str() {
                    "FS" => runtime.separator = AwkSeparator::from_value(&value),
                    "OFS" => runtime.ofs = value,
                    "ORS" => runtime.ors = value,
                    _ => {
                        runtime.variables.insert(name.clone(), value);
                    }
                }
            }
            AwkAction::Print(expressions) => {
                let values = expressions
                    .iter()
                    .map(|expr| eval_awk_expr(expr, context, runtime))
                    .collect::<Vec<_>>();
                stdout.push_str(&values.join(&runtime.ofs));
                stdout.push_str(&runtime.ors);
            }
            AwkAction::Printf { format, args } => {
                let format = eval_awk_expr(format, context, runtime);
                let values = args
                    .iter()
                    .map(|expr| eval_awk_expr(expr, context, runtime))
                    .collect::<Vec<_>>();
                stdout.push_str(&format_awk_printf(&format, &values)?);
            }
        }
    }
    Ok(())
}

fn eval_awk_expr(
    expression: &AwkExpr,
    context: &AwkRecordContext<'_>,
    runtime: &AwkRuntime,
) -> String {
    match expression {
        AwkExpr::Concat(atoms) => atoms
            .iter()
            .map(|atom| eval_awk_atom(atom, context, runtime))
            .collect::<Vec<_>>()
            .join(""),
    }
}

fn eval_awk_atom(atom: &AwkAtom, context: &AwkRecordContext<'_>, runtime: &AwkRuntime) -> String {
    match atom {
        AwkAtom::WholeLine => context.line.to_string(),
        AwkAtom::Field(0) => context.line.to_string(),
        AwkAtom::Field(field) => context
            .fields
            .get(field.saturating_sub(1))
            .cloned()
            .unwrap_or_default(),
        AwkAtom::LastField => context.fields.last().cloned().unwrap_or_default(),
        AwkAtom::Identifier(identifier) => match identifier.as_str() {
            "NR" => context.nr.to_string(),
            "FNR" => context.fnr.to_string(),
            "NF" => context.fields.len().to_string(),
            "FILENAME" => context.filename.to_string(),
            "FS" => runtime.separator.as_value(),
            "OFS" => runtime.ofs.clone(),
            "ORS" => runtime.ors.clone(),
            _ => runtime
                .variables
                .get(identifier)
                .cloned()
                .unwrap_or_default(),
        },
        AwkAtom::Literal(value) => value.clone(),
    }
}

fn compare_awk_values(left: &str, op: AwkCompareOp, right: &str) -> bool {
    let numeric = left.parse::<f64>().ok().zip(right.parse::<f64>().ok());
    if let Some((left, right)) = numeric {
        return match op {
            AwkCompareOp::Eq => left == right,
            AwkCompareOp::Ne => left != right,
            AwkCompareOp::Gt => left > right,
            AwkCompareOp::Ge => left >= right,
            AwkCompareOp::Lt => left < right,
            AwkCompareOp::Le => left <= right,
        };
    }
    match op {
        AwkCompareOp::Eq => left == right,
        AwkCompareOp::Ne => left != right,
        AwkCompareOp::Gt => left > right,
        AwkCompareOp::Ge => left >= right,
        AwkCompareOp::Lt => left < right,
        AwkCompareOp::Le => left <= right,
    }
}

fn format_awk_printf(format: &str, args: &[String]) -> Result<String, String> {
    let mut output = String::new();
    let mut chars = format.chars().peekable();
    let mut arg_index = 0usize;
    while let Some(ch) = chars.next() {
        if ch != '%' {
            output.push(ch);
            continue;
        }
        let Some(specifier) = chars.next() else {
            return Err("invalid printf format".to_string());
        };
        if specifier == '%' {
            output.push('%');
            continue;
        }
        let value = args.get(arg_index).cloned().unwrap_or_default();
        arg_index += 1;
        match specifier {
            's' => output.push_str(&value),
            'd' | 'i' => {
                let number = value.parse::<f64>().unwrap_or_default() as i64;
                output.push_str(&number.to_string());
            }
            _ => return Err("unsupported printf format".to_string()),
        }
    }
    Ok(output)
}

fn parse_awk_regex_pattern(source: &str, start: usize) -> Result<(AwkPattern, usize), String> {
    let mut cursor = start + 1;
    while cursor < source.len() {
        if source.as_bytes()[cursor] == b'/' && !is_awk_escaped(source, cursor) {
            let pattern = &source[start + 1..cursor];
            let regex = Regex::new(pattern).map_err(|error| error.to_string())?;
            return Ok((AwkPattern::Regex(regex), cursor + 1));
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
    while cursor < source.len() {
        let ch = source.as_bytes()[cursor];
        if ch == b'"' && !is_awk_escaped(source, cursor) {
            in_string = !in_string;
        } else if ch == b'{' && !in_string {
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
    while cursor < source.len() {
        let ch = source.as_bytes()[cursor];
        if ch == b'"' && !is_awk_escaped(source, cursor) {
            in_string = !in_string;
        } else if !in_string {
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

fn find_awk_assignment(statement: &str) -> Option<usize> {
    let mut cursor = 0usize;
    let mut in_string = false;
    while cursor < statement.len() {
        let ch = statement.as_bytes()[cursor];
        if ch == b'"' && !is_awk_escaped(statement, cursor) {
            in_string = !in_string;
        } else if ch == b'=' && !in_string {
            let previous = cursor
                .checked_sub(1)
                .and_then(|index| statement.as_bytes().get(index));
            let next = statement.as_bytes().get(cursor + 1);
            if !matches!(previous, Some(b'!' | b'<' | b'>' | b'=')) && next != Some(&b'=') {
                return Some(cursor);
            }
        }
        cursor += 1;
    }
    None
}

fn find_awk_operator(value: &str, operator: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    let operator_bytes = operator.as_bytes();
    let mut cursor = 0usize;
    let mut in_string = false;
    while cursor + operator_bytes.len() <= bytes.len() {
        if bytes[cursor] == b'"' && !is_awk_escaped(value, cursor) {
            in_string = !in_string;
            cursor += 1;
            continue;
        }
        if !in_string && &bytes[cursor..cursor + operator_bytes.len()] == operator_bytes {
            return Some(cursor);
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
    let mut paren_depth = 0usize;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' if !is_awk_escaped(value, cursor) => in_string = !in_string,
            b'(' if !in_string => paren_depth += 1,
            b')' if !in_string => paren_depth = paren_depth.saturating_sub(1),
            ch if ch == delimiter && !in_string && paren_depth == 0 => {
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
    while cursor < source.len() {
        let ch = source.as_bytes()[cursor];
        if ch == b'"' && !is_awk_escaped(source, cursor) {
            in_string = !in_string;
        } else if !in_string {
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

fn take_awk_digits(value: &str) -> Option<&str> {
    let end = value
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_digit())
        .last()
        .map(|(index, ch)| index + ch.len_utf8())?;
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
    let mut paths = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-c" | "--count" => count = true,
            "-d" | "--repeated" => duplicates_only = true,
            "-u" | "--unique" => unique_only = true,
            _ if arg.starts_with('-') && arg.len() > 1 => {
                for flag in arg[1..].chars() {
                    match flag {
                        'c' => count = true,
                        'd' => duplicates_only = true,
                        'u' => unique_only = true,
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
    let mut previous: Option<String> = None;
    let mut group_count = 0;
    for line in input.lines().map(ToString::to_string) {
        if previous.as_ref().is_some_and(|previous| previous == &line) {
            group_count += 1;
        } else {
            emit_uniq_group(
                &mut stdout,
                previous.take(),
                group_count,
                count,
                duplicates_only,
                unique_only,
            );
            previous = Some(line);
            group_count = 1;
        }
    }
    emit_uniq_group(
        &mut stdout,
        previous,
        group_count,
        count,
        duplicates_only,
        unique_only,
    );
    stdout_result(stdout)
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
    let mut operands = Vec::new();
    for arg in args {
        match arg.as_str() {
            "-d" | "--delete" => delete = true,
            "-s" | "--squeeze-repeats" => squeeze = true,
            _ => operands.push(arg.clone()),
        }
    }
    let Some(set1) = operands.first() else {
        return stderr_result(1, "tr: missing operand\n");
    };
    let set1 = expand_tr_set(set1);
    if delete {
        let output = stdin
            .chars()
            .filter(|ch| !set1.contains(ch))
            .collect::<String>();
        return stdout_result(output);
    }
    if squeeze {
        let mut output = String::new();
        let mut previous = None;
        for ch in stdin.chars() {
            if Some(ch) == previous && set1.contains(&ch) {
                continue;
            }
            output.push(ch);
            previous = Some(ch);
        }
        return stdout_result(output);
    }
    let Some(set2) = operands.get(1) else {
        return stderr_result(1, "tr: missing operand after SET1\n");
    };
    let set2 = expand_tr_set(set2);
    let mut output = String::new();
    for ch in stdin.chars() {
        if let Some(index) = set1.iter().position(|candidate| *candidate == ch) {
            output.push(*set2.get(index).or_else(|| set2.last()).unwrap_or(&ch));
        } else {
            output.push(ch);
        }
    }
    stdout_result(output)
}

fn expand_tr_set(value: &str) -> Vec<char> {
    let chars = unescape_tr_set(value).chars().collect::<Vec<_>>();
    let mut expanded = Vec::new();
    let mut index = 0;
    while index < chars.len() {
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
    if options.tab_indent {
        return serde_json::to_string_pretty(value)
            .unwrap_or_else(|_| value.to_string())
            .replace("  ", "\t");
    }
    match value {
        JsonValue::Array(_) | JsonValue::Object(_) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        }
        _ => value.to_string(),
    }
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
            object.insert(key, value);
        } else {
            let key = entry.trim().trim_matches('"').to_string();
            object.insert(
                key.clone(),
                eval_first(value, root, &format!(".{key}"), env)?,
            );
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
        _ => {}
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
    if let Some(inner) = function_arg(expr, "range") {
        let args = split_top_level(inner, ';');
        let (start, end) = if args.len() == 1 {
            (0, args[0].trim().parse::<i64>().unwrap_or(0))
        } else {
            (
                args[0].trim().parse::<i64>().unwrap_or(0),
                args[1].trim().parse::<i64>().unwrap_or(0),
            )
        };
        return Ok(Some(JsonValue::Array(
            (start..end)
                .map(|value| json_number(value as f64))
                .collect(),
        )));
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
            let inside = inside.trim().trim_matches('"');
            current = json_index_or_field(&current, inside).unwrap_or(JsonValue::Null);
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

fn split_top_level(input: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0_i32;
    let mut in_string = false;
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
        match ch {
            '"' => in_string = true,
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
            && let Some(JsonValue::String(key)) = entry.get("key")
        {
            map.insert(
                key.clone(),
                entry.get("value").cloned().unwrap_or(JsonValue::Null),
            );
        }
    }
    JsonValue::Object(map)
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
            let indexes = (0..csv.headers.len())
                .filter(|index| selected.contains(index) != drop)
                .collect::<Vec<_>>();
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
                .map(|value| sql_value_text(value, &options.null_value))
                .collect::<Vec<_>>()
                .join(separator),
        );
        output.push_str(&options.newline);
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
        .join(",");
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

fn sql_value_quote(value: &SqlValue) -> String {
    match &value.raw {
        None => "NULL".to_string(),
        Some(value) if value.parse::<f64>().is_ok() => value.clone(),
        Some(value) => format!("'{}'", value.replace('\'', "''")),
    }
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
    if pattern == "*" {
        return true;
    }
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == text;
    }
    let mut remaining = text;
    if let Some(first) = parts.first()
        && !first.is_empty()
    {
        let Some(stripped) = remaining.strip_prefix(first) else {
            return false;
        };
        remaining = stripped;
    }
    for part in parts.iter().skip(1).take(parts.len().saturating_sub(2)) {
        if part.is_empty() {
            continue;
        }
        let Some(index) = remaining.find(part) else {
            return false;
        };
        remaining = &remaining[index + part.len()..];
    }
    if let Some(last) = parts.last()
        && !last.is_empty()
    {
        return remaining.ends_with(last);
    }
    true
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
        .filter_map(|arg| arg.parse::<f64>().ok())
        .map(|seconds| (seconds * 1000.0) as u64)
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
            merge_json_object(&mut result, arg)?;
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
    let mut output = String::new();
    for (index, ch) in value.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
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
    let mut lines = vec![
        format!("Executor tools: {namespace}"),
        String::new(),
        "USAGE".to_string(),
        format!("  {namespace} <command> [flags]"),
        String::new(),
        "COMMANDS".to_string(),
    ];
    for subcommand in subcommands {
        lines.push(format!(
            "  {:<16}{}",
            subcommand.name,
            subcommand.description.as_deref().unwrap_or("")
        ));
    }
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
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn math_executor() -> JustBashExecutor {
        JustBashExecutor::new().with_tool(
            "math.add",
            JustBashExecutorTool::new(Some("Add two numbers"), |args| {
                let a = args.get("a").and_then(JsonValue::as_i64).unwrap_or(0);
                let b = args.get("b").and_then(JsonValue::as_i64).unwrap_or(0);
                Ok(json!({ "sum": a + b }))
            }),
        )
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

        let help = bash.exec("math --help", JustBashExecOptions::new());
        assert!(help.stdout.contains("Executor tools: math"));
        assert!(help.stdout.contains("Add two numbers"));
    }
}
