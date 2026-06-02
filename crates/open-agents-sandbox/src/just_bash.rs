//! Open Agents adapter for the shared Just Bash in-process backend.
//!
//! This module bridges the Open Agents [`Sandbox`] trait to `crates/just-bash`.
//! It keeps provider selection explicit: the default `just-bash` backend runs
//! against a process-local virtual filesystem, while `local` and `vercel`
//! remain opt-in backends. It never falls back to host `/bin/bash` or arbitrary
//! host processes.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use ::just_bash::{
    DirentEntry, FileStat, JustBashError, JustBashErrorKind, JustBashExecOptions,
    JustBashExecResult, JustBashSession, JustBashSessionOptions, MkdirOptions, is_path_within_root,
    normalize_path, resolve_path,
};

use crate::{
    DEFAULT_EXEC_TIMEOUT_MS, DEFAULT_MAX_OUTPUT_LENGTH, Sandbox, SandboxDetachedCommand,
    SandboxDetachedOptions, SandboxDirEntry, SandboxDirEntryKind, SandboxError, SandboxExecOptions,
    SandboxExecResult, SandboxMkdirOptions, SandboxResult, SandboxState, SandboxStats,
    SandboxTimeoutExtension, SandboxType, SnapshotResult,
};

/// Default virtual working directory for the in-process Just Bash backend.
pub const JUST_BASH_DEFAULT_WORKING_DIRECTORY: &str = "/workspace";

/// Options for [`JustBashSandbox`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JustBashSandboxOptions {
    /// Environment variables exposed to commands.
    pub env: BTreeMap<String, String>,
    /// Current git branch, when known.
    pub current_branch: Option<String>,
    /// Configured timeout in milliseconds.
    pub timeout_ms: Option<u64>,
    /// Current expiration timestamp in milliseconds since Unix epoch.
    pub expires_at_ms: Option<u64>,
}

impl JustBashSandboxOptions {
    /// Creates empty Just Bash options.
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

    /// Sets the backend timeout in milliseconds.
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self.expires_at_ms = Some(now_ms().saturating_add(timeout_ms));
        self
    }
}

/// In-process Open Agents sandbox backed by `crates/just-bash`.
///
/// Filesystem state persists by workspace id for durable reconnects, while each
/// command executes through a fresh Just Bash shell state. The backend is
/// intentionally virtual-only and does not spawn host shell commands.
pub struct JustBashSandbox {
    workspace_id: String,
    working_directory: String,
    env: BTreeMap<String, String>,
    current_branch: Option<String>,
    timeout_ms: Option<u64>,
    expires_at_ms: Mutex<Option<u64>>,
    inner: Arc<JustBashWorkspace>,
}

impl fmt::Debug for JustBashSandbox {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JustBashSandbox")
            .field("workspace_id", &self.workspace_id)
            .field("working_directory", &self.working_directory)
            .field("env_keys", &self.env.keys().collect::<Vec<_>>())
            .field("current_branch", &self.current_branch)
            .field("timeout_ms", &self.timeout_ms)
            .field("expires_at_ms", &self.expires_at_ms())
            .finish_non_exhaustive()
    }
}

impl JustBashSandbox {
    /// Creates a Just Bash sandbox for a virtual workspace id.
    pub fn new(workspace_id: impl Into<String>) -> SandboxResult<Self> {
        Self::with_options(
            workspace_id,
            JUST_BASH_DEFAULT_WORKING_DIRECTORY,
            JustBashSandboxOptions::new(),
        )
    }

