//! Sandbox boundary contracts for the Rust Open Agents Slack remote agent.
//!
//! The agent runtime runs outside the sandbox. This crate owns the data shapes
//! and operation names for the boundary so tools do not depend on Slack or on a
//! concrete sandbox provider.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// Bucket that owns the first sandbox connector implementation.
pub const OWNER_BUCKET: u8 = 3;

/// Optional base snapshot id for Vercel-backed sandboxes.
pub const VERCEL_SANDBOX_BASE_SNAPSHOT_ID_ENV: &str = "VERCEL_SANDBOX_BASE_SNAPSHOT_ID";

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

/// Git source used when provisioning a sandbox.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxSource {
    /// Repository clone URL.
    pub repo: String,
    /// Existing branch to check out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// New branch to create during setup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_branch: Option<String>,
}

/// Portable shell execution result.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxExecResult {
    /// Whether the command exited successfully.
    pub success: bool,
    /// Process exit code, absent when the provider cannot report one.
    pub exit_code: Option<i32>,
    /// Captured stdout.
    pub stdout: String,
    /// Captured stderr.
    pub stderr: String,
    /// Whether stdout or stderr was truncated.
    pub truncated: bool,
}

/// Operation families the sandbox crate must expose.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sandbox_context_round_trips_with_optional_fields() {
        let context = SandboxContext::new(json!({"type": "vercel"}), "/workspace")
            .with_current_branch("main")
            .with_environment_details("Vercel Sandbox");

        let encoded = serde_json::to_string(&context).unwrap();
        let decoded: SandboxContext = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.current_branch.as_deref(), Some("main"));
        assert_eq!(
            decoded.environment_details.as_deref(),
            Some("Vercel Sandbox")
        );
    }

    #[test]
    fn operation_serializes_as_snake_case() {
        let encoded = serde_json::to_string(&SandboxOperation::ExecDetached).unwrap();

        assert_eq!(encoded, "\"exec_detached\"");
    }
}
