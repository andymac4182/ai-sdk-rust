//! Just Bash-style in-process shell session for Open Agents.
//!
//! This module owns the JB-02 core contract only: session state, exec
//! options/results, cancellation/time limits, default metadata, and an inline
//! executor tool bridge. It intentionally does not replace any Open Agents
//! runtime path; JB-07 can choose where to wire this backend.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};

use crate::commands::CommandRegistry;
use crate::encoding::OutputPayload;
use crate::error::{JustBashError, JustBashErrorKind, JustBashResult};
use crate::fs::{CpOptions, MkdirOptions, RmOptions, VirtualFileSystem};
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

    /// Writes a UTF-8 file to the session filesystem.
    pub fn write_file(&self, path: &str, content: &str) -> JustBashResult<()> {
        let mut fs = self.inner.fs.lock().map_err(|_| lock_poisoned_error())?;
        fs.write_file(path, content)
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
        "grep" => command_grep(state, &tokens[1..], &stdin),
        "fgrep" | "egrep" => command_grep(state, &tokens[1..], &stdin),
        "rg" => command_rg(state, &tokens[1..], &stdin),
        "sed" => command_sed(state, &tokens[1..], &stdin),
        "awk" => command_awk(state, &tokens[1..], &stdin),
        "head" => command_head(state, &tokens[1..], &stdin),
        "wc" => command_wc(&tokens[1..], &stdin),
        "ls" => command_ls(state, &tokens[1..]),
        "mkdir" => command_mkdir(state, &tokens[1..]),
        "touch" => command_touch(state, &tokens[1..]),
        "rm" => command_rm(state, &tokens[1..]),
        "cp" => command_cp(state, &tokens[1..]),
        "mv" => command_mv(state, &tokens[1..]),
        "find" => command_find(state, &tokens[1..]),
        "read" => command_read(state, &tokens[1..], &stdin),
        "jq" => command_jq(state, &tokens[1..], &stdin),
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
    if args.is_empty() {
        return stdout_result(stdin.to_string());
    }
    let fs = match state.session.inner.fs.lock() {
        Ok(fs) => fs,
        Err(_) => return stderr_result(1, "cat: filesystem lock poisoned\n"),
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    let mut exit_code = 0;
    for path in args {
        let path = resolve_path(&state.cwd, path);
        match fs.read_file(&path) {
            Ok(content) => stdout.push_str(&content),
            Err(_) => {
                stderr.push_str(&format!("cat: {path}: No such file or directory\n"));
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

fn command_grep(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let mut ignore_case = false;
    let mut invert = false;
    let mut line_number = false;
    let mut count_only = false;
    let mut index = 0;
    while let Some(arg) = args.get(index) {
        match arg.as_str() {
            "-i" | "--ignore-case" => ignore_case = true,
            "-v" | "--invert-match" => invert = true,
            "-n" | "--line-number" => line_number = true,
            "-c" | "--count" => count_only = true,
            _ if arg.starts_with('-') && arg.len() > 1 => {
                for flag in arg[1..].chars() {
                    match flag {
                        'i' => ignore_case = true,
                        'v' => invert = true,
                        'n' => line_number = true,
                        'c' => count_only = true,
                        _ => {}
                    }
                }
            }
            _ => break,
        }
        index += 1;
    }

    let Some(pattern) = args.get(index) else {
        return stderr_result(2, "grep: missing pattern\n");
    };
    let files = &args[index + 1..];
    let input = if files.is_empty() {
        vec![("".to_string(), stdin.to_string())]
    } else {
        let fs = match state.session.inner.fs.lock() {
            Ok(fs) => fs,
            Err(_) => return stderr_result(1, "grep: filesystem lock poisoned\n"),
        };
        let mut inputs = Vec::new();
        for path in files {
            let path = resolve_path(&state.cwd, path);
            match fs.read_file(&path) {
                Ok(content) => inputs.push((path, content)),
                Err(_) => {
                    return stderr_result(2, format!("grep: {path}: No such file or directory\n"));
                }
            }
        }
        inputs
    };
    let mut stdout = String::new();
    let mut matches = 0;
    for (path, text) in input {
        for (line_index, line) in text.lines().enumerate() {
            let matched = line_matches(line, pattern, ignore_case);
            if matched ^ invert {
                matches += 1;
                if count_only {
                    continue;
                }
                if files.len() > 1 {
                    stdout.push_str(&path);
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
    }
    if count_only {
        stdout.push_str(&format!("{matches}\n"));
    }
    CommandResult {
        exit_code: if matches == 0 { 1 } else { 0 },
        stdout,
        ..CommandResult::default()
    }
}

fn command_wc(args: &[String], stdin: &str) -> CommandResult {
    if args.iter().any(|arg| arg == "-l") {
        stdout_result(format!("{}\n", stdin.lines().count()))
    } else {
        stdout_result(format!("{}\n", stdin.len()))
    }
}

fn command_ls(state: &ExecState<'_>, args: &[String]) -> CommandResult {
    let path = args
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .unwrap_or(".");
    let path = resolve_path(&state.cwd, path);
    match state.session.inner.fs.lock() {
        Ok(fs) => match fs.readdir(&path) {
            Ok(entries) => {
                let mut stdout = entries.join("\n");
                if !stdout.is_empty() {
                    stdout.push('\n');
                }
                stdout_result(stdout)
            }
            Err(_) => stderr_result(2, format!("ls: {path}: No such file or directory\n")),
        },
        Err(_) => stderr_result(1, "ls: filesystem lock poisoned\n"),
    }
}

fn command_mkdir(state: &mut ExecState<'_>, args: &[String]) -> CommandResult {
    let mut fs = match state.session.inner.fs.lock() {
        Ok(fs) => fs,
        Err(_) => return stderr_result(1, "mkdir: filesystem lock poisoned\n"),
    };
    for path in args.iter().filter(|arg| !arg.starts_with('-')) {
        if let Err(error) = fs.mkdir(
            &resolve_path(&state.cwd, path),
            MkdirOptions { recursive: true },
        ) {
            return stderr_result(1, format!("mkdir: {error}\n"));
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
    let mut fs = match state.session.inner.fs.lock() {
        Ok(fs) => fs,
        Err(_) => return stderr_result(1, "rm: filesystem lock poisoned\n"),
    };
    for path in args.iter().filter(|arg| !arg.starts_with('-')) {
        if let Err(error) = fs.rm(
            &resolve_path(&state.cwd, path),
            RmOptions {
                recursive: true,
                force: false,
            },
        ) {
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
        return stderr_result(1, "cp: missing file operand\n");
    }
    let src = resolve_path(&state.cwd, paths[0]);
    let dest = resolve_path(&state.cwd, paths[1]);
    match state.session.inner.fs.lock() {
        Ok(mut fs) => match fs.cp(&src, &dest, CpOptions { recursive }) {
            Ok(()) => CommandResult::default(),
            Err(error) => stderr_result(1, format!("cp: {error}\n")),
        },
        Err(_) => stderr_result(1, "cp: filesystem lock poisoned\n"),
    }
}

fn command_mv(state: &mut ExecState<'_>, args: &[String]) -> CommandResult {
    let paths = args
        .iter()
        .filter(|arg| !arg.starts_with('-'))
        .collect::<Vec<_>>();
    if paths.len() < 2 {
        return stderr_result(1, "mv: missing file operand\n");
    }
    let src = resolve_path(&state.cwd, paths[0]);
    let dest = resolve_path(&state.cwd, paths[1]);
    match state.session.inner.fs.lock() {
        Ok(mut fs) => match fs.mv(&src, &dest) {
            Ok(()) => CommandResult::default(),
            Err(error) => stderr_result(1, format!("mv: {error}\n")),
        },
        Err(_) => stderr_result(1, "mv: filesystem lock poisoned\n"),
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
    let Some(pattern) = args.iter().find(|arg| !arg.starts_with('-')) else {
        return stderr_result(2, "rg: missing pattern\n");
    };
    let roots = args
        .iter()
        .skip_while(|arg| *arg != pattern)
        .skip(1)
        .filter(|arg| !arg.starts_with('-'))
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return command_grep(state, std::slice::from_ref(pattern), stdin);
    }
    let fs = match state.session.inner.fs.lock() {
        Ok(fs) => fs,
        Err(_) => return stderr_result(1, "rg: filesystem lock poisoned\n"),
    };
    let mut stdout = String::new();
    let mut matches = 0;
    for root in roots {
        let root = resolve_path(&state.cwd, root);
        let mut paths = fs
            .get_all_paths()
            .into_iter()
            .filter(|path| path == &root || path.starts_with(&format!("{root}/")))
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            let Ok(stat) = fs.stat(&path) else {
                continue;
            };
            if !stat.is_file {
                continue;
            }
            if let Ok(content) = fs.read_file(&path) {
                for line in content
                    .lines()
                    .filter(|line| line_matches(line, pattern, false))
                {
                    matches += 1;
                    stdout.push_str(&path);
                    stdout.push(':');
                    stdout.push_str(line);
                    stdout.push('\n');
                }
            }
        }
    }
    CommandResult {
        exit_code: if matches == 0 { 1 } else { 0 },
        stdout,
        ..CommandResult::default()
    }
}

fn command_sed(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let Some(script) = args.first() else {
        return stderr_result(1, "sed: missing script\n");
    };
    let input = match collect_text_inputs(state, &args[1..], stdin) {
        Ok(input) => input,
        Err(error) => return stderr_result(1, format!("sed: {error}\n")),
    };
    let Some((from, to, global)) = parse_sed_substitution(script) else {
        return stderr_result(1, "sed: unsupported script\n");
    };
    let output = if global {
        input.replace(&from, &to)
    } else {
        input
            .lines()
            .map(|line| line.replacen(&from, &to, 1))
            .collect::<Vec<_>>()
            .join("\n")
            + if input.ends_with('\n') { "\n" } else { "" }
    };
    stdout_result(output)
}

fn command_awk(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let Some(program) = args.first() else {
        return stderr_result(1, "awk: missing program\n");
    };
    let input = match collect_text_inputs(state, &args[1..], stdin) {
        Ok(input) => input,
        Err(error) => return stderr_result(1, format!("awk: {error}\n")),
    };
    let field = if program.contains("$0") {
        Some(0)
    } else {
        program.split('$').nth(1).and_then(|rest| {
            rest.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .ok()
        })
    };
    let Some(field) = field else {
        return stderr_result(1, "awk: unsupported program\n");
    };
    let mut stdout = String::new();
    for line in input.lines() {
        if field == 0 {
            stdout.push_str(line);
        } else if let Some(value) = line.split_whitespace().nth(field - 1) {
            stdout.push_str(value);
        }
        stdout.push('\n');
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
    let input = match collect_text_inputs(state, &paths, stdin) {
        Ok(input) => input,
        Err(error) => return stderr_result(1, format!("head: {error}\n")),
    };
    let mut stdout = input.lines().take(lines).collect::<Vec<_>>().join("\n");
    if !stdout.is_empty() {
        stdout.push('\n');
    }
    stdout_result(stdout)
}

fn command_read(state: &mut ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let Some(name) = args.first() else {
        return CommandResult::default();
    };
    let value = stdin.lines().next().unwrap_or_default().to_string();
    state.env.insert(name.clone(), value);
    CommandResult::default()
}

fn command_jq(state: &ExecState<'_>, args: &[String], stdin: &str) -> CommandResult {
    let Some(filter) = args.first() else {
        return stderr_result(2, "jq: missing filter\n");
    };
    let input = match collect_text_inputs(state, &args[1..], stdin) {
        Ok(input) => input,
        Err(error) => return stderr_result(1, format!("jq: {error}\n")),
    };
    let Ok(value) = serde_json::from_str::<JsonValue>(&input) else {
        return stderr_result(4, "jq: invalid JSON\n");
    };
    if filter == "." {
        return stdout_result(format!("{value}\n"));
    }
    let Some(field) = filter.strip_prefix('.') else {
        return stderr_result(3, "jq: unsupported filter\n");
    };
    let selected = value.get(field).cloned().unwrap_or(JsonValue::Null);
    let rendered = match selected {
        JsonValue::String(value) => value,
        other => other.to_string(),
    };
    stdout_result(format!("{rendered}\n"))
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

fn line_matches(line: &str, pattern: &str, ignore_case: bool) -> bool {
    if ignore_case {
        line.to_ascii_lowercase()
            .contains(&pattern.to_ascii_lowercase())
    } else {
        line.contains(pattern)
    }
}

fn parse_sed_substitution(script: &str) -> Option<(String, String, bool)> {
    let mut chars = script.chars();
    if chars.next()? != 's' {
        return None;
    }
    let delimiter = chars.next()?;
    let rest = chars.as_str();
    let (from, rest) = rest.split_once(delimiter)?;
    let (to, flags) = rest.split_once(delimiter)?;
    Some((from.to_string(), to.to_string(), flags.contains('g')))
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