    /// Creates a Just Bash sandbox with explicit virtual working directory and options.
    pub fn with_options(
        workspace_id: impl Into<String>,
        working_directory: impl Into<String>,
        mut options: JustBashSandboxOptions,
    ) -> SandboxResult<Self> {
        let workspace_id = workspace_id.into();
        if workspace_id.trim().is_empty() {
            return Err(internal("Just Bash workspace id must not be empty"));
        }
        let working_directory = normalize_absolute_virtual_path(&working_directory.into())
            .map_err(|message| {
                internal(format!("invalid Just Bash working directory: {message}"))
            })?;
        if !is_path_within_root(&working_directory, JUST_BASH_DEFAULT_WORKING_DIRECTORY) {
            return Err(SandboxError::PathOutsideWorkspace {
                path: working_directory,
                workspace: JUST_BASH_DEFAULT_WORKING_DIRECTORY.to_string(),
            });
        }
        if options.expires_at_ms.is_none() {
            options.expires_at_ms = options
                .timeout_ms
                .map(|timeout_ms| now_ms().saturating_add(timeout_ms));
        }

        let inner = workspace_for_id(&workspace_id);
        inner
            .session
            .mkdir(&working_directory, MkdirOptions { recursive: true })
            .map_err(|error| map_just_bash_error("mkdir", error))?;

        Ok(Self {
            workspace_id,
            working_directory,
            env: options.env,
            current_branch: options.current_branch,
            timeout_ms: options.timeout_ms,
            expires_at_ms: Mutex::new(options.expires_at_ms),
            inner,
        })
    }

    fn ensure_running(&self) -> SandboxResult<()> {
        let stopped = lock(&self.inner.stopped, "Just Bash stop state")?;
        if *stopped {
            return Err(SandboxError::Stopped {
                sandbox_type: SandboxType::JustBash,
            });
        }
        Ok(())
    }

    fn resolve_path(&self, path: &str) -> SandboxResult<String> {
        resolve_under_workspace(&self.working_directory, &self.working_directory, path)
    }

    fn resolve_cwd(&self, cwd: Option<&str>) -> SandboxResult<String> {
        let cwd = match cwd {
            Some(cwd) => {
                resolve_under_workspace(&self.working_directory, &self.working_directory, cwd)?
            }
            None => self.working_directory.clone(),
        };
        let stat = self
            .inner
            .session
            .stat(&cwd)
            .map_err(|error| map_just_bash_error("cwd", error))?;
        if !stat.is_directory {
            return Err(SandboxError::NotDirectory { path: cwd });
        }
        Ok(cwd)
    }
}

impl Sandbox for JustBashSandbox {
    fn sandbox_type(&self) -> SandboxType {
        SandboxType::JustBash
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
            "- Just Bash runs in-process with a virtual filesystem and working directory /workspace\n- Commands never fall back to host /bin/bash or host processes\n- Open Agents smoke coverage exercises echo, pwd, cat, printf, redirection, mkdir, ls, touch, cd, true, false, env reset, cwd reset, and shell-shaped failures; broader command parity is tracked in docs/open-agents/just-bash-parity.md",
        )
    }

    fn host(&self) -> Option<&str> {
        None
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
        let resolved = self.resolve_path(path)?;
        self.inner
            .session
            .read_file_buffer(&resolved)
            .map_err(|error| map_just_bash_error("read", error))
    }

    fn write_file(&self, path: &str, content: &str) -> SandboxResult<()> {
        self.ensure_running()?;
        let resolved = self.resolve_path(path)?;
        self.inner
            .session
            .write_file(&resolved, content)
            .map_err(|error| map_just_bash_error("write", error))
    }

    fn stat(&self, path: &str) -> SandboxResult<SandboxStats> {
        self.ensure_running()?;
        let resolved = self.resolve_path(path)?;
        self.inner
            .session
            .stat(&resolved)
            .map(to_sandbox_stats)
            .map_err(|error| map_just_bash_error("stat", error))
    }

    fn access(&self, path: &str) -> SandboxResult<()> {
        self.ensure_running()?;
        let resolved = self.resolve_path(path)?;
        self.inner
            .session
            .stat(&resolved)
            .map(|_| ())
            .map_err(|error| map_just_bash_error("access", error))
    }

    fn mkdir(&self, path: &str, options: SandboxMkdirOptions) -> SandboxResult<()> {
        self.ensure_running()?;
        let resolved = self.resolve_path(path)?;
        self.inner
            .session
            .mkdir(
                &resolved,
                MkdirOptions {
                    recursive: options.recursive,
                },
            )
            .map_err(|error| map_just_bash_error("mkdir", error))
    }

    fn read_dir(&self, path: &str) -> SandboxResult<Vec<SandboxDirEntry>> {
        self.ensure_running()?;
        let resolved = self.resolve_path(path)?;
        self.inner
            .session
            .readdir_with_file_types(&resolved)
            .map(|entries| entries.into_iter().map(to_sandbox_dir_entry).collect())
            .map_err(|error| map_just_bash_error("read_dir", error))
    }

    fn exec(&self, options: SandboxExecOptions) -> SandboxResult<SandboxExecResult> {
        self.ensure_running()?;
        let cwd = self.resolve_cwd(options.cwd.as_deref())?;
        let timeout_ms = options
            .timeout_ms
            .or(self.timeout_ms)
            .unwrap_or(DEFAULT_EXEC_TIMEOUT_MS);
        let result = self.inner.session.exec(
            &options.command,
            JustBashExecOptions::new()
                .with_envs(self.env.clone())
                .with_cwd(cwd)
                .with_timeout_ms(timeout_ms),
        );
        Ok(to_sandbox_exec_result(result))
    }

    fn exec_detached(
        &self,
        _options: SandboxDetachedOptions,
    ) -> SandboxResult<SandboxDetachedCommand> {
        Err(SandboxError::UnsupportedOperation {
            operation: "exec_detached".to_string(),
            sandbox_type: SandboxType::JustBash,
        })
    }

    fn set_github_auth_token(&self, _token: Option<&str>) -> SandboxResult<()> {
        self.ensure_running()
    }

    fn domain(&self, _port: u16) -> Option<String> {
        None
    }

    fn stop(&self) -> SandboxResult<()> {
        let mut stopped = lock(&self.inner.stopped, "Just Bash stop state")?;
        *stopped = true;
        if let Ok(mut expires_at_ms) = self.expires_at_ms.lock() {
            *expires_at_ms = None;
        }
        Ok(())
    }

    fn extend_timeout(&self, additional_ms: u64) -> SandboxResult<SandboxTimeoutExtension> {
        self.ensure_running()?;
        let mut expires_at_ms = lock(&self.expires_at_ms, "Just Bash expiration")?;
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
            sandbox_type: SandboxType::JustBash,
        })
    }

    fn state(&self) -> SandboxState {
        SandboxState::JustBash {
            workspace_id: self.workspace_id.clone(),
            working_directory: self.working_directory.clone(),
            current_branch: self.current_branch.clone(),
            expires_at: self.expires_at_ms(),
        }
    }
}

#[derive(Debug)]
struct JustBashWorkspace {
    session: JustBashSession,
    stopped: Mutex<bool>,
}

impl Default for JustBashWorkspace {
    fn default() -> Self {
        Self {
            session: JustBashSession::with_options(
                JustBashSessionOptions::new()
                    .with_cwd(JUST_BASH_DEFAULT_WORKING_DIRECTORY)
                    .with_env("HOME", JUST_BASH_DEFAULT_WORKING_DIRECTORY)
                    .with_default_timeout_ms(DEFAULT_EXEC_TIMEOUT_MS)
                    .with_max_output_length(DEFAULT_MAX_OUTPUT_LENGTH),
            ),
            stopped: Mutex::new(false),
        }
    }
}

fn normalize_absolute_virtual_path(path: &str) -> Result<String, String> {
    if path.contains('\0') {
        return Err("path must not contain NUL bytes".to_string());
    }
    if !path.starts_with('/') {
        return Err("path must be absolute".to_string());
    }
    Ok(normalize_path(path))
}

fn resolve_under_workspace(workspace: &str, cwd: &str, path: &str) -> SandboxResult<String> {
    if path.contains('\0') {
        return Err(SandboxError::Io {
            operation: "resolve".to_string(),
            path: path.to_string(),
            message: "path must not contain NUL bytes".to_string(),
        });
    }
    let resolved = resolve_path(cwd, path);
    if is_path_within_root(&resolved, workspace) {
        Ok(resolved)
    } else {
        Err(SandboxError::PathOutsideWorkspace {
            path: path.to_string(),
            workspace: workspace.to_string(),
        })
    }
}

fn to_sandbox_stats(stat: FileStat) -> SandboxStats {
    SandboxStats {
        is_directory: stat.is_directory,
        is_file: stat.is_file,
        size: stat.size as u64,
        mtime_ms: stat.mtime,
    }
}

fn to_sandbox_dir_entry(entry: DirentEntry) -> SandboxDirEntry {
    let kind = if entry.is_directory {
        SandboxDirEntryKind::Directory
    } else if entry.is_file {
        SandboxDirEntryKind::File
    } else if entry.is_symbolic_link {
        SandboxDirEntryKind::Symlink
    } else {
        SandboxDirEntryKind::Other
    };
    SandboxDirEntry {
        name: entry.name,
        kind,
    }
}

fn to_sandbox_exec_result(result: JustBashExecResult) -> SandboxExecResult {
    SandboxExecResult {
        success: result.success(),
        exit_code: Some(result.exit_code),
        stdout: result.stdout,
        stderr: result.stderr,
        truncated: result.metadata.truncated,
    }
}

fn map_just_bash_error(operation: &str, error: JustBashError) -> SandboxError {
    match error.kind() {
        JustBashErrorKind::NotDirectory => SandboxError::NotDirectory {
            path: error.path().to_string(),
        },
        _ => SandboxError::Io {
            operation: operation.to_string(),
            path: error.path().to_string(),
            message: error.to_string(),
        },
    }
}

fn workspace_for_id(workspace_id: &str) -> Arc<JustBashWorkspace> {
    static WORKSPACES: OnceLock<Mutex<BTreeMap<String, Arc<JustBashWorkspace>>>> = OnceLock::new();
    let registry = WORKSPACES.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut registry = registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(workspace) = registry.get(workspace_id) {
        return Arc::clone(workspace);
    }
    let workspace = Arc::new(JustBashWorkspace::default());
    registry.insert(workspace_id.to_string(), Arc::clone(&workspace));
    workspace
}

fn lock<'a, T>(mutex: &'a Mutex<T>, name: &str) -> SandboxResult<std::sync::MutexGuard<'a, T>> {
    mutex
        .lock()
        .map_err(|_| internal(format!("{name} lock poisoned")))
}

fn internal(message: impl Into<String>) -> SandboxError {
    SandboxError::Internal {
        message: message.into(),
    }
}

fn now_ms() -> u64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    u64::try_from(duration.as_millis()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SandboxConnectConfig, connect_sandbox};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_WORKSPACE: AtomicU64 = AtomicU64::new(0);

    fn workspace_id(label: &str) -> String {
        format!(
            "just-bash-{label}-{}-{}",
            std::process::id(),
            NEXT_WORKSPACE.fetch_add(1, Ordering::Relaxed)
        )
    }

    #[test]
    fn just_bash_executes_echo_pwd_and_cat_without_host_process() {
        let sandbox = JustBashSandbox::new(workspace_id("basic")).expect("just bash sandbox");
        sandbox
            .write_file("notes.txt", "from virtual fs")
            .expect("write virtual file");

        let echo = sandbox
            .exec(SandboxExecOptions::new("echo hello world"))
            .expect("echo");
        assert!(echo.success);
        assert_eq!(echo.stdout, "hello world\n");

        let pwd = sandbox.exec(SandboxExecOptions::new("pwd")).expect("pwd");
        assert!(pwd.success);
        assert_eq!(pwd.stdout, "/workspace\n");

        let cat = sandbox
            .exec(SandboxExecOptions::new("cat notes.txt"))
            .expect("cat");
        assert!(cat.success);
        assert_eq!(cat.stdout, "from virtual fs");

        let host_shell = sandbox
            .exec(SandboxExecOptions::new("/bin/bash -lc 'echo host'"))
            .expect("host shell probe");
        assert!(!host_shell.success);
        assert_eq!(host_shell.exit_code, Some(127));
        assert!(host_shell.stderr.contains("No such file or directory"));
    }

    #[test]
    fn open_agents_just_bash_blocks_js_python_host_runtime_without_fallback() {
        let sandbox =
            JustBashSandbox::new(workspace_id("host-runtime")).expect("just bash sandbox");
        let probes = [
            "js-exec -c \"console.log('host-js')\"",
            "node -e \"console.log('host-node')\"",
            "python3 -c \"print('host-python3')\"",
            "python -c \"print('host-python')\"",
            "/usr/bin/node -e \"console.log('host-node')\"",
            "/usr/bin/python3 -c \"print('host-python3')\"",
            "/usr/bin/env node -e \"console.log('host-node')\"",
        ];

        for probe in probes {
            let result = sandbox
                .exec(SandboxExecOptions::new(probe))
                .expect("host runtime probe");
            assert!(!result.success, "{probe}");
            assert_eq!(result.exit_code, Some(127), "{probe}");
            assert_eq!(result.stdout, "", "{probe}");
            assert!(result.stderr.contains("command not found"), "{probe}");
            assert!(!result.stderr.contains("host-js"), "{probe}");
            assert!(!result.stderr.contains("host-node"), "{probe}");
            assert!(!result.stderr.contains("host-python"), "{probe}");
        }
    }

    #[test]
    fn just_bash_persists_virtual_files_across_exec_calls() {
        let sandbox = JustBashSandbox::new(workspace_id("persistence")).expect("just bash sandbox");

        let write = sandbox
            .exec(SandboxExecOptions::new("printf 'one' > out.txt"))
            .expect("write");
        assert!(write.success);
        let append = sandbox
            .exec(SandboxExecOptions::new("printf '\\ntwo' >> out.txt"))
            .expect("append");
        assert!(append.success);
        let read = sandbox
            .exec(SandboxExecOptions::new("cat out.txt"))
            .expect("read");

        assert_eq!(read.stdout, "one\ntwo");
    }

    #[test]
    fn just_bash_resets_env_and_cwd_between_exec_calls() {
        let sandbox = JustBashSandbox::new(workspace_id("reset")).expect("just bash sandbox");
        sandbox
            .mkdir("nested", SandboxMkdirOptions::recursive())
            .expect("mkdir");

        let first = sandbox
            .exec(SandboxExecOptions::new(
                "export TEMP_VALUE=present; cd nested; pwd; echo $TEMP_VALUE",
            ))
            .expect("first exec");
        assert_eq!(first.stdout, "/workspace/nested\npresent\n");

        let second = sandbox
            .exec(SandboxExecOptions::new("pwd; echo $TEMP_VALUE"))
            .expect("second exec");
        assert_eq!(second.stdout, "/workspace\n\n");
    }

    #[test]
    fn just_bash_maps_failures_to_shell_shaped_exec_results() {
        let sandbox = JustBashSandbox::new(workspace_id("failures")).expect("just bash sandbox");

        let missing = sandbox
            .exec(SandboxExecOptions::new("cat missing.txt"))
            .expect("cat missing");
        assert!(!missing.success);
        assert_eq!(missing.exit_code, Some(1));
        assert!(missing.stderr.contains("No such file or directory"));

        let unsupported = sandbox
            .exec(SandboxExecOptions::new("python -c 'print(1)'"))
            .expect("unsupported");
        assert!(!unsupported.success);
        assert_eq!(unsupported.exit_code, Some(127));
        assert_eq!(unsupported.stderr, "bash: python: command not found\n");
    }

    #[test]
    fn connect_sandbox_reuses_just_bash_virtual_workspace_by_id() {
        let workspace_id = workspace_id("connect");
        let first = connect_sandbox(SandboxConnectConfig::new(SandboxState::JustBash {
            workspace_id: workspace_id.clone(),
            working_directory: JUST_BASH_DEFAULT_WORKING_DIRECTORY.to_string(),
            current_branch: None,
            expires_at: None,
        }))
        .expect("connect first");
        first.write_file("saved.txt", "persisted").expect("write");
        drop(first);

        let second = connect_sandbox(SandboxConnectConfig::new(SandboxState::JustBash {
            workspace_id,
            working_directory: JUST_BASH_DEFAULT_WORKING_DIRECTORY.to_string(),
            current_branch: None,
            expires_at: None,
        }))
        .expect("connect second");

        assert_eq!(second.read_file("saved.txt").expect("read"), "persisted");
    }
}
